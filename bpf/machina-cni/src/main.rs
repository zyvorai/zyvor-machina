// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! machina-cni — Machina's native eBPF Kubernetes networking.
//!
//! * Invoked by the kubelet/containerd with `CNI_COMMAND` set → CNI plugin
//!   (veth + /32 route + link-local gateway, file-based IPAM), registering
//!   each pod with machina-bpfd for redirect + NetworkPolicy.
//! * `machina-cni agent` → per-node reconciler: writes the CNI config from the
//!   node's podCIDR, programs pod routes to other nodes and masquerade, and
//!   compiles NetworkPolicies / Services into machina-bpfd's CNI maps
//!   (replacing Cilium, flannel and kube-proxy).

mod agent;
mod compile;
mod ipam;
mod plugin;

fn main() {
    if std::env::var_os("CNI_COMMAND").is_some() {
        std::process::exit(plugin::run());
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("agent") => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info".into()),
                )
                .init();
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build();
            let res = rt
                .map_err(anyhow::Error::from)
                .and_then(|rt| rt.block_on(agent::run(agent::Config::from_env())));
            if let Err(e) = res {
                eprintln!("machina-cni agent: {e:#}");
                // EX_CONFIG: the unit's RestartPreventExitStatus stops the restart loop.
                let code = if e.downcast_ref::<agent::ForeignCni>().is_some() {
                    78
                } else {
                    1
                };
                std::process::exit(code);
            }
        }
        Some("version") | Some("--version") => {
            println!("machina-cni {}", env!("CARGO_PKG_VERSION"))
        }
        _ => {
            eprintln!(
                "usage: machina-cni agent        run the node reconciler\n       \
                 machina-cni version\n\
                 As a CNI plugin it is invoked with CNI_COMMAND set (ADD/DEL/CHECK/VERSION)."
            );
            std::process::exit(2);
        }
    }
}
