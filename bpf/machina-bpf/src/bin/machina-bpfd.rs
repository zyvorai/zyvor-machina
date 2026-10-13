// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! machina-bpfd — owns the host eBPF datapath (enforcement, flows, process
//! telemetry, health, capture, QoS, CNI maps) and serves it on a Unix socket.

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "machina-bpfd", version = machina_bpf::VERSION)]
struct Args {
    /// Unix socket to serve on.
    #[arg(long, env = "MACHINA_BPFD_SOCK", default_value = machina_bpf::api::DEFAULT_SOCKET)]
    socket: PathBuf,
    /// Directory for persisted policy/telemetry state.
    #[arg(
        long,
        env = "MACHINA_BPFD_STATE_DIR",
        default_value = "/var/lib/machina/bpf"
    )]
    state_dir: PathBuf,
    /// Group granted access to the socket (mode 0660).
    #[arg(long, env = "MACHINA_BPFD_SOCKET_GROUP", default_value = "machina")]
    socket_group: String,
    /// Print kernel feature detection and exit.
    #[arg(long)]
    probe: bool,
    /// Refuse to start if this binary was built without the embedded eBPF object.
    #[arg(long, env = "MACHINA_BPF_REQUIRED", default_value_t = false)]
    require_programs: bool,
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    if args.probe {
        let f = machina_bpf::loader::kernel_features();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "features": f,
                "programs_compiled": machina_bpf::loader::programs_compiled(),
                "programs_required": args.require_programs,
            }))?
        );
        return Ok(());
    }
    machina_bpf::startup::require_programs(
        args.require_programs,
        machina_bpf::loader::programs_compiled(),
    )?;
    machina_bpf::server::run(machina_bpf::server::Config {
        socket: args.socket,
        state_dir: args.state_dir,
        socket_group: (!args.socket_group.is_empty()).then_some(args.socket_group),
    })
    .await
}

#[cfg(not(target_os = "linux"))]
fn main() {
    let _ = Args::parse();
    eprintln!("machina-bpfd requires Linux");
    std::process::exit(1);
}
