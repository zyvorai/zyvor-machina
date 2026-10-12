// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Host cluster bootstrap (k3s → metrics-server → KubeVirt/CDI/virtctl).
//! By default k3s keeps its own networking (flannel, NetworkPolicy controller,
//! kube-proxy). The native eBPF CNI (machina-bpfd + `machina-cni agent`) is
//! opt-in: with `cni: "machina"` k3s is installed without those three and
//! machina-cni provides them instead.
//! Invoked from `POST /api/v1/k8s/cluster-bootstrap`.
//!
//! Linux-only; intended for `machina-daemon` running as **root** (stock systemd unit).

use machina_core::LibvirtError;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::{sleep, timeout};
use tracing::{info, warn};

/// Same payload shape as `routes::k8s::KubectlResult` for HTTP responses.
#[derive(Debug, serde::Serialize)]
pub struct BootstrapCmdResult {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub ok: bool,
}

/// Pod networking for the bootstrapped cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CniChoice {
    /// k3s defaults: flannel, NetworkPolicy controller, kube-proxy.
    #[default]
    Default,
    /// Native eBPF machina-cni replaces all three.
    Machina,
}

impl CniChoice {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "" | "default" => Some(Self::Default),
            "machina" => Some(Self::Machina),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Machina => "machina",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClusterBootstrapParams {
    pub phase: String,
    pub server_ip: Option<String>,
    pub skip_kubevirt_cdi: bool,
    pub install_metrics_server: bool,
    pub cni: CniChoice,
}

const K3S_INSTALL_URL_DEFAULT: &str = "https://get.k3s.io";
const KUBECONFIG_ADMIN: &str = "/etc/rancher/k3s/k3s.yaml";
const METRICS_SERVER_MANIFEST_DEFAULT: &str =
    "https://github.com/kubernetes-sigs/metrics-server/releases/download/v0.7.2/components.yaml";
const KUBEVIRT_STABLE_TXT_DEFAULT: &str =
    "https://storage.googleapis.com/kubevirt-prow/release/kubevirt/kubevirt/stable.txt";
const CDI_RELEASES_LATEST_API_DEFAULT: &str =
    "https://api.github.com/repos/kubevirt/containerized-data-importer/releases/latest";

const KUBEVIRT_RELEASES_DEFAULT: &str = "https://github.com/kubevirt/kubevirt/releases/download";
const CDI_RELEASES_DEFAULT: &str =
    "https://github.com/kubevirt/containerized-data-importer/releases/download";

/// Air-gapped sites point the bootstrap at an internal mirror: each source URL can be
/// overridden by an environment variable, and the default stays the upstream location.
fn mirror(env: &str, default: &str) -> String {
    std::env::var(env)
        .ok()
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn k3s_install_url() -> String {
    mirror("MACHINA_K3S_INSTALL_URL", K3S_INSTALL_URL_DEFAULT)
}
fn metrics_server_manifest() -> String {
    mirror(
        "MACHINA_METRICS_SERVER_MANIFEST_URL",
        METRICS_SERVER_MANIFEST_DEFAULT,
    )
}
fn kubevirt_stable_txt() -> String {
    mirror("MACHINA_KUBEVIRT_STABLE_URL", KUBEVIRT_STABLE_TXT_DEFAULT)
}
fn cdi_releases_latest_api() -> String {
    mirror(
        "MACHINA_CDI_LATEST_API_URL",
        CDI_RELEASES_LATEST_API_DEFAULT,
    )
}
fn kubevirt_releases() -> String {
    mirror("MACHINA_KUBEVIRT_RELEASES_URL", KUBEVIRT_RELEASES_DEFAULT)
}
fn cdi_releases() -> String {
    mirror("MACHINA_CDI_RELEASES_URL", CDI_RELEASES_DEFAULT)
}

const TIMEOUT_K3S_INSTALL_SECS: u64 = 900;
const TIMEOUT_KUBECTL_SECS: u64 = 600;
/// `kubectl wait … --timeout=10m`
const TIMEOUT_KUBECTL_WAIT_SECS: u64 = 660;
const TIMEOUT_CNI_READY_SECS: u64 = 660;
const TIMEOUT_METRICS_ROLLOUT_SECS: u64 = 180;

/// Machina-owned home for the admin kubeconfig this bootstrap writes.
///
/// Not `$HOME/.kube/config`: `machina-daemon.service` runs with
/// `ProtectHome=read-only` (systemd hardening), which makes `/root` (this
/// process's `$HOME`) read-only in the daemon's own mount namespace regardless
/// of on-disk permissions — every k3s-phase run failed at "copy k3s admin
/// kubeconfig: Read-only file system" as a result. This path sits outside
/// `/home`/`/root`, so it's unaffected by that hardening.
fn kubeconfig_dir() -> PathBuf {
    PathBuf::from("/var/lib/machina/kube")
}

fn kubectl_bin() -> PathBuf {
    let p = Path::new("/usr/local/bin/kubectl");
    if p.is_file() {
        p.to_path_buf()
    } else {
        PathBuf::from("kubectl")
    }
}

fn append_section(log: &mut String, title: &str, body: &str) {
    log.push_str("\n=== ");
    log.push_str(title);
    log.push_str(" ===\n");
    log.push_str(body);
    if !body.ends_with('\n') {
        log.push('\n');
    }
}

async fn run_sh(
    label: &str,
    script: &str,
    env: &[(String, String)],
    timeout_secs: u64,
    stdout_log: &mut String,
    stderr_log: &mut String,
) -> Result<(), LibvirtError> {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(script);
    cmd.stdin(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let cmdline = format!("/bin/sh -c {script:?} (+ {} env vars)", env.len());
    info!(target: "machina_bootstrap", phase = %label, cmdline = %cmdline, "run_sh");

    let child = timeout(
        Duration::from_secs(timeout_secs),
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).output(),
    )
    .await
    .map_err(|_| LibvirtError::Operation(format!("{label}: timed out after {timeout_secs}s")))?
    .map_err(|e| LibvirtError::Operation(format!("{label}: spawn failed: {e}")))?;

    let out = String::from_utf8_lossy(&child.stdout).to_string();
    let err = String::from_utf8_lossy(&child.stderr).to_string();
    append_section(stdout_log, label, &out);
    if !err.is_empty() {
        append_section(stderr_log, label, &err);
    }

    if !child.status.success() {
        return Err(LibvirtError::Operation(format!(
            "{label} failed (exit {:?}): {err}",
            child.status.code()
        )));
    }
    Ok(())
}

async fn run_cmd_argv(
    label: &str,
    program: &Path,
    args: &[String],
    env: &[(String, String)],
    timeout_secs: u64,
    stdout_log: &mut String,
    stderr_log: &mut String,
) -> Result<(), LibvirtError> {
    let mut cmd = Command::new(program);
    for a in args {
        cmd.arg(a);
    }
    cmd.stdin(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let cmdline = format!("{} {}", program.display(), args.join(" "));
    info!(target: "machina_bootstrap", phase = %label, cmd = %cmdline, "run_cmd_argv");

    let child = timeout(
        Duration::from_secs(timeout_secs),
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).output(),
    )
    .await
    .map_err(|_| LibvirtError::Operation(format!("{label}: timed out after {timeout_secs}s")))?
    .map_err(|e| LibvirtError::Operation(format!("{label}: spawn failed: {e}")))?;

    let out = String::from_utf8_lossy(&child.stdout).to_string();
    let err = String::from_utf8_lossy(&child.stderr).to_string();
    append_section(stdout_log, label, &out);
    if !err.is_empty() {
        append_section(stderr_log, label, &err);
    }

    if !child.status.success() {
        return Err(LibvirtError::Operation(format!(
            "{label} failed (exit {:?}): {}",
            child.status.code(),
            err.trim()
        )));
    }
    Ok(())
}

/// First IPv4/IPv6 token from `hostname -I` (Linux).
async fn detect_server_ip(stdout_log: &mut String) -> Result<String, LibvirtError> {
    let mut cmd = Command::new("hostname");
    cmd.arg("-I");
    cmd.stdin(Stdio::null());
    let out = timeout(Duration::from_secs(10), cmd.output())
        .await
        .map_err(|_| LibvirtError::Operation("hostname -I timed out".into()))?
        .map_err(|e| LibvirtError::Operation(format!("hostname: {e}")))?;
    let line = String::from_utf8_lossy(&out.stdout);
    let ip = line.split_whitespace().next().unwrap_or("127.0.0.1").trim();
    if ip.is_empty() {
        return Err(LibvirtError::Operation(
            "could not determine server IP (hostname -I empty)".into(),
        ));
    }
    append_section(
        stdout_log,
        "detect_server_ip",
        &format!("using first address from hostname -I: {ip}"),
    );
    Ok(ip.to_string())
}

fn kubeconfig_path() -> PathBuf {
    kubeconfig_dir().join("config")
}

fn kubeconfig_env_pairs() -> Vec<(String, String)> {
    vec![(
        "KUBECONFIG".into(),
        kubeconfig_path().to_string_lossy().into_owned(),
    )]
}

async fn wait_until_kubectl_nodes(
    kubectl: &Path,
    env: &[(String, String)],
    stdout_log: &mut String,
) -> Result<(), LibvirtError> {
    for attempt in 1..=80 {
        let mut cmd = Command::new(kubectl);
        cmd.args(["get", "nodes"]);
        cmd.stdin(Stdio::null());
        for (k, v) in env {
            cmd.env(k, v);
        }
        match timeout(Duration::from_secs(15), cmd.output()).await {
            Ok(Ok(out)) if out.status.success() => {
                append_section(
                    stdout_log,
                    "kubectl_get_nodes",
                    &String::from_utf8_lossy(&out.stdout),
                );
                return Ok(());
            }
            _ => {}
        }
        sleep(Duration::from_secs(3)).await;
        if attempt % 10 == 0 {
            warn!(target: "machina_bootstrap", attempt, "still waiting for kubectl get nodes");
        }
    }
    Err(LibvirtError::Operation(
        "timed out waiting for Kubernetes API (kubectl get nodes)".into(),
    ))
}

fn k3s_server_flags(cni: CniChoice) -> &'static str {
    match cni {
        CniChoice::Default => "--disable=traefik --disable=servicelb",
        CniChoice::Machina => {
            "--flannel-backend=none --disable-network-policy --disable-kube-proxy --disable=traefik --disable=servicelb"
        }
    }
}

async fn phase_k3s(
    server_ip: &str,
    cni: CniChoice,
    stdout_log: &mut String,
    stderr_log: &mut String,
) -> Result<(), LibvirtError> {
    let url = k3s_install_url();
    if url.contains(['\'', '"', ';', '|', '&', '$', '`', '\n']) {
        return Err(LibvirtError::Operation(
            "MACHINA_K3S_INSTALL_URL contains shell metacharacters".into(),
        ));
    }
    let install = format!(
        "curl -sfL '{url}' | sh -s - server {}",
        k3s_server_flags(cni)
    );
    let no_env: Vec<(String, String)> = vec![];
    run_sh(
        "k3s_install",
        &install,
        &no_env,
        TIMEOUT_K3S_INSTALL_SECS,
        stdout_log,
        stderr_log,
    )
    .await?;

    let kube_dir = kubeconfig_dir();
    std::fs::create_dir_all(&kube_dir)
        .map_err(|e| LibvirtError::Operation(format!("mkdir {}: {e}", kube_dir.display())))?;

    let cfg = kubeconfig_path();
    std::fs::copy(KUBECONFIG_ADMIN, &cfg)
        .map_err(|e| LibvirtError::Operation(format!("copy k3s admin kubeconfig: {e}")))?;

    let mut yaml = std::fs::read_to_string(&cfg)
        .map_err(|e| LibvirtError::Operation(format!("read kubeconfig: {e}")))?;
    yaml = yaml.replace("127.0.0.1", server_ip);
    std::fs::write(&cfg, yaml)
        .map_err(|e| LibvirtError::Operation(format!("write kubeconfig: {e}")))?;

    append_section(
        stdout_log,
        "kubeconfig",
        &format!(
            "wrote {} (replaced 127.0.0.1 with {server_ip})",
            cfg.display()
        ),
    );

    let k3s_bin = Path::new("/usr/local/bin/k3s");
    let kubectl_link = Path::new("/usr/local/bin/kubectl");
    #[cfg(unix)]
    if !kubectl_link.exists() && k3s_bin.is_file() {
        let _ = std::fs::remove_file(kubectl_link);
        std::os::unix::fs::symlink(k3s_bin, kubectl_link)
            .map_err(|e| LibvirtError::Operation(format!("symlink kubectl -> k3s: {e}")))?;
        append_section(
            stdout_log,
            "kubectl_symlink",
            "/usr/local/bin/kubectl -> k3s",
        );
    }

    Ok(())
}

async fn download_text(client: &reqwest::Client, url: &str) -> Result<String, LibvirtError> {
    let text = client
        .get(url)
        .header(
            "User-Agent",
            "machina-daemon-cluster-bootstrap/1.0 (compatible; +https://github.com/zyvorai/machina)",
        )
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| LibvirtError::Operation(format!("GET {url}: {e}")))?
        .error_for_status()
        .map_err(|e| LibvirtError::Operation(format!("GET {url}: {e}")))?
        .text()
        .await
        .map_err(|e| LibvirtError::Operation(format!("read body {url}: {e}")))?;
    Ok(text.trim().to_string())
}

async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
) -> Result<(), LibvirtError> {
    let bytes = client
        .get(url)
        .header("User-Agent", "machina-daemon-cluster-bootstrap/1.0")
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .map_err(|e| LibvirtError::Operation(format!("GET {url}: {e}")))?
        .error_for_status()
        .map_err(|e| LibvirtError::Operation(format!("GET {url}: {e}")))?
        .bytes()
        .await
        .map_err(|e| LibvirtError::Operation(format!("read bytes {url}: {e}")))?;
    std::fs::write(dest, bytes)
        .map_err(|e| LibvirtError::Operation(format!("write {}: {e}", dest.display())))?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct GithubReleaseTag {
    tag_name: String,
}

fn host_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => "amd64",
    }
}

const MACHINA_CNI_BIN: &str = "/usr/local/bin/machina-cni";
const MACHINA_CNI_ENV_FILE: &str = "/etc/default/machina-cni";
/// Same defaults as `machina-cni agent` (`MACHINA_CNI_CONF_DIRS`).
const CNI_CONF_DIRS: &[&str] = &["/etc/cni/net.d", "/var/lib/rancher/k3s/agent/etc/cni/net.d"];
const MACHINA_CONFLIST: &str = "05-machina.conflist";

/// CNI configs in `dirs` that machina-cni did not write.
fn foreign_cni_configs(dirs: &[&Path]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let is_cfg =
                name.ends_with(".conf") || name.ends_with(".conflist") || name.ends_with(".json");
            if is_cfg && name != MACHINA_CONFLIST && e.path().is_file() {
                found.push(e.path());
            }
        }
    }
    found.sort();
    found
}

fn takeover_enabled(env_file: &str) -> bool {
    env_file.lines().any(|l| {
        let l = l.trim();
        l.strip_prefix("MACHINA_CNI_TAKEOVER=")
            .map(|v| {
                matches!(
                    v.trim_matches(['"', '\'']).to_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
    })
}

/// Native eBPF CNI (opt-in): machina-bpfd owns the datapath, `machina-cni agent`
/// writes the CNI config, node routes, masquerade and compiles
/// NetworkPolicies / Services. Refuses to replace a CNI that is already
/// configured unless `MACHINA_CNI_TAKEOVER=1` is set in `/etc/default/machina-cni`.
async fn phase_cni(stdout_log: &mut String, stderr_log: &mut String) -> Result<(), LibvirtError> {
    if !Path::new(MACHINA_CNI_BIN).is_file() {
        return Err(LibvirtError::Operation(format!(
            "{MACHINA_CNI_BIN} not installed — rerun install.sh (it builds machina-cni with the workspace)"
        )));
    }
    let dirs: Vec<&Path> = CNI_CONF_DIRS.iter().map(Path::new).collect();
    let foreign = foreign_cni_configs(&dirs);
    if !foreign.is_empty() {
        let takeover =
            std::fs::read_to_string(MACHINA_CNI_ENV_FILE).is_ok_and(|s| takeover_enabled(&s));
        let list = foreign
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        if !takeover {
            return Err(LibvirtError::Operation(format!(
                "another CNI is already configured ({list}); machina-cni will not replace it. \
                 Reinstall k3s with cni=\"machina\" (no flannel/kube-proxy), or set \
                 MACHINA_CNI_TAKEOVER=1 in {MACHINA_CNI_ENV_FILE} to take over deliberately"
            )));
        }
        append_section(
            stdout_log,
            "cni_takeover",
            &format!("MACHINA_CNI_TAKEOVER=1: replacing {list}"),
        );
    }
    let no_env: Vec<(String, String)> = vec![];
    run_sh(
        "machina_bpfd_active",
        "systemctl enable --now machina-bpfd && systemctl is-active machina-bpfd",
        &no_env,
        60,
        stdout_log,
        stderr_log,
    )
    .await?;
    run_sh(
        "machina_cni_enable",
        "systemctl daemon-reload && systemctl enable machina-cni && systemctl restart machina-cni",
        &no_env,
        60,
        stdout_log,
        stderr_log,
    )
    .await?;
    phase_wait_nodes(stdout_log, stderr_log).await
}

/// Wait until every node is Ready (the CNI, default or machina, is up).
async fn phase_wait_nodes(
    stdout_log: &mut String,
    stderr_log: &mut String,
) -> Result<(), LibvirtError> {
    let kube_env = kubeconfig_env_pairs();
    let kubectl = kubectl_bin();
    wait_until_kubectl_nodes(&kubectl, &kube_env, stdout_log).await?;
    run_cmd_argv(
        "nodes_ready",
        &kubectl,
        &[
            "wait".into(),
            "--for=condition=Ready".into(),
            "nodes".into(),
            "--all".into(),
            "--timeout=10m".into(),
        ],
        &kube_env,
        TIMEOUT_CNI_READY_SECS,
        stdout_log,
        stderr_log,
    )
    .await?;
    run_cmd_argv(
        "kubectl_get_nodes_wide",
        &kubectl,
        &["get".into(), "nodes".into(), "-o".into(), "wide".into()],
        &kube_env,
        TIMEOUT_KUBECTL_SECS,
        stdout_log,
        stderr_log,
    )
    .await?;
    run_cmd_argv(
        "kubectl_get_pods_A",
        &kubectl,
        &[
            "get".into(),
            "pods".into(),
            "-A".into(),
            "-o".into(),
            "wide".into(),
        ],
        &kube_env,
        TIMEOUT_KUBECTL_SECS,
        stdout_log,
        stderr_log,
    )
    .await?;
    Ok(())
}

async fn phase_metrics(
    install: bool,
    stdout_log: &mut String,
    stderr_log: &mut String,
) -> Result<(), LibvirtError> {
    if !install {
        append_section(
            stdout_log,
            "metrics_server",
            "skipped (install_metrics_server=false)",
        );
        return Ok(());
    }

    let kube_env = kubeconfig_env_pairs();
    let kubectl = kubectl_bin();

    let mut check = Command::new(&kubectl);
    check.args(["get", "deployment", "metrics-server", "-n", "kube-system"]);
    check.stdin(Stdio::null());
    for (k, v) in &kube_env {
        check.env(k, v);
    }
    let exists = check
        .output()
        .await
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if exists {
        append_section(stdout_log, "metrics_server", "already present");
        return Ok(());
    }

    run_cmd_argv(
        "kubectl_apply_metrics_server",
        &kubectl,
        &["apply".into(), "-f".into(), metrics_server_manifest()],
        &kube_env,
        TIMEOUT_KUBECTL_SECS,
        stdout_log,
        stderr_log,
    )
    .await?;

    let patch = r#"[{"op":"add","path":"/spec/template/spec/containers/0/args/-","value":"--kubelet-insecure-tls"}]"#;
    let mut p = Command::new(&kubectl);
    p.args([
        "patch",
        "deployment",
        "metrics-server",
        "-n",
        "kube-system",
        "--type=json",
        "-p",
        patch,
    ]);
    p.stdin(Stdio::null());
    for (k, v) in &kube_env {
        p.env(k, v);
    }
    let _ = p.output().await;

    let mut roll = Command::new(&kubectl);
    roll.args([
        "rollout",
        "status",
        "deployment/metrics-server",
        "-n",
        "kube-system",
        &format!("--timeout={TIMEOUT_METRICS_ROLLOUT_SECS}s"),
    ]);
    roll.stdin(Stdio::null());
    for (k, v) in &kube_env {
        roll.env(k, v);
    }
    match roll.output().await {
        // `Command::output()` returns `Ok` even when the process exits non-zero — it
        // only errors if spawning failed. A timed-out/failed `rollout status` was
        // previously logged into stdout_log unconditionally, which reads as success
        // in the bootstrap transcript even though the deployment never became ready.
        Ok(o) if o.status.success() => append_section(
            stdout_log,
            "metrics_server_rollout",
            &String::from_utf8_lossy(&o.stdout),
        ),
        Ok(o) => append_section(
            stderr_log,
            "metrics_server_rollout",
            &format!(
                "rollout status failed (exit {:?}): {}{}",
                o.status.code(),
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr),
            ),
        ),
        Err(_) => append_section(
            stderr_log,
            "metrics_server_rollout",
            "rollout status failed or timed out — check kube-system",
        ),
    }

    Ok(())
}

async fn phase_kubevirt_cdi(
    stdout_log: &mut String,
    stderr_log: &mut String,
) -> Result<(), LibvirtError> {
    let kube_env = kubeconfig_env_pairs();
    let kubectl = kubectl_bin();

    let mut vhv = Command::new("virt-host-validate");
    vhv.arg("qemu");
    vhv.stdin(Stdio::null());
    if let Ok(o) = vhv.output().await {
        append_section(
            stdout_log,
            "virt_host_validate",
            &String::from_utf8_lossy(&o.stdout),
        );
        if !o.stderr.is_empty() {
            append_section(
                stderr_log,
                "virt_host_validate",
                &String::from_utf8_lossy(&o.stderr),
            );
        }
    } else {
        append_section(
            stdout_log,
            "virt_host_validate",
            "virt-host-validate not available — skipping host check",
        );
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| LibvirtError::Internal(format!("reqwest: {e}")))?;

    let kv_ver = download_text(&client, &kubevirt_stable_txt()).await?;
    if kv_ver.is_empty() {
        return Err(LibvirtError::Operation(
            "empty KubeVirt stable.txt response".into(),
        ));
    }

    let kv_base = kubevirt_releases();
    let op_url = format!("{kv_base}/{kv_ver}/kubevirt-operator.yaml");
    let cr_url = format!("{kv_base}/{kv_ver}/kubevirt-cr.yaml");

    run_cmd_argv(
        "kubectl_kubevirt_operator",
        &kubectl,
        &["apply".into(), "-f".into(), op_url],
        &kube_env,
        TIMEOUT_KUBECTL_SECS,
        stdout_log,
        stderr_log,
    )
    .await?;
    run_cmd_argv(
        "kubectl_kubevirt_cr",
        &kubectl,
        &["apply".into(), "-f".into(), cr_url],
        &kube_env,
        TIMEOUT_KUBECTL_SECS,
        stdout_log,
        stderr_log,
    )
    .await?;

    run_cmd_argv(
        "kubectl_wait_kubevirt",
        &kubectl,
        &[
            "-n".into(),
            "kubevirt".into(),
            "wait".into(),
            "kv/kubevirt".into(),
            "--for=condition=Available".into(),
            "--timeout=10m".into(),
        ],
        &kube_env,
        TIMEOUT_KUBECTL_WAIT_SECS,
        stdout_log,
        stderr_log,
    )
    .await?;

    let body = client
        .get(cdi_releases_latest_api())
        .header("User-Agent", "machina-daemon-cluster-bootstrap/1.0")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| LibvirtError::Operation(format!("GitHub CDI API: {e}")))?
        .error_for_status()
        .map_err(|e| LibvirtError::Operation(format!("GitHub CDI API: {e}")))?
        .json::<GithubReleaseTag>()
        .await
        .map_err(|e| LibvirtError::Operation(format!("parse GitHub CDI JSON: {e}")))?;

    let cdi_ver = body.tag_name.trim();
    if cdi_ver.is_empty() {
        return Err(LibvirtError::Operation("empty CDI tag_name".into()));
    }

    let cdi_base = cdi_releases();
    let cdi_op = format!("{cdi_base}/{cdi_ver}/cdi-operator.yaml");
    let cdi_cr = format!("{cdi_base}/{cdi_ver}/cdi-cr.yaml");

    run_cmd_argv(
        "kubectl_cdi_operator",
        &kubectl,
        &["apply".into(), "-f".into(), cdi_op],
        &kube_env,
        TIMEOUT_KUBECTL_SECS,
        stdout_log,
        stderr_log,
    )
    .await?;
    run_cmd_argv(
        "kubectl_cdi_cr",
        &kubectl,
        &["apply".into(), "-f".into(), cdi_cr],
        &kube_env,
        TIMEOUT_KUBECTL_SECS,
        stdout_log,
        stderr_log,
    )
    .await?;

    let wait_ns = run_cmd_argv(
        "kubectl_wait_cdi_ns",
        &kubectl,
        &[
            "-n".into(),
            "cdi".into(),
            "wait".into(),
            "cdi".into(),
            "cdi".into(),
            "--for=condition=Available".into(),
            "--timeout=10m".into(),
        ],
        &kube_env,
        TIMEOUT_KUBECTL_WAIT_SECS,
        stdout_log,
        stderr_log,
    )
    .await;

    if wait_ns.is_err() {
        run_cmd_argv(
            "kubectl_wait_cdi_cluster",
            &kubectl,
            &[
                "wait".into(),
                "cdi/cdi".into(),
                "--for=condition=Available".into(),
                "--timeout=10m".into(),
            ],
            &kube_env,
            TIMEOUT_KUBECTL_WAIT_SECS,
            stdout_log,
            stderr_log,
        )
        .await?;
    }

    let arch = host_arch();
    let virt_url = format!(
        "{}/{kv_ver}/virtctl-{kv_ver}-linux-{arch}",
        kubevirt_releases()
    );
    let tmp_virt = std::env::temp_dir().join(format!("virtctl-{}", uuid::Uuid::new_v4()));
    download_file(&client, &virt_url, &tmp_virt).await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&tmp_virt)
            .map_err(|e| LibvirtError::Operation(format!("virtctl stat: {e}")))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&tmp_virt, perms)
            .map_err(|e| LibvirtError::Operation(format!("chmod virtctl: {e}")))?;
    }

    let dest = Path::new("/usr/local/bin/virtctl");
    let _ = std::fs::remove_file(dest);
    std::fs::rename(&tmp_virt, dest)
        .map_err(|e| LibvirtError::Operation(format!("install virtctl: {e}")))?;

    run_cmd_argv(
        "kubectl_get_kubevirt_ns",
        &kubectl,
        &["get".into(), "pods".into(), "-n".into(), "kubevirt".into()],
        &kube_env,
        TIMEOUT_KUBECTL_SECS,
        stdout_log,
        stderr_log,
    )
    .await?;
    run_cmd_argv(
        "kubectl_get_cdi_ns",
        &kubectl,
        &["get".into(), "pods".into(), "-n".into(), "cdi".into()],
        &kube_env,
        TIMEOUT_KUBECTL_SECS,
        stdout_log,
        stderr_log,
    )
    .await?;

    run_cmd_argv(
        "virtctl_version",
        Path::new("virtctl"),
        &["version".into()],
        &kube_env,
        60,
        stdout_log,
        stderr_log,
    )
    .await?;

    Ok(())
}

fn print_footer(skip_kv: bool, cni: CniChoice, stdout_log: &mut String) {
    let mut msg = String::from(match cni {
        CniChoice::Default => "\n[SUCCESS] k3s (default CNI: flannel + kube-proxy)",
        CniChoice::Machina => "\n[SUCCESS] k3s + machina-cni (native eBPF)",
    });
    if !skip_kv {
        msg.push_str(" + KubeVirt + CDI");
    }
    if cni == CniChoice::Machina {
        msg.push_str("\n\nCNI agent:\n  journalctl -u machina-cni -f\n  curl -sk https://127.0.0.1:5092/api/v1/bpf/status   # datapath\n");
    }
    if !skip_kv {
        msg.push_str("\nKubeVirt / CDI:\n  kubectl get kubevirt -n kubevirt\n  kubectl get cdi\n  kubectl get storageclass\n  virtctl version\n\nConfigure [kubevirt] in machina config.toml for YAML bundles.\n");
    }
    stdout_log.push_str(&msg);
}

pub async fn run_cluster_bootstrap(
    params: ClusterBootstrapParams,
) -> Result<BootstrapCmdResult, LibvirtError> {
    let phase = params.phase.trim().to_string();
    let summary = format!(
        "cluster_bootstrap Rust phase={phase} server_ip={:?} skip_kubevirt_cdi={} install_metrics_server={} cni={}",
        params.server_ip.as_deref(),
        params.skip_kubevirt_cdi,
        params.install_metrics_server,
        params.cni.as_str()
    );
    info!(target: "machina_bootstrap", summary = %summary, "start");

    let mut stdout_log = String::new();
    let mut stderr_log = String::new();

    let server_ip = if let Some(ref ip) = params.server_ip {
        ip.trim().to_string()
    } else {
        detect_server_ip(&mut stdout_log).await?
    };

    let result = async {
        match phase.as_str() {
            "full" => {
                phase_k3s(&server_ip, params.cni, &mut stdout_log, &mut stderr_log).await?;
                match params.cni {
                    CniChoice::Default => {
                        phase_wait_nodes(&mut stdout_log, &mut stderr_log).await?
                    }
                    CniChoice::Machina => phase_cni(&mut stdout_log, &mut stderr_log).await?,
                }
                phase_metrics(
                    params.install_metrics_server,
                    &mut stdout_log,
                    &mut stderr_log,
                )
                .await?;
                if params.skip_kubevirt_cdi {
                    append_section(
                        &mut stdout_log,
                        "kubevirt_cdi",
                        "skipped (skip_kubevirt_cdi=true)",
                    );
                } else {
                    phase_kubevirt_cdi(&mut stdout_log, &mut stderr_log).await?;
                }
                print_footer(params.skip_kubevirt_cdi, params.cni, &mut stdout_log);
                Ok(())
            }
            "k3s" => phase_k3s(&server_ip, params.cni, &mut stdout_log, &mut stderr_log).await,
            // "cilium" is the pre-native phase name, kept so old clients still work.
            "cni" | "cilium" => phase_cni(&mut stdout_log, &mut stderr_log).await,
            "metrics" => {
                phase_metrics(
                    params.install_metrics_server,
                    &mut stdout_log,
                    &mut stderr_log,
                )
                .await
            }
            "kubevirt_cdi" => phase_kubevirt_cdi(&mut stdout_log, &mut stderr_log).await,
            _ => Err(LibvirtError::Invalid(format!(
                "unknown phase {phase} (expected full|k3s|cni|metrics|kubevirt_cdi)"
            ))),
        }
    }
    .await;

    match result {
        Ok(()) => Ok(BootstrapCmdResult {
            command: summary,
            stdout: stdout_log,
            stderr: stderr_log,
            exit_code: 0,
            ok: true,
        }),
        Err(e) => {
            let msg = format!("{e}");
            Err(LibvirtError::Operation(format!(
                "{msg}\n\n--- stdout ---\n{stdout_log}\n--- stderr ---\n{stderr_log}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cni_choice_parse() {
        assert_eq!(CniChoice::parse(""), Some(CniChoice::Default));
        assert_eq!(CniChoice::parse("default"), Some(CniChoice::Default));
        assert_eq!(CniChoice::parse("machina"), Some(CniChoice::Machina));
        assert_eq!(CniChoice::parse("cilium"), None);
    }

    #[test]
    fn default_k3s_keeps_bundled_networking() {
        let flags = k3s_server_flags(CniChoice::Default);
        assert!(!flags.contains("flannel-backend"));
        assert!(!flags.contains("disable-kube-proxy"));
        assert!(!flags.contains("disable-network-policy"));
        let flags = k3s_server_flags(CniChoice::Machina);
        assert!(flags.contains("--flannel-backend=none"));
        assert!(flags.contains("--disable-kube-proxy"));
    }

    #[test]
    fn foreign_configs_ignore_our_own() {
        let dir = std::env::temp_dir().join(format!("machina-cni-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MACHINA_CONFLIST), "{}").unwrap();
        assert!(foreign_cni_configs(&[dir.as_path()]).is_empty());
        std::fs::write(dir.join("10-flannel.conflist"), "{}").unwrap();
        std::fs::write(dir.join("README"), "").unwrap();
        let found = foreign_cni_configs(&[dir.as_path(), Path::new("/nonexistent-cni-dir")]);
        assert_eq!(found, vec![dir.join("10-flannel.conflist")]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn takeover_parsing() {
        assert!(takeover_enabled("MACHINA_CNI_TAKEOVER=1\n"));
        assert!(takeover_enabled("# x\nMACHINA_CNI_TAKEOVER=\"true\""));
        assert!(!takeover_enabled("# MACHINA_CNI_TAKEOVER=1"));
        assert!(!takeover_enabled("MACHINA_CNI_TAKEOVER=0"));
        assert!(!takeover_enabled(""));
    }
}
