// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Instance metadata service (EC2-style `http://169.254.169.254/`).
//!
//! The controller pushes one entry per guest (`imds.sync`); a small HTTP server answers each request from the entry whose
//! address is the request's *source* address. An nft rule redirects 169.254.169.254:80 on the host's bridges to that server.
//! A guest therefore sees only its own data. Anything on the guest (including a vulnerable web app: SSRF) can read it, exactly
//! as on EC2, so user-data should not hold long-lived secrets. Disable with `MACHINA_IMDS=0`.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::process::{Command, Stdio};
use std::sync::RwLock;

use axum::extract::ConnectInfo;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use serde_json::{json, Value};

pub const METADATA_ADDR: &str = "169.254.169.254";
const DEFAULT_PORT: u16 = 8169;

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct Instance {
    pub instance_id: String,
    pub hostname: String,
    #[serde(default)]
    pub ips: Vec<String>,
    #[serde(default)]
    pub instance_type: String,
    #[serde(default)]
    pub public_keys: Vec<String>,
    #[serde(default)]
    pub user_data: Option<String>,
    #[serde(default)]
    pub project: String,
    /// `false` when the instance's metadata endpoint was switched off (`ModifyInstanceMetadataOptions`): every request gets 404.
    #[serde(default = "enabled")]
    pub enabled: bool,
}

fn enabled() -> bool {
    true
}

static STORE: RwLock<Option<HashMap<IpAddr, Instance>>> = RwLock::new(None);

/// Replace the whole table from an `imds.sync` payload `{instances:[{instance_id, hostname, ips, …}]}`.
pub fn sync(payload: &Value) -> Result<usize, String> {
    let list: Vec<Instance> = serde_json::from_value(payload.get("instances").cloned().unwrap_or(json!([])))
        .map_err(|e| format!("imds.sync payload: {e}"))?;
    let mut map = HashMap::new();
    for inst in list {
        for ip in &inst.ips {
            if let Ok(a) = ip.parse::<IpAddr>() {
                map.insert(a, inst.clone());
            }
        }
    }
    let n = map.len();
    *STORE.write().map_err(|_| "imds store poisoned".to_string())? = Some(map);
    Ok(n)
}

fn lookup(ip: IpAddr) -> Option<Instance> {
    STORE.read().ok()?.as_ref()?.get(&ip).cloned()
}

/// `/2009-04-04/meta-data/instance-id` and `/latest/meta-data/instance-id` → `meta-data/instance-id`.
pub fn strip_version(path: &str) -> Option<&str> {
    let p = path.trim_start_matches('/');
    let (ver, rest) = p.split_once('/').unwrap_or((p, ""));
    let dated = ver.len() == 10 && ver.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() });
    (ver == "latest" || dated).then_some(rest)
}

/// The body for `rest` (the path after the version) or None for 404.
pub fn answer(inst: &Instance, rest: &str) -> Option<(String, &'static str)> {
    const TEXT: &str = "text/plain";
    let ip = inst.ips.first().cloned().unwrap_or_default();
    let rest = rest.trim_end_matches('/');
    let body = match rest {
        "" => "meta-data\nuser-data\ndynamic".to_string(),
        "meta-data" => "ami-id\nhostname\ninstance-id\ninstance-type\nlocal-hostname\nlocal-ipv4\nplacement/\npublic-keys/".to_string(),
        "meta-data/instance-id" => inst.instance_id.clone(),
        "meta-data/hostname" | "meta-data/local-hostname" => inst.hostname.clone(),
        "meta-data/local-ipv4" => ip,
        "meta-data/instance-type" => inst.instance_type.clone(),
        "meta-data/ami-id" => "ami-00000000000000000".to_string(),
        "meta-data/placement" => "availability-zone".to_string(),
        "meta-data/placement/availability-zone" => "machina-a".to_string(),
        "meta-data/public-keys" => inst.public_keys.iter().enumerate().map(|(i, _)| format!("{i}=machina-key-{i}")).collect::<Vec<_>>().join("\n"),
        "user-data" => return inst.user_data.clone().map(|u| (u, TEXT)),
        "dynamic/instance-identity/document" => {
            return Some((
                json!({ "instanceId": inst.instance_id, "instanceType": inst.instance_type, "privateIp": ip,
                        "availabilityZone": "machina-a", "region": "machina", "accountId": "000000000000" })
                .to_string(),
                "application/json",
            ))
        }
        other => {
            // meta-data/public-keys/<n>[/openssh-key]
            let key_path = other.strip_prefix("meta-data/public-keys/")?;
            let (n, tail) = key_path.split_once('/').unwrap_or((key_path, ""));
            let key = inst.public_keys.get(n.parse::<usize>().ok()?)?;
            match tail {
                "" => "openssh-key".to_string(),
                "openssh-key" => key.clone(),
                _ => return None,
            }
        }
    };
    Some((body, TEXT))
}

async fn handle(ConnectInfo(peer): ConnectInfo<SocketAddr>, uri: Uri) -> Response {
    let Some(inst) = lookup(peer.ip()) else {
        return (StatusCode::NOT_FOUND, "no instance at your address").into_response();
    };
    if !inst.enabled {
        return (StatusCode::NOT_FOUND, "the metadata endpoint is disabled for this instance").into_response();
    }
    match strip_version(uri.path()).and_then(|rest| answer(&inst, rest)) {
        Some((body, ctype)) => ([(header::CONTENT_TYPE, ctype)], body).into_response(),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

/// nft rules that send guests' requests for the metadata address to the local server.
pub fn redirect_rules(port: u16) -> String {
    format!(
        "table ip machina_imds {{\n  chain pre {{\n    type nat hook prerouting priority dstnat; policy accept;\n    ip daddr {METADATA_ADDR} tcp dport 80 redirect to :{port}\n  }}\n}}\n"
    )
}

fn install_redirect(port: u16) -> Result<(), String> {
    let _ = Command::new("nft").args(["delete", "table", "ip", "machina_imds"]).stderr(Stdio::null()).status();
    let mut child = Command::new("nft")
        .args(["-f", "-"])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("nft: {e}"))?;
    {
        use std::io::Write;
        child.stdin.take().ok_or("nft stdin")?.write_all(redirect_rules(port).as_bytes()).map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Start the server and the redirect (unless `MACHINA_IMDS=0`). Failures are logged: the agent runs without a metadata service.
pub fn spawn() {
    if std::env::var("MACHINA_IMDS").as_deref() == Ok("0") {
        tracing::info!("instance metadata service disabled (MACHINA_IMDS=0)");
        return;
    }
    let port = std::env::var("MACHINA_IMDS_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);
    tokio::spawn(async move {
        let app = Router::new().route("/", get(handle)).route("/{*path}", get(handle));
        let listener = match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("instance metadata service: cannot listen on :{port}: {e}");
                return;
            }
        };
        match tokio::task::spawn_blocking(move || install_redirect(port)).await {
            Ok(Ok(())) => tracing::info!("instance metadata service on :{port}, {METADATA_ADDR}:80 redirected"),
            Ok(Err(e)) => tracing::warn!("instance metadata service: could not install the nft redirect: {e}"),
            Err(e) => tracing::warn!("instance metadata service: {e}"),
        }
        if let Err(e) = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await {
            tracing::warn!("instance metadata service stopped: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst() -> Instance {
        Instance {
            instance_id: "i-0123456789abcdef0".into(),
            hostname: "web-1".into(),
            ips: vec!["192.168.122.50".into()],
            instance_type: "small".into(),
            public_keys: vec!["ssh-ed25519 AAAA test".into()],
            user_data: Some("#!/bin/sh\necho hi\n".into()),
            project: "default".into(),
            enabled: true,
        }
    }

    #[test]
    fn the_endpoint_switch_defaults_on_and_follows_the_sync() {
        let on: Instance = serde_json::from_value(json!({ "instance_id": "i-1", "hostname": "h" })).unwrap();
        assert!(on.enabled, "an entry without the field (an older controller) stays enabled");
        let off: Instance = serde_json::from_value(json!({ "instance_id": "i-1", "hostname": "h", "enabled": false })).unwrap();
        assert!(!off.enabled);
        // `sync` keeps the field (checked on the entries it would store, not through the shared table other tests replace)
        let list: Vec<Instance> =
            serde_json::from_value(json!([{ "instance_id": "i-9", "hostname": "h", "ips": ["10.9.9.9"], "enabled": false }])).unwrap();
        assert!(!list[0].enabled);
    }

    #[test]
    fn versions_are_stripped() {
        assert_eq!(strip_version("/latest/meta-data/instance-id"), Some("meta-data/instance-id"));
        assert_eq!(strip_version("/2009-04-04/meta-data/instance-id"), Some("meta-data/instance-id"));
        assert_eq!(strip_version("/2009-04-04"), Some(""));
        assert_eq!(strip_version("/foo/meta-data"), None);
        assert_eq!(strip_version("/20090404/meta-data"), None);
    }

    #[test]
    fn answers_the_documented_paths() {
        let i = inst();
        assert_eq!(answer(&i, "meta-data/instance-id").unwrap().0, "i-0123456789abcdef0");
        assert_eq!(answer(&i, "meta-data/local-ipv4").unwrap().0, "192.168.122.50");
        assert_eq!(answer(&i, "meta-data/hostname").unwrap().0, "web-1");
        assert!(answer(&i, "meta-data/").unwrap().0.contains("instance-id"));
        assert_eq!(answer(&i, "meta-data/public-keys").unwrap().0, "0=machina-key-0");
        assert_eq!(answer(&i, "meta-data/public-keys/0/openssh-key").unwrap().0, "ssh-ed25519 AAAA test");
        assert_eq!(answer(&i, "user-data").unwrap().0, "#!/bin/sh\necho hi\n");
        let doc = answer(&i, "dynamic/instance-identity/document").unwrap();
        assert_eq!(doc.1, "application/json");
        assert!(doc.0.contains("\"instanceId\":\"i-0123456789abcdef0\""));
    }

    #[test]
    fn unknown_paths_and_missing_user_data_are_404() {
        let mut i = inst();
        assert!(answer(&i, "meta-data/nonsense").is_none());
        assert!(answer(&i, "meta-data/public-keys/5/openssh-key").is_none());
        assert!(answer(&i, "meta-data/public-keys/x").is_none());
        i.user_data = None;
        assert!(answer(&i, "user-data").is_none());
    }

    #[test]
    fn sync_replaces_the_table_and_lookup_is_by_source_address() {
        let n = sync(&json!({ "instances": [{ "instance_id": "i-a", "hostname": "a", "ips": ["10.0.0.5", "10.0.0.6"] }] })).unwrap();
        assert_eq!(n, 2);
        assert_eq!(lookup("10.0.0.6".parse().unwrap()).unwrap().instance_id, "i-a");
        assert!(lookup("10.0.0.7".parse().unwrap()).is_none());
        sync(&json!({ "instances": [] })).unwrap();
        assert!(lookup("10.0.0.5".parse().unwrap()).is_none());
        assert!(sync(&json!({ "instances": [{ "hostname": "x" }] })).is_err(), "instance_id is required");
    }

    #[test]
    fn the_redirect_rule_targets_the_metadata_address() {
        let r = redirect_rules(8169);
        assert!(r.contains("ip daddr 169.254.169.254 tcp dport 80 redirect to :8169"));
        assert!(r.contains("hook prerouting"));
    }
}
