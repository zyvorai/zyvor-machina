// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Pure planning for the agent's k3s node operations (install a server, join an agent, upgrade,
//! remove). Everything here validates input and builds the installer's arguments and environment;
//! nothing runs a process. The agent executes a [`Plan`] without a shell, so no value below is ever
//! interpolated into a command line, and the join token travels in the environment, never in argv.

use std::net::IpAddr;

pub const DEFAULT_INSTALL_URL: &str = "https://get.k3s.io";

/// What the controller asks a host to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    InstallServer,
    /// Join an extra server to an HA control plane (embedded etcd).
    JoinServer,
    JoinAgent,
    Upgrade,
    Remove,
    Status,
    /// `kubectl drain` of a node, run on a server host.
    Drain,
    /// `kubectl uncordon` of a node, run on a server host.
    Uncordon,
    /// `kubectl delete node`, run on a server host after the node's k3s is removed.
    DeleteNode,
}

impl Action {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "install_server" => Self::InstallServer,
            "join_server" => Self::JoinServer,
            "join_agent" => Self::JoinAgent,
            "upgrade" => Self::Upgrade,
            "remove" => Self::Remove,
            "status" => Self::Status,
            "drain" => Self::Drain,
            "uncordon" => Self::Uncordon,
            "delete_node" => Self::DeleteNode,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Request {
    pub action: String,
    /// `v1.31.4+k3s1`. Required to install, join or upgrade (no floating channel).
    pub version: String,
    /// `https://10.0.0.5:6443`, required to join.
    pub server_url: String,
    /// Join token, required to join. Never logged.
    pub token: String,
    pub node_name: String,
    /// Address to add as a TLS SAN on a new server (the address the kubeconfig will use).
    pub advertise_address: String,
    pub cluster_cidr: String,
    pub service_cidr: String,
    /// First server of an HA control plane: start embedded etcd (`--cluster-init`) so more servers
    /// can join. Leave false for a single-server cluster (SQLite).
    pub cluster_init: bool,
    /// Installer script URL (an internal mirror for air-gapped sites). Default [`DEFAULT_INSTALL_URL`].
    pub install_url: String,
}

/// How to run the installer: `sh <script> <args...>` with `env` added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub install_url: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// `v1.31.4` or `v1.31.4+k3s1`.
pub fn validate_version(v: &str) -> Result<(), String> {
    let (core, suffix) = match v.split_once('+') {
        Some((c, s)) => (c, Some(s)),
        None => (v, None),
    };
    let nums = core
        .strip_prefix('v')
        .ok_or("version must start with 'v'")?;
    let parts: Vec<&str> = nums.split('.').collect();
    let ok = parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit()));
    if !ok {
        return Err(format!("invalid k3s version '{v}' (expected v1.31.4+k3s1)"));
    }
    if let Some(s) = suffix {
        let digits = s.strip_prefix("k3s").unwrap_or("");
        if digits.is_empty() || digits.len() > 3 || !digits.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!(
                "invalid k3s version suffix '+{s}' (expected +k3sN)"
            ));
        }
    }
    Ok(())
}

/// `https://host:port` only: a hostname or IP, an optional port, no path, query or credentials.
pub fn validate_server_url(u: &str) -> Result<(), String> {
    let rest = u
        .strip_prefix("https://")
        .ok_or("server_url must start with https://")?;
    let (host, port) = match rest.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(']') || h.starts_with('[') => (h, Some(p)),
        _ => (rest, None),
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let host_ok = host.parse::<IpAddr>().is_ok()
        || (!host.is_empty()
            && host.len() <= 253
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.'));
    let port_ok = port.is_none_or(|p| p.parse::<u16>().is_ok_and(|n| n > 0));
    if host_ok && port_ok {
        Ok(())
    } else {
        Err(format!("invalid server_url '{u}'"))
    }
}

pub fn validate_token(t: &str) -> Result<(), String> {
    let ok = !t.is_empty()
        && t.len() <= 512
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '_' | '-' | '='));
    if ok {
        Ok(())
    } else {
        Err("invalid join token".into())
    }
}

/// A lowercase DNS label.
pub fn validate_node_name(n: &str) -> Result<(), String> {
    let ok = !n.is_empty()
        && n.len() <= 63
        && !n.starts_with('-')
        && !n.ends_with('-')
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "invalid node name '{n}' (lowercase letters, digits, '-')"
        ))
    }
}

pub fn validate_cidr(c: &str) -> Result<(), String> {
    let (ip, prefix) = c
        .split_once('/')
        .ok_or_else(|| format!("invalid CIDR '{c}'"))?;
    let ip: IpAddr = ip.parse().map_err(|_| format!("invalid CIDR '{c}'"))?;
    let max = if ip.is_ipv4() { 32 } else { 128 };
    match prefix.parse::<u8>() {
        Ok(p) if p <= max => Ok(()),
        _ => Err(format!("invalid CIDR '{c}'")),
    }
}

pub fn validate_install_url(u: &str) -> Result<(), String> {
    let rest = u
        .strip_prefix("https://")
        .or_else(|| u.strip_prefix("http://"))
        .ok_or("install_url must be http(s)")?;
    let ok = !rest.is_empty()
        && rest.len() <= 512
        && rest.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '/' | ':' | '_' | '~' | '%' | '+')
        });
    if ok {
        Ok(())
    } else {
        Err(format!("invalid install_url '{u}'"))
    }
}

/// Build the installer plan for an install, join or upgrade. `Remove` and `Status` have no plan.
pub fn plan(req: &Request) -> Result<Plan, String> {
    let action =
        Action::parse(&req.action).ok_or_else(|| format!("unknown action '{}'", req.action))?;
    if matches!(
        action,
        Action::Remove | Action::Status | Action::Drain | Action::Uncordon | Action::DeleteNode
    ) {
        return Err(format!("action '{}' has no installer plan", req.action));
    }
    validate_version(&req.version)?;
    let install_url = if req.install_url.is_empty() {
        DEFAULT_INSTALL_URL.to_string()
    } else {
        validate_install_url(&req.install_url)?;
        req.install_url.clone()
    };
    let mut env = vec![("INSTALL_K3S_VERSION".to_string(), req.version.clone())];
    let mut args: Vec<String> = Vec::new();

    if !req.node_name.is_empty() {
        validate_node_name(&req.node_name)?;
    }
    match action {
        Action::InstallServer | Action::JoinServer => {
            args.push("server".into());
            if matches!(action, Action::InstallServer) && req.cluster_init {
                args.push("--cluster-init".into());
            }
            if matches!(action, Action::JoinServer) {
                validate_server_url(&req.server_url)?;
                validate_token(&req.token)?;
                env.push(("K3S_URL".into(), req.server_url.clone()));
                env.push(("K3S_TOKEN".into(), req.token.clone()));
            }
            if !req.advertise_address.is_empty() {
                if req.advertise_address.parse::<IpAddr>().is_err()
                    && validate_server_url(&format!("https://{}", req.advertise_address)).is_err()
                {
                    return Err(format!(
                        "invalid advertise_address '{}'",
                        req.advertise_address
                    ));
                }
                args.push("--tls-san".into());
                args.push(req.advertise_address.clone());
            }
            if !req.cluster_cidr.is_empty() {
                validate_cidr(&req.cluster_cidr)?;
                args.push("--cluster-cidr".into());
                args.push(req.cluster_cidr.clone());
            }
            if !req.service_cidr.is_empty() {
                validate_cidr(&req.service_cidr)?;
                args.push("--service-cidr".into());
                args.push(req.service_cidr.clone());
            }
        }
        Action::JoinAgent => {
            validate_server_url(&req.server_url)?;
            validate_token(&req.token)?;
            args.push("agent".into());
            env.push(("K3S_URL".into(), req.server_url.clone()));
            env.push(("K3S_TOKEN".into(), req.token.clone()));
        }
        // An upgrade re-runs the installer, which keeps the existing service configuration.
        Action::Upgrade => {}
        Action::Remove | Action::Status | Action::Drain | Action::Uncordon | Action::DeleteNode => {
            unreachable!()
        }
    }
    if !req.node_name.is_empty()
        && matches!(
            action,
            Action::InstallServer | Action::JoinServer | Action::JoinAgent
        )
    {
        args.push("--node-name".into());
        args.push(req.node_name.clone());
    }
    Ok(Plan {
        install_url,
        args,
        env,
    })
}

/// `k3s kubectl` arguments to drain, uncordon or delete `node_name`, run on a server host. Pods that no
/// controller manages are not forced off (no `--force`): such a pod blocks the drain, which then
/// fails visibly rather than deleting data.
pub fn kubectl_args(
    action: Action,
    node_name: &str,
    timeout_secs: u32,
) -> Result<Vec<String>, String> {
    validate_node_name(node_name)?;
    match action {
        Action::Drain => Ok(vec![
            "kubectl".into(),
            "drain".into(),
            node_name.into(),
            "--ignore-daemonsets".into(),
            "--delete-emptydir-data".into(),
            format!("--timeout={}s", timeout_secs.clamp(30, 3600)),
        ]),
        Action::Uncordon => Ok(vec!["kubectl".into(), "uncordon".into(), node_name.into()]),
        Action::DeleteNode => Ok(vec![
            "kubectl".into(),
            "delete".into(),
            "node".into(),
            node_name.into(),
        ]),
        _ => Err("kubectl_args is for drain and uncordon".into()),
    }
}

/// Which uninstall script exists on the host decides what is removed.
pub fn uninstall_script(
    server_script_exists: bool,
    agent_script_exists: bool,
) -> Option<&'static str> {
    if server_script_exists {
        Some("/usr/local/bin/k3s-uninstall.sh")
    } else if agent_script_exists {
        Some("/usr/local/bin/k3s-agent-uninstall.sh")
    } else {
        None
    }
}

/// The kubeconfig k3s writes points at 127.0.0.1; rewrite it to the address clients should use.
pub fn kubeconfig_for(raw: &str, api_url: &str) -> String {
    raw.replace("https://127.0.0.1:6443", api_url)
        .replace("https://localhost:6443", api_url)
}

/// Keep the last `max` bytes of command output, on a character boundary.
pub fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut start = s.len() - max;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    s[start..].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(action: &str) -> Request {
        Request {
            action: action.into(),
            version: "v1.31.4+k3s1".into(),
            ..Default::default()
        }
    }

    #[test]
    fn versions() {
        for ok in ["v1.31.4", "v1.31.4+k3s1", "v1.30.12+k3s2"] {
            assert!(validate_version(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "1.31.4",
            "v1.31",
            "v1.31.4+rke2r1",
            "v1.31.4+k3s",
            "v1.31.4;rm",
            "v1..4",
            "latest",
        ] {
            assert!(validate_version(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn server_urls() {
        for ok in [
            "https://10.0.0.5:6443",
            "https://k8s.example.com:6443",
            "https://[fd00::1]:6443",
            "https://10.0.0.5",
        ] {
            assert!(validate_server_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://10.0.0.5:6443",
            "https://user:pw@10.0.0.5:6443",
            "https://10.0.0.5:6443/x",
            "https://",
            "https://a b:6443",
            "https://10.0.0.5:0",
            "https://10.0.0.5:99999",
        ] {
            assert!(validate_server_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn tokens_names_cidrs_urls() {
        assert!(validate_token("K10abc123::server:def456").is_ok());
        assert!(validate_token("a b").is_err());
        assert!(validate_token("x;reboot").is_err());
        assert!(validate_token("").is_err());
        assert!(validate_node_name("worker-1").is_ok());
        assert!(validate_node_name("Worker").is_err());
        assert!(validate_node_name("-x").is_err());
        assert!(validate_cidr("10.42.0.0/16").is_ok());
        assert!(validate_cidr("fd00::/64").is_ok());
        assert!(validate_cidr("10.42.0.0/33").is_err());
        assert!(validate_cidr("10.42.0.0").is_err());
        assert!(validate_install_url("http://mirror.local/k3s/install.sh").is_ok());
        assert!(validate_install_url("file:///etc/passwd").is_err());
        assert!(validate_install_url("https://x/y z").is_err());
    }

    #[test]
    fn install_server_plan() {
        let mut r = req("install_server");
        r.advertise_address = "10.0.0.5".into();
        r.node_name = "cp-1".into();
        r.cluster_cidr = "10.42.0.0/16".into();
        let p = plan(&r).unwrap();
        assert_eq!(p.install_url, DEFAULT_INSTALL_URL);
        assert_eq!(
            p.args,
            [
                "server",
                "--tls-san",
                "10.0.0.5",
                "--cluster-cidr",
                "10.42.0.0/16",
                "--node-name",
                "cp-1"
            ]
        );
        assert_eq!(
            p.env,
            [(
                "INSTALL_K3S_VERSION".to_string(),
                "v1.31.4+k3s1".to_string()
            )]
        );
    }

    #[test]
    fn join_plan_puts_the_token_in_env_not_args() {
        let mut r = req("join_agent");
        r.server_url = "https://10.0.0.5:6443".into();
        r.token = "K10secret::server:abc".into();
        r.node_name = "w-1".into();
        r.install_url = "http://mirror.local/install.sh".into();
        let p = plan(&r).unwrap();
        assert_eq!(p.install_url, "http://mirror.local/install.sh");
        assert_eq!(p.args, ["agent", "--node-name", "w-1"]);
        assert!(p
            .env
            .contains(&("K3S_TOKEN".into(), "K10secret::server:abc".into())));
        assert!(p
            .env
            .contains(&("K3S_URL".into(), "https://10.0.0.5:6443".into())));
        assert!(!p.args.iter().any(|a| a.contains("secret")));
    }

    #[test]
    fn ha_servers_start_etcd_on_the_first_and_join_the_rest() {
        let mut first = req("install_server");
        first.cluster_init = true;
        first.advertise_address = "10.0.0.5".into();
        assert_eq!(
            plan(&first).unwrap().args,
            ["server", "--cluster-init", "--tls-san", "10.0.0.5"]
        );
        // A single-server cluster keeps SQLite: no --cluster-init.
        assert_eq!(plan(&req("install_server")).unwrap().args, ["server"]);

        let mut more = req("join_server");
        more.server_url = "https://10.0.0.5:6443".into();
        more.token = "K10secret::server:abc".into();
        more.node_name = "cp-2".into();
        more.cluster_init = true; // ignored for a join
        let p = plan(&more).unwrap();
        assert_eq!(p.args, ["server", "--node-name", "cp-2"]);
        assert!(p
            .env
            .contains(&("K3S_TOKEN".into(), "K10secret::server:abc".into())));
        assert!(p
            .env
            .contains(&("K3S_URL".into(), "https://10.0.0.5:6443".into())));
        assert!(!p.args.iter().any(|a| a.contains("secret")));
        // A join needs the first server's URL and the token.
        assert!(plan(&req("join_server")).is_err());
    }

    #[test]
    fn drain_and_uncordon_are_kubectl_not_installer_plans() {
        assert_eq!(Action::parse("drain"), Some(Action::Drain));
        assert_eq!(Action::parse("uncordon"), Some(Action::Uncordon));
        assert!(plan(&req("drain")).is_err());
        assert_eq!(
            kubectl_args(Action::Drain, "worker-1", 300).unwrap(),
            [
                "kubectl",
                "drain",
                "worker-1",
                "--ignore-daemonsets",
                "--delete-emptydir-data",
                "--timeout=300s"
            ]
        );
        // No --force: an unmanaged pod blocks the drain instead of being deleted.
        assert!(!kubectl_args(Action::Drain, "w", 300)
            .unwrap()
            .iter()
            .any(|a| a == "--force"));
        assert_eq!(
            kubectl_args(Action::Uncordon, "worker-1", 0).unwrap(),
            ["kubectl", "uncordon", "worker-1"]
        );
        assert_eq!(
            kubectl_args(Action::Drain, "w", 1).unwrap().last().unwrap(),
            "--timeout=30s"
        );
        assert_eq!(
            kubectl_args(Action::Drain, "w", 999_999)
                .unwrap()
                .last()
                .unwrap(),
            "--timeout=3600s"
        );
        assert!(kubectl_args(Action::Drain, "Bad Name", 300).is_err());
        assert!(kubectl_args(Action::Drain, "--all", 300).is_err());
        assert!(kubectl_args(Action::Upgrade, "w", 300).is_err());
        assert_eq!(Action::parse("delete_node"), Some(Action::DeleteNode));
        assert_eq!(
            kubectl_args(Action::DeleteNode, "w-1", 0).unwrap(),
            ["kubectl", "delete", "node", "w-1"]
        );
        assert!(plan(&req("delete_node")).is_err());
    }

    #[test]
    fn upgrade_reruns_the_installer_with_only_the_version() {
        let p = plan(&req("upgrade")).unwrap();
        assert!(p.args.is_empty());
        assert_eq!(p.env.len(), 1);
    }

    #[test]
    fn bad_requests_are_refused() {
        assert!(plan(&req("explode")).is_err());
        assert!(plan(&req("remove")).is_err());
        assert!(plan(&req("status")).is_err());
        let mut r = req("join_agent");
        assert!(plan(&r).is_err()); // no server url / token
        r.server_url = "https://10.0.0.5:6443".into();
        assert!(plan(&r).is_err()); // no token
        r.token = "t".into();
        assert!(plan(&r).is_ok());
        let mut v = req("upgrade");
        v.version = String::new();
        assert!(plan(&v).is_err());
        let mut s = req("install_server");
        s.advertise_address = "10.0.0.5 --disable traefik".into();
        assert!(plan(&s).is_err());
    }

    #[test]
    fn uninstall_and_kubeconfig_and_tail() {
        assert_eq!(
            uninstall_script(true, true),
            Some("/usr/local/bin/k3s-uninstall.sh")
        );
        assert_eq!(
            uninstall_script(false, true),
            Some("/usr/local/bin/k3s-agent-uninstall.sh")
        );
        assert_eq!(uninstall_script(false, false), None);
        let kc = "server: https://127.0.0.1:6443\n";
        assert_eq!(
            kubeconfig_for(kc, "https://10.0.0.5:6443"),
            "server: https://10.0.0.5:6443\n"
        );
        assert_eq!(tail("héllo wörld", 6), "wörld");
        assert_eq!(tail("abc", 10), "abc");
    }
}
