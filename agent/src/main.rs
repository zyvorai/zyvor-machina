// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#![allow(clippy::result_large_err)]

use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};

use clap::{Parser, Subcommand};
use machina_agent::console_ws::{self, ConsoleProxyState};
use machina_agent::grpc::AgentService;
use machina_agent::libvirt_ops::LibvirtCtx;
use machina_agent::pb::host_agent_server::HostAgentServer;
use machina_agent::state::shared_state;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};
use tracing::info;

#[derive(Parser)]
#[command(
    name = "machina-agent",
    about = "Machina host agent (libvirt executor)"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[arg(long, env = "MACHINA_AGENT_LISTEN", default_value = "127.0.0.1:50051")]
    listen: String,
    #[arg(
        long,
        env = "MACHINA_AGENT_CONSOLE_LISTEN",
        default_value = "127.0.0.1:50052"
    )]
    console_listen: String,
    #[arg(long, env = "MACHINA_LIBVIRT_URI", default_value = "qemu:///system")]
    libvirt_uri: String,
    #[arg(long, env = "MACHINA_AGENT_HOSTNAME")]
    hostname: Option<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Register with controller and run gRPC + console proxy (default).
    Serve,
    /// Join a controller cluster using an enrollment token.
    Join {
        #[arg(long)]
        controller: String,
        #[arg(long)]
        token: String,
        #[arg(long)]
        address: Option<String>,
        /// Make this agent reachable by a controller on another machine: listen on this host's
        /// own address (never 0.0.0.0) instead of loopback. Off unless given; takes effect only
        /// together with an agent token received from the controller.
        #[arg(long)]
        expose: bool,
        /// Also expose the console port (VNC/serial) on this host's address. It is plaintext and a
        /// serial console is a root shell: leave it off unless the network between the machines is trusted.
        #[arg(long)]
        expose_console: bool,
        /// SHA-256 fingerprint of the controller's CA (from the Add Host command). With an https://
        /// controller URL the node verifies the controller by it, sends a certificate request and
        /// serves gRPC over mutual TLS with the certificate it gets back.
        #[arg(long)]
        ca_sha256: Option<String>,
        /// Where the node's key, certificate and the CA are written.
        #[arg(long, default_value = "/etc/machina/pki")]
        pki_dir: String,
        /// File the services read MACHINA_AGENT_TOKEN from.
        #[arg(long, default_value = machina_agent::enrol::DEFAULT_ENV_FILE)]
        env_file: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // One process-wide TLS provider: the gRPC server (mTLS) and the join client both need it.
    let _ = rustls::crypto::ring::default_provider().install_default();
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    if let Some(Command::Join {
        controller,
        token,
        address,
        expose,
        expose_console,
        ca_sha256,
        pki_dir,
        env_file,
    }) = &cli.command
    {
        return run_join(
            &cli,
            controller,
            token,
            address.as_deref(),
            JoinOpts {
                expose: *expose,
                expose_console: *expose_console,
                ca_sha256: ca_sha256.as_deref(),
                pki_dir,
            },
            env_file,
        )
        .await;
    }

    run_serve(&cli).await
}

struct JoinOpts<'a> {
    expose: bool,
    expose_console: bool,
    ca_sha256: Option<&'a str>,
    pki_dir: &'a str,
}

async fn run_join(
    cli: &Cli,
    controller: &str,
    token: &str,
    address: Option<&str>,
    opts: JoinOpts<'_>,
    env_file: &str,
) -> anyhow::Result<()> {
    let hostname = machina_agent::enrol::local_hostname(cli.hostname.as_deref());
    let detected = machina_agent::enrol::address_towards(controller);
    let addr = address
        .map(str::to_string)
        .or(detected)
        .unwrap_or_else(|| "127.0.0.1".into());
    let addr = addr.as_str();
    let is_loopback =
        |l: &str| l.starts_with("127.") || l.starts_with("localhost") || l.starts_with("[::1]");
    let port_of = |l: &str| {
        l.rsplit_once(':')
            .map(|(_, p)| p.to_string())
            .unwrap_or_default()
    };
    // Only on request, and only on this host's own address (never 0.0.0.0).
    let expose = opts.expose && addr != "127.0.0.1" && is_loopback(&cli.listen);
    let expose_console =
        opts.expose_console && addr != "127.0.0.1" && is_loopback(&cli.console_listen);
    let listen = if expose {
        format!("{addr}:{}", port_of(&cli.listen))
    } else {
        cli.listen.clone()
    };
    let console_listen = if expose_console {
        format!("{addr}:{}", port_of(&cli.console_listen))
    } else {
        cli.console_listen.clone()
    };
    let grpc_addr = machina_agent::enrol::advertised(&listen, addr);
    let console_addr = machina_agent::enrol::advertised(&console_listen, addr);
    if !expose && addr != "127.0.0.1" {
        tracing::warn!(
            "this agent listens on loopback ({grpc_addr}), so a controller on another machine cannot reach it; \
             re-run join with --expose to listen on {addr}"
        );
    }

    // Pinned controller: learn its CA by fingerprint, then talk to it with that CA only.
    let mut client = reqwest::Client::new();
    let mut csr = None;
    let mut node_key = None;
    if let Some(fp) = opts.ca_sha256 {
        if !controller.starts_with("https://") {
            anyhow::bail!("--ca-sha256 needs an https:// controller URL");
        }
        let ca = machina_agent::enrol::fetch_pinned_ca(controller, fp).await?;
        client = reqwest::Client::builder()
            .tls_built_in_root_certs(false)
            .add_root_certificate(reqwest::Certificate::from_pem(ca.as_bytes())?)
            .build()?;
        let key = machina_bpf::authca::new_host_key()?;
        csr = Some(machina_bpf::authca::host_csr(&key)?);
        node_key = Some(key);
        info!("controller CA verified against the pinned fingerprint");
    }

    let body = serde_json::json!({
        "token": token,
        "hostname": hostname,
        "address": addr,
        "agent_grpc_addr": grpc_addr,
        "agent_console_addr": console_addr,
        "libvirt_uri": cli.libvirt_uri,
        "csr_pem": csr,
    });
    let url = format!("{}/api/v1/hosts/join", controller.trim_end_matches('/'));
    if controller.starts_with("http://") {
        tracing::warn!(
            "Joining controller over plaintext HTTP — enrollment token will be transmitted \
             unencrypted. Use HTTPS in production."
        );
    }
    let resp = client.post(&url).json(&body).send().await?;
    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("join failed: {text}");
    }
    info!("joined controller at {controller}");
    let reply: serde_json::Value = resp.json().await.unwrap_or_default();
    if let Some(shared) = reply.get("agent_token").and_then(|v| v.as_str()) {
        if !machina_agent::enrol::channel_is_safe(controller) {
            tracing::warn!(
                "the controller offered the shared agent token but {controller} is not https or \
                 loopback; not storing it. Set MACHINA_AGENT_TOKEN in {env_file} by hand, or join over https."
            );
        } else {
            let mut vars: Vec<(String, String)> =
                vec![("MACHINA_AGENT_TOKEN".into(), shared.to_string())];
            if expose {
                vars.push(("MACHINA_AGENT_LISTEN".into(), listen.clone()));
            }
            if expose_console {
                vars.push((
                    "MACHINA_AGENT_CONSOLE_LISTEN".into(),
                    console_listen.clone(),
                ));
            }
            // Mutual TLS: the certificate the controller signed for our key.
            let mut tls = false;
            if let (Some(cert), Some(ca_back), Some(key), Some(fp)) = (
                reply.get("agent_cert_pem").and_then(|v| v.as_str()),
                reply.get("ca_pem").and_then(|v| v.as_str()),
                node_key.as_deref(),
                opts.ca_sha256,
            ) {
                if !machina_bpf::authca::fingerprint_matches(ca_back, fp) {
                    anyhow::bail!("the CA in the join reply is not the pinned one; refusing");
                }
                let (k, c, a) =
                    machina_agent::enrol::write_pki(Path::new(opts.pki_dir), key, cert, ca_back)?;
                vars.push(("MACHINA_AGENT_TLS_CERT".into(), c.display().to_string()));
                vars.push(("MACHINA_AGENT_TLS_KEY".into(), k.display().to_string()));
                vars.push((
                    "MACHINA_AGENT_TLS_CLIENT_CA".into(),
                    a.display().to_string(),
                ));
                tls = true;
                info!("node certificate stored in {}", opts.pki_dir);
            }
            let refs: Vec<(&str, &str)> =
                vars.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            match machina_agent::enrol::set_env_vars(Path::new(env_file), &refs) {
                Ok(changed) => {
                    if expose && !tls {
                        tracing::warn!(
                            "agent will listen on {listen}, protected by its own token but NOT encrypted: \
                             join with --ca-sha256 to get a certificate, and allow only the controller to reach port 50051."
                        );
                    }
                    if expose_console {
                        tracing::warn!(
                            "the console port {console_listen} is exposed in PLAINTEXT (a serial console is a root shell); \
                             allow only the controller to reach port 50052."
                        );
                    }
                    // The serving process below must use it too.
                    std::env::set_var("MACHINA_AGENT_TOKEN", shared);
                    info!(
                        "agent token {} in {env_file}",
                        if changed { "stored" } else { "already set" }
                    );
                    if changed
                        && env_file == machina_agent::enrol::DEFAULT_ENV_FILE
                        && restart_service("machina-agent")
                    {
                        info!("restarted machina-agent with the new settings; join is complete");
                        return Ok(());
                    }
                }
                Err(e) => tracing::warn!("could not store the agent settings in {env_file}: {e}"),
            }
        }
    }
    run_serve(cli).await
}

/// Restarts a systemd unit if one with this name is active; false when there is none.
fn restart_service(unit: &str) -> bool {
    // A unit that exists but is stopped (a freshly installed node) must be started too; only a host
    // without the unit (a hand-run agent) falls through to serving in this process.
    let installed = std::process::Command::new("systemctl")
        .args(["cat", unit])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    installed
        && std::process::Command::new("systemctl")
            .args(["restart", unit])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
}

/// Constant-time byte comparison for the shared agent token (avoids leaking
/// length/prefix match timing).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

async fn run_serve(cli: &Cli) -> anyhow::Result<()> {
    let hostname = cli.hostname.clone().unwrap_or_else(|| {
        std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("HOST"))
            .unwrap_or_else(|_| "localhost".into())
    });
    let state = shared_state(hostname);
    let libvirt_uri = cli.libvirt_uri.clone();
    let libvirt = Arc::new(Mutex::new(LibvirtCtx::open(&libvirt_uri)?));
    let service = AgentService::new(state, libvirt.clone());
    machina_agent::wake::spawn(libvirt.clone());
    machina_agent::imds::spawn();

    let grpc_addr: SocketAddr = cli.listen.parse()?;
    let console_addr: SocketAddr = cli.console_listen.parse()?;
    // Reuse the shared agent token to authenticate the console port (serial/VNC/SPICE
    // console WS). Empty = unauthenticated (dev), warned below.
    let console_secret = std::env::var("MACHINA_AGENT_TOKEN")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    if console_secret.is_empty() {
        tracing::warn!(
            "MACHINA_AGENT_TOKEN not set — agent console proxy ({console_addr}) accepts \
             UNAUTHENTICATED connections (serial console = interactive root shell). Set a \
             shared token on the controller and this agent to require auth."
        );
    }
    let console_state = ConsoleProxyState {
        libvirt,
        secret: console_secret,
    };

    // The console proxy (VNC/SPICE/serial — serial is an interactive root shell)
    // has no TLS support of its own: it always serves plaintext TCP, regardless
    // of whether MACHINA_AGENT_TLS_CERT/KEY are configured for the gRPC port.
    // Anyone positioned on the network path can read the console token (sent as
    // a `?token=` query param) and the console traffic itself. Terminate TLS in
    // front of this port (stunnel/nginx/an SSH tunnel) until the proxy gains
    // native TLS.
    tracing::warn!(
        "agent console proxy ({console_addr}) has no built-in TLS and always serves \
         plaintext, independent of MACHINA_AGENT_TLS_CERT/KEY — put it behind a TLS \
         terminator (stunnel/nginx/SSH tunnel) if it is reachable off-host"
    );

    info!("machina-agent gRPC on {grpc_addr}, console proxy on {console_addr}");
    tokio::try_join!(
        async {
            // Shared-secret auth for the gRPC surface. When MACHINA_AGENT_TOKEN is
            // set, every RPC must carry `authorization: Bearer <token>` (the
            // controller attaches it via agent_client). Unset = accept unauthenticated
            // (dev/backward-compat) with a loud warning.
            let expected_token = std::env::var("MACHINA_AGENT_TOKEN")
                .ok()
                .filter(|s| !s.is_empty());
            if expected_token.is_none() {
                tracing::warn!(
                    "MACHINA_AGENT_TOKEN not set — agent gRPC accepts UNAUTHENTICATED requests; \
                     set a shared token on the controller and this agent to require auth"
                );
            }
            // Refuse a controller that lost leadership: it sends an older epoch than one already seen here.
            let epoch_required = std::env::var("MACHINA_AGENT_REQUIRE_EPOCH").as_deref() == Ok("1");
            let auth = move |req: tonic::Request<()>| -> Result<tonic::Request<()>, tonic::Status> {
                machina_agent::epoch::check(&req, epoch_required)?;
                match &expected_token {
                    None => Ok(req),
                    Some(exp) => {
                        let provided = req
                            .metadata()
                            .get("authorization")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|s| s.strip_prefix("Bearer "));
                        match provided {
                            Some(t) if ct_eq(t.as_bytes(), exp.as_bytes()) => Ok(req),
                            _ => Err(tonic::Status::unauthenticated(
                                "invalid or missing agent token",
                            )),
                        }
                    }
                }
            };
            let mut builder = Server::builder();
            match (
                std::env::var("MACHINA_AGENT_TLS_CERT").ok(),
                std::env::var("MACHINA_AGENT_TLS_KEY").ok(),
            ) {
                (None, None) => {
                    tracing::warn!(
                        "MACHINA_AGENT_TLS_CERT/MACHINA_AGENT_TLS_KEY not set — \
                         gRPC serving UNENCRYPTED plaintext; set env vars in production"
                    );
                }
                (Some(cert_path), Some(key_path)) => {
                    // Both vars are set — the operator explicitly asked for TLS. Refuse
                    // to start rather than silently falling back to plaintext if the
                    // paths turn out not to exist (e.g. a typo, or certs not yet
                    // provisioned): the previous behavior of downgrading unnoticed to
                    // an unencrypted root-capable gRPC surface is a fail-open bug.
                    if !Path::new(&cert_path).exists() || !Path::new(&key_path).exists() {
                        anyhow::bail!(
                            "MACHINA_AGENT_TLS_CERT/MACHINA_AGENT_TLS_KEY are set but do not \
                             both point to existing files (cert='{cert_path}', key='{key_path}') \
                             — refusing to start rather than silently falling back to plaintext"
                        );
                    }
                    let cert = tokio::fs::read_to_string(&cert_path).await?;
                    let key = tokio::fs::read_to_string(&key_path).await?;
                    let mut tls = ServerTlsConfig::new().identity(Identity::from_pem(cert, key));
                    // With a client CA the controller must present a certificate from it
                    // (mutual TLS); set by `join --ca-sha256`.
                    if let Some(ca_path) = std::env::var("MACHINA_AGENT_TLS_CLIENT_CA")
                        .ok()
                        .filter(|p| !p.is_empty())
                    {
                        if !Path::new(&ca_path).exists() {
                            anyhow::bail!(
                                "MACHINA_AGENT_TLS_CLIENT_CA={ca_path} does not exist — refusing to start without client authentication"
                            );
                        }
                        let ca = tokio::fs::read_to_string(&ca_path).await?;
                        tls = tls.client_ca_root(Certificate::from_pem(ca));
                        info!("agent gRPC requires client certificates (mutual TLS)");
                    }
                    builder = builder.tls_config(tls)?;
                    info!("agent gRPC TLS enabled");
                }
                (cert, key) => {
                    // Only one of the two is set — almost certainly a misconfiguration,
                    // not an intentional plaintext choice. Fail closed instead of
                    // guessing.
                    anyhow::bail!(
                        "only one of MACHINA_AGENT_TLS_CERT ({}) / MACHINA_AGENT_TLS_KEY ({}) \
                         is set — both are required to enable TLS; refusing to start in an \
                         ambiguous state",
                        cert.is_some(),
                        key.is_some()
                    );
                }
            }
            // DNS threat feeds reach bpfd as one bpf_call of up to ~64 MiB.
            let svc = HostAgentServer::new(service).max_decoding_message_size(80 << 20);
            builder
                .add_service(tonic::service::interceptor::InterceptedService::new(
                    svc, auth,
                ))
                .serve(grpc_addr)
                .await
                .map_err(anyhow::Error::from)
        },
        console_ws::serve(console_addr, console_state),
    )?;
    Ok(())
}
