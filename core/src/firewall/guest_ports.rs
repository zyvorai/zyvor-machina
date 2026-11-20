// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::types::{ExposureRisk, OpenPort};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GuestListeningPort {
    pub port: u16,
    pub protocol: String,
    pub bind_address: String,
    pub process: Option<String>,
}

#[cfg(target_os = "linux")]
pub fn scan_guest_listening_ports(vm_name: &str) -> Vec<GuestListeningPort> {
    use std::process::Command;
    // `vm_name` is a bare argv element to `virsh qemu-agent-command <vm_name> ...`
    // with no `--` separator; unlike most other name-taking functions in this
    // workspace, nothing upstream of this best-effort probe is guaranteed to have
    // run `validate::validate_name` first, so guard it here rather than let a name
    // starting with '-' be parsed as a virsh option.
    if !vm_name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        || vm_name.is_empty()
        || vm_name.starts_with('-')
        || vm_name.starts_with('.')
    {
        return Vec::new();
    }
    let exec_json = r#"{"execute":"guest-exec","arguments":{"path":"ss","arg":["-tlnp"],"capture-output":true}}"#;
    let out = match Command::new("virsh")
        .args(["qemu-agent-command", vm_name, exec_json])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    let v: serde_json::Value = match serde_json::from_slice(&out.stdout) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(pid) = v
        .get("return")
        .and_then(|r| r.get("pid"))
        .and_then(|p| p.as_u64())
    else {
        return Vec::new();
    };
    std::thread::sleep(std::time::Duration::from_millis(500));
    let status_json = format!(r#"{{"execute":"guest-exec-status","arguments":{{"pid":{pid}}}}}"#);
    let st = match Command::new("virsh")
        .args(["qemu-agent-command", vm_name, &status_json])
        .output()
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    let st_v: serde_json::Value = match serde_json::from_slice(&st.stdout) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(b64) = st_v
        .get("return")
        .and_then(|r| r.get("out-data"))
        .and_then(|d| d.as_str())
    else {
        return Vec::new();
    };
    let Some(decoded) = base64_decode(b64) else {
        return Vec::new();
    };
    parse_ss_output(&String::from_utf8_lossy(&decoded))
}

#[cfg(not(target_os = "linux"))]
pub fn scan_guest_listening_ports(_vm_name: &str) -> Vec<GuestListeningPort> {
    Vec::new()
}

pub fn guest_ports_to_open_ports(vm_name: &str, ports: &[GuestListeningPort]) -> Vec<OpenPort> {
    ports
        .iter()
        .map(|p| {
            let risk = if p.bind_address == "0.0.0.0" || p.bind_address == "::" {
                if p.port == 22 || p.port == 5432 || p.port == 3306 || p.port == 3389 {
                    ExposureRisk::Critical
                } else {
                    ExposureRisk::Warning
                }
            } else {
                ExposureRisk::Safe
            };
            OpenPort {
                port: p.port,
                protocol: p.protocol.clone(),
                service_name: p
                    .process
                    .clone()
                    .unwrap_or_else(|| format!("guest-{vm_name}")),
                bind_address: p.bind_address.clone(),
                process: p.process.clone(),
                allowed_from: vec![],
                risk,
                evidence: vec!["qemu-guest-agent:ss".into()],
            }
        })
        .collect()
}

fn parse_ss_output(text: &str) -> Vec<GuestListeningPort> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let local = parts[3];
        let (bind, port_str) = local.rsplit_once(':').unwrap_or(("*", local));
        let Ok(port) = port_str.parse::<u16>() else {
            continue;
        };
        let process = line.split("users:((").nth(1).map(|s| {
            s.trim_end_matches("))")
                .trim_matches('"')
                .split('"')
                .next()
                .unwrap_or("")
                .to_string()
        });
        out.push(GuestListeningPort {
            port,
            // Derive the family from the bind address: `ss` brackets/colon-separates
            // IPv6 hosts (e.g. "[::]"), while IPv4 binds ("0.0.0.0", "*") have no
            // colon (the port was already split off). parts[0] is the State column
            // ("LISTEN"), so the old `parts[0].contains('6')` was always false —
            // and both ternary branches returned "tcp" anyway.
            protocol: if bind.contains(':') { "tcp6" } else { "tcp" }.into(),
            bind_address: bind.into(),
            process,
        });
    }
    out
}

#[cfg(target_os = "linux")]
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .ok()
}
