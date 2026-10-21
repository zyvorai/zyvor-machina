// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Executes k3s node operations for the controller (see `machina_core::k3s_node` for the validation
//! and installer plan). This runs root installers, so it is off unless the host's operator sets
//! `MACHINA_AGENT_K3S_ENABLED=1`. Processes are started without a shell; the join token reaches the
//! installer through its environment only and is never logged or echoed back.

use std::path::Path;
use std::time::Duration;

use machina_core::k3s_node::{self, Action, Request};
use tokio::process::Command;

use crate::pb::{K3sNodeRequest, K3sNodeResponse};

const ENABLE_ENV: &str = "MACHINA_AGENT_K3S_ENABLED";
const NODE_TOKEN_PATH: &str = "/var/lib/rancher/k3s/server/node-token";
const KUBECONFIG_PATH: &str = "/etc/rancher/k3s/k3s.yaml";
const K3S_BIN: &str = "/usr/local/bin/k3s";
const OUTPUT_TAIL: usize = 4000;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(180);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(900);
const REMOVE_TIMEOUT: Duration = Duration::from_secs(300);
/// kubectl's own eviction timeout; the process is allowed a little longer.
const KUBECTL_DRAIN_SECS: u32 = 600;
const DRAIN_TIMEOUT: Duration = Duration::from_secs(660);

fn fail(message: impl Into<String>) -> K3sNodeResponse {
    K3sNodeResponse {
        ok: false,
        message: message.into(),
        ..Default::default()
    }
}

pub fn enabled() -> bool {
    std::env::var(ENABLE_ENV).is_ok_and(|v| v == "1")
}

struct Out {
    ok: bool,
    text: String,
}

async fn run(program: &str, args: &[String], env: &[(String, String)], limit: Duration) -> Out {
    let mut cmd = Command::new(program);
    cmd.args(args).kill_on_drop(true);
    for (k, v) in env {
        cmd.env(k, v);
    }
    match tokio::time::timeout(limit, cmd.output()).await {
        Err(_) => Out {
            ok: false,
            text: format!("{program} timed out after {}s", limit.as_secs()),
        },
        Ok(Err(e)) => Out {
            ok: false,
            text: format!("cannot run {program}: {e}"),
        },
        Ok(Ok(o)) => {
            let mut text = String::from_utf8_lossy(&o.stdout).to_string();
            text.push_str(&String::from_utf8_lossy(&o.stderr));
            Out {
                ok: o.status.success(),
                text,
            }
        }
    }
}

pub async fn handle(req: K3sNodeRequest) -> K3sNodeResponse {
    if !enabled() {
        return fail(format!(
            "k3s operations are disabled on this host: set {ENABLE_ENV}=1 in the agent environment to allow them"
        ));
    }
    let r = Request {
        action: req.action.clone(),
        version: req.version,
        server_url: req.server_url,
        token: req.token,
        node_name: req.node_name,
        advertise_address: req.advertise_address,
        cluster_cidr: req.cluster_cidr,
        service_cidr: req.service_cidr,
        install_url: req.install_url,
        cluster_init: req.cluster_init,
    };
    match Action::parse(&r.action) {
        None => fail(format!("unknown action '{}'", r.action)),
        Some(Action::Status) => status().await,
        Some(Action::Remove) => remove().await,
        Some(action @ (Action::Drain | Action::Uncordon | Action::DeleteNode)) => {
            kubectl(&r, action).await
        }
        Some(action) => install(&r, action).await,
    }
}

async fn install(r: &Request, action: Action) -> K3sNodeResponse {
    let plan = match k3s_node::plan(r) {
        Ok(p) => p,
        Err(e) => return fail(e),
    };
    // Download the installer to a private temp file, then run it with sh. No shell parses the URL.
    let dir = std::env::temp_dir().join(format!("machina-k3s-{}", uuid::Uuid::new_v4()));
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return fail(format!("cannot create {}: {e}", dir.display()));
    }
    let script = dir.join("install.sh");
    let script_s = script.to_string_lossy().to_string();
    let dl = run(
        "curl",
        &[
            "-fsSL".into(),
            "--max-time".into(),
            "120".into(),
            "-o".into(),
            script_s.clone(),
            plan.install_url.clone(),
        ],
        &[],
        DOWNLOAD_TIMEOUT,
    )
    .await;
    if !dl.ok {
        let _ = tokio::fs::remove_dir_all(&dir).await;
        return fail(format!(
            "downloading the installer failed: {}",
            k3s_node::tail(&dl.text, 500)
        ));
    }
    let mut args = vec![script_s];
    args.extend(plan.args.iter().cloned());
    let out = run("sh", &args, &plan.env, INSTALL_TIMEOUT).await;
    let _ = tokio::fs::remove_dir_all(&dir).await;
    if !out.ok {
        return K3sNodeResponse {
            ok: false,
            message: "the k3s installer failed".into(),
            output_tail: redact(&k3s_node::tail(&out.text, OUTPUT_TAIL), &r.token),
            ..Default::default()
        };
    }
    let mut resp = K3sNodeResponse {
        ok: true,
        message: "installed".into(),
        output_tail: redact(&k3s_node::tail(&out.text, OUTPUT_TAIL), &r.token),
        ..Default::default()
    };
    if matches!(action, Action::InstallServer) {
        match (
            tokio::fs::read_to_string(NODE_TOKEN_PATH).await,
            tokio::fs::read_to_string(KUBECONFIG_PATH).await,
        ) {
            (Ok(t), Ok(k)) => {
                resp.node_token = t.trim().to_string();
                resp.kubeconfig = k;
            }
            (t, k) => {
                return fail(format!(
                    "k3s installed but its token or kubeconfig could not be read ({:?} / {:?})",
                    t.err().map(|e| e.to_string()),
                    k.err().map(|e| e.to_string())
                ));
            }
        }
    }
    resp
}

/// Drain, uncordon or delete a node with the k3s bundled kubectl. Runs on a server host only (it needs the
/// admin kubeconfig k3s writes there).
async fn kubectl(r: &Request, action: Action) -> K3sNodeResponse {
    if !Path::new(K3S_BIN).exists() || !Path::new(KUBECONFIG_PATH).exists() {
        return fail("this host is not a k3s server (no k3s binary or admin kubeconfig)");
    }
    let args = match k3s_node::kubectl_args(action, &r.node_name, KUBECTL_DRAIN_SECS) {
        Ok(a) => a,
        Err(e) => return fail(e),
    };
    let env = [("KUBECONFIG".to_string(), KUBECONFIG_PATH.to_string())];
    let out = run(K3S_BIN, &args, &env, DRAIN_TIMEOUT).await;
    K3sNodeResponse {
        ok: out.ok,
        message: if out.ok {
            match action {
                Action::Drain => "drained".into(),
                Action::DeleteNode => "node deleted".into(),
                _ => "uncordoned".into(),
            }
        } else {
            format!(
                "kubectl {} failed",
                args.get(1).map(String::as_str).unwrap_or("")
            )
        },
        output_tail: k3s_node::tail(&out.text, OUTPUT_TAIL),
        ..Default::default()
    }
}

async fn remove() -> K3sNodeResponse {
    let script = k3s_node::uninstall_script(
        Path::new("/usr/local/bin/k3s-uninstall.sh").exists(),
        Path::new("/usr/local/bin/k3s-agent-uninstall.sh").exists(),
    );
    let Some(script) = script else {
        return K3sNodeResponse {
            ok: true,
            message: "k3s is not installed".into(),
            ..Default::default()
        };
    };
    let out = run(script, &[], &[], REMOVE_TIMEOUT).await;
    K3sNodeResponse {
        ok: out.ok,
        message: if out.ok {
            "removed".into()
        } else {
            "uninstall failed".into()
        },
        output_tail: k3s_node::tail(&out.text, OUTPUT_TAIL),
        ..Default::default()
    }
}

async fn status() -> K3sNodeResponse {
    let ver = run(K3S_BIN, &["--version".into()], &[], Duration::from_secs(10)).await;
    if !ver.ok {
        return K3sNodeResponse {
            ok: true,
            message: "k3s is not installed".into(),
            ..Default::default()
        };
    }
    let mut active = false;
    for unit in ["k3s", "k3s-agent"] {
        let o = run(
            "systemctl",
            &["is-active".into(), unit.into()],
            &[],
            Duration::from_secs(10),
        )
        .await;
        if o.ok {
            active = true;
        }
    }
    K3sNodeResponse {
        ok: true,
        message: "installed".into(),
        version: ver.text.lines().next().unwrap_or("").trim().to_string(),
        active,
        ..Default::default()
    }
}

/// Belt and braces: the installer does not print the token, but never return it if it does.
fn redact(text: &str, token: &str) -> String {
    if token.is_empty() {
        text.to_string()
    } else {
        text.replace(token, "***")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_hides_the_token() {
        assert_eq!(redact("join K10abc ok", "K10abc"), "join *** ok");
        assert_eq!(redact("nothing", ""), "nothing");
    }

    #[tokio::test]
    async fn drain_is_refused_on_a_host_that_is_not_a_k3s_server() {
        if Path::new(K3S_BIN).exists() {
            return; // a real k3s host: this refusal does not apply
        }
        let r = kubectl(
            &Request {
                node_name: "worker-1".into(),
                ..Default::default()
            },
            Action::Drain,
        )
        .await;
        assert!(!r.ok && r.message.contains("not a k3s server"));
    }

    #[tokio::test]
    async fn disabled_by_default() {
        // The test environment does not set the opt-in variable.
        std::env::remove_var(ENABLE_ENV);
        let r = handle(K3sNodeRequest {
            action: "status".into(),
            ..Default::default()
        })
        .await;
        assert!(!r.ok);
        assert!(r.message.contains(ENABLE_ENV));
    }
}
