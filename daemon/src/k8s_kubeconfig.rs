// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Auto-select a kubeconfig file when the default kubectl config cannot reach the API (TLS, etc.).
//!
//! Order: probe default (`kubectl` with env); then well-known admin configs (k3s, RKE2, kubeadm);
//! then each `KUBECONFIG` path segment; then `~/.kube/config`. First file that succeeds
//! `kubectl get --raw /version` wins. Cached for the process lifetime.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
use tracing::{info, warn};

const PROBE_TIMEOUT_SECS: u64 = 8;

#[derive(Clone, Debug)]
pub struct KubeconfigChoice {
    /// Extra args inserted after `kubectl` (empty or `--kubeconfig`, `<path>`).
    pub prefix: Vec<String>,
    /// When set, machina auto-selected this file because the default probe failed.
    pub auto_selected_path: Option<String>,
}

static CACHE: OnceLock<Mutex<Option<KubeconfigChoice>>> = OnceLock::new();

fn cache() -> &'static Mutex<Option<KubeconfigChoice>> {
    CACHE.get_or_init(|| Mutex::new(None))
}

async fn raw_version_ok(kubeconfig: Option<&Path>) -> bool {
    let mut cmd = Command::new("kubectl");
    cmd.args(["get", "--raw", "/version", "--request-timeout=5s"]);
    if let Some(p) = kubeconfig {
        let Some(ps) = p.to_str() else {
            return false;
        };
        cmd.args(["--kubeconfig", ps]);
    }
    match timeout(Duration::from_secs(PROBE_TIMEOUT_SECS), cmd.output()).await {
        Ok(Ok(out)) => out.status.success(),
        _ => false,
    }
}

fn prioritized_paths() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen = HashSet::new();

    let mut push = |pb: PathBuf| {
        if pb.is_file() {
            let key = pb.to_string_lossy().to_string();
            if seen.insert(key) {
                out.push(pb);
            }
        }
    };

    #[cfg(unix)]
    {
        push(PathBuf::from("/etc/rancher/k3s/k3s.yaml"));
        push(PathBuf::from("/etc/rancher/rke2/rke2.yaml"));
        push(PathBuf::from("/etc/kubernetes/admin.conf"));
        push(PathBuf::from(
            "/var/snap/microk8s/current/credentials/client.config",
        ));
    }

    if let Ok(kc) = std::env::var("KUBECONFIG") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for part in kc.split(sep) {
            let t = part.trim();
            if t.is_empty() {
                continue;
            }
            push(PathBuf::from(t));
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        push(PathBuf::from(format!("{home}/.kube/config")));
    }

    out
}

async fn probe_choice() -> KubeconfigChoice {
    if raw_version_ok(None).await {
        info!(target: "machina_k8s", "kubectl kubeconfig: default (env / merged config) reaches API");
        return KubeconfigChoice {
            prefix: Vec::new(),
            auto_selected_path: None,
        };
    }

    for path in prioritized_paths() {
        if raw_version_ok(Some(&path)).await {
            let ps = path.to_string_lossy().to_string();
            info!(
                target: "machina_k8s",
                "kubectl kubeconfig: auto-selected {} (default config did not reach API)",
                ps
            );
            return KubeconfigChoice {
                prefix: vec!["--kubeconfig".into(), ps.clone()],
                auto_selected_path: Some(ps),
            };
        }
    }

    warn!(
        target: "machina_k8s",
        "kubectl kubeconfig: no candidate reached the API; continuing with kubectl default"
    );
    KubeconfigChoice {
        prefix: Vec::new(),
        auto_selected_path: None,
    }
}

/// Returns extra `kubectl` args (typically empty or `--kubeconfig` + path) and metadata.
pub async fn kubectl_kubeconfig_choice() -> KubeconfigChoice {
    let lock = cache();
    let mut g = lock.lock().await;
    if let Some(c) = g.as_ref() {
        return c.clone();
    }
    let c = probe_choice().await;
    *g = Some(c.clone());
    c
}
