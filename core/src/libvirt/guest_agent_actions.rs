// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use virt::connect::Connect;
use virt::sys;

use super::domain::lookup_domain;
use crate::LibvirtError;

#[cfg(target_os = "linux")]
use super::guest_agent::qemu_agent_command;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestUserSession {
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestTimeInfo {
    pub guest_time_rfc3339: String,
    pub host_time_rfc3339: String,
    pub delta_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsFreezeStatus {
    pub frozen: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestFstrimResult {
    pub mountpoint: String,
    pub trimmed_bytes: u64,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestServiceUnit {
    pub name: String,
    pub status: String,
    pub detail: String,
    /// When true, UI may offer start/stop/restart (unit is on the safe allowlist).
    #[serde(default)]
    pub controllable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestAgentActionResult {
    pub action: String,
    pub ok: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<GuestTimeInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fs_freeze: Option<FsFreezeStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fstrim: Vec<GuestFstrimResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<GuestServiceUnit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<GuestNetworkConfig>,
}

pub fn get_guest_users(vm_name: &str) -> Vec<GuestUserSession> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = vm_name;
        return Vec::new();
    }
    #[cfg(target_os = "linux")]
    {
        let Some(v) = qemu_agent_command(vm_name, r#"{"execute":"guest-get-users"}"#) else {
            return Vec::new();
        };
        let Some(arr) = v.get("return").and_then(|r| r.as_array()) else {
            return Vec::new();
        };
        arr.iter()
            .filter_map(|u| {
                let username = u.get("user")?.as_str()?.to_string();
                let login_time = u
                    .get("login-time")
                    .or_else(|| u.get("login_time"))
                    .and_then(|x| x.as_u64())
                    .map(|ts| {
                        DateTime::<Utc>::from_timestamp(ts as i64, 0)
                            .map(|d| d.to_rfc3339())
                            .unwrap_or_else(|| ts.to_string())
                    });
                let host = u
                    .get("domain")
                    .or_else(|| u.get("host"))
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                Some(GuestUserSession {
                    username,
                    login_time,
                    host,
                })
            })
            .collect()
    }
}

pub fn get_guest_time_info(conn: &Connect, name: &str) -> Result<GuestTimeInfo, LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    if !domain.is_active().unwrap_or(false) {
        return Err(LibvirtError::Invalid("VM must be running".into()));
    }
    let host_now = Utc::now();
    let (guest_secs, guest_nsecs) = domain
        .get_time(0)
        .map_err(|e| LibvirtError::Operation(format!("guest get_time: {e}")))?;
    let guest_dt = DateTime::<Utc>::from_timestamp(guest_secs, guest_nsecs as u32)
        .ok_or_else(|| LibvirtError::Operation("invalid guest time".into()))?;
    let delta_ms = (guest_dt - host_now).num_milliseconds();
    Ok(GuestTimeInfo {
        guest_time_rfc3339: guest_dt.to_rfc3339(),
        host_time_rfc3339: host_now.to_rfc3339(),
        delta_ms,
    })
}

pub fn sync_guest_time_to_host(conn: &Connect, name: &str) -> Result<GuestTimeInfo, LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    if !domain.is_active().unwrap_or(false) {
        return Err(LibvirtError::Invalid("VM must be running".into()));
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| LibvirtError::Internal(e.to_string()))?;
    let qga = domain.set_time(
        now.as_secs() as i64,
        now.subsec_nanos() as i32,
        sys::VIR_DOMAIN_TIME_SYNC,
    );
    if let Err(e) = qga {
        // guestkit / some QGA builds reject guest-set-time ("missing time"). Fall
        // back to guest-exec date -s so Sync still works from the UX.
        #[cfg(target_os = "linux")]
        {
            let secs = now.as_secs();
            let (code, _out, err) =
                guest_exec_command(name, "/bin/date", &["-u", "-s", &format!("@{secs}")])?;
            if code != 0 {
                return Err(LibvirtError::Operation(format!(
                    "guest set_time failed ({e}); date -s fallback failed: {err}"
                )));
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            return Err(LibvirtError::Operation(format!("guest set_time: {e}")));
        }
    }
    get_guest_time_info(conn, name).or_else(|_| {
        // get_time may also be incomplete; synthesize from host after successful set.
        let host_now = Utc::now();
        Ok(GuestTimeInfo {
            guest_time_rfc3339: host_now.to_rfc3339(),
            host_time_rfc3339: host_now.to_rfc3339(),
            delta_ms: 0,
        })
    })
}

pub fn get_fs_freeze_status(vm_name: &str) -> FsFreezeStatus {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = vm_name;
        return FsFreezeStatus {
            frozen: false,
            detail: "unavailable on this platform".into(),
        };
    }
    #[cfg(target_os = "linux")]
    {
        let Some(v) = qemu_agent_command(vm_name, r#"{"execute":"guest-fsfreeze-status"}"#) else {
            return FsFreezeStatus {
                frozen: false,
                detail: "guest-fsfreeze-status unavailable".into(),
            };
        };
        let ret = v.get("return").unwrap_or(&v);
        let frozen = ret.get("frozen").and_then(|x| x.as_bool()).unwrap_or(false);
        let detail = if frozen {
            "One or more guest filesystems are frozen".into()
        } else {
            "No frozen filesystems".into()
        };
        FsFreezeStatus { frozen, detail }
    }
}

pub fn run_guest_fstrim(vm_name: &str) -> Result<Vec<GuestFstrimResult>, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = vm_name;
        return Ok(Vec::new());
    }
    #[cfg(target_os = "linux")]
    {
        let Some(v) = qemu_agent_command(vm_name, r#"{"execute":"guest-fstrim"}"#) else {
            return Err(LibvirtError::Operation(
                "guest-fstrim failed — is guestkit-agent running?".into(),
            ));
        };
        if let Some(err) = v.get("error") {
            let msg = err
                .get("desc")
                .and_then(|x| x.as_str())
                .unwrap_or("guest-fstrim error");
            return Err(LibvirtError::Operation(msg.to_string()));
        }
        let Some(arr) = v.get("return").and_then(|r| r.as_array()) else {
            return Ok(Vec::new());
        };
        Ok(arr
            .iter()
            .map(|item| {
                let mountpoint = item
                    .get("path")
                    .or_else(|| item.get("mountpoint"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let trimmed_bytes = item
                    .get("minimum")
                    .or_else(|| item.get("trimmed"))
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0);
                let error = item
                    .get("error")
                    .and_then(|x| x.as_i64())
                    .filter(|&e| e != 0)
                    .map(|e| format!("error code {e}"))
                    .unwrap_or_default();
                GuestFstrimResult {
                    mountpoint,
                    trimmed_bytes,
                    error,
                }
            })
            .collect())
    }
}

/// Validate a systemd unit name for guest-exec systemctl actions.
pub fn validate_guest_service_unit(unit: &str) -> Result<String, LibvirtError> {
    let u = unit.trim();
    if u.is_empty() || u.len() > 128 {
        return Err(LibvirtError::Invalid(
            "invalid guest service unit name".into(),
        ));
    }
    if !u
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '@')
        || u.starts_with('-')
        || u.starts_with('.')
    {
        return Err(LibvirtError::Invalid(format!(
            "invalid guest service unit name: {u}"
        )));
    }
    let base = u.strip_suffix(".service").unwrap_or(u).to_ascii_lowercase();
    const BLOCKED: &[&str] = &[
        "sshd",
        "ssh",
        "networking",
        "network",
        "systemd-networkd",
        "NetworkManager",
        "networkmanager",
        "dbus",
        "systemd",
        "guestkit-agent",
        "zyvor-guest-agent",
        "qemu-guest-agent",
    ];
    if BLOCKED.iter().any(|b| base.eq_ignore_ascii_case(b)) {
        return Err(LibvirtError::Invalid(format!(
            "refusing to control protected guest unit: {u}"
        )));
    }
    let normalized = if u.ends_with(".service") {
        u.to_string()
    } else {
        format!("{u}.service")
    };
    Ok(normalized)
}

#[cfg(target_os = "linux")]
fn guest_exec_command(
    vm_name: &str,
    path: &str,
    args: &[&str],
) -> Result<(i32, String, String), LibvirtError> {
    use super::guest_agent::qemu_agent_command;
    use base64::Engine;

    if !vm_name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        || vm_name.is_empty()
        || vm_name.starts_with('-')
    {
        return Err(LibvirtError::Invalid("invalid VM name".into()));
    }
    if path.is_empty() || !path.starts_with('/') {
        return Err(LibvirtError::Invalid(
            "guest-exec path must be absolute".into(),
        ));
    }
    let exec_json = serde_json::json!({
        "execute": "guest-exec",
        "arguments": {
            "path": path,
            "arg": args,
            "capture-output": true,
        }
    })
    .to_string();
    // Netplan/NM apply can briefly drop the virtio serial channel; retry QGA
    // until the agent is reachable again rather than failing the whole apply.
    let v = {
        let mut last = None;
        for attempt in 0..8 {
            if let Some(v) = qemu_agent_command(vm_name, &exec_json) {
                last = Some(v);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250 + attempt * 150));
        }
        last.ok_or_else(|| {
            LibvirtError::Operation("guest-exec failed — is guestkit-agent running?".into())
        })?
    };
    let pid = v
        .get("return")
        .and_then(|r| r.get("pid"))
        .and_then(|p| p.as_u64())
        .ok_or_else(|| LibvirtError::Operation("guest-exec returned no pid".into()))?;
    let mut last = None;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let status_json =
            format!(r#"{{"execute":"guest-exec-status","arguments":{{"pid":{pid}}}}}"#);
        let st = {
            let mut got = None;
            for attempt in 0..6 {
                if let Some(v) = qemu_agent_command(vm_name, &status_json) {
                    got = Some(v);
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(200 + attempt * 100));
            }
            got.ok_or_else(|| LibvirtError::Operation("guest-exec-status failed".into()))?
        };
        let ret = st.get("return").cloned().unwrap_or(st);
        if ret.get("exited").and_then(|x| x.as_bool()).unwrap_or(false) {
            last = Some(ret);
            break;
        }
    }
    let ret = last.ok_or_else(|| LibvirtError::Operation("guest-exec timed out".into()))?;
    let code = ret.get("exitcode").and_then(|c| c.as_i64()).unwrap_or(-1) as i32;
    let decode = |key: &str| -> String {
        ret.get(key)
            .and_then(|d| d.as_str())
            .and_then(|b64| {
                base64::engine::general_purpose::STANDARD
                    .decode(b64.trim())
                    .ok()
            })
            .map(|b| String::from_utf8_lossy(&b).trim().to_string())
            .unwrap_or_default()
    };
    Ok((code, decode("out-data"), decode("err-data")))
}

/// In-guest GuestKit agent methods the host may relay through `guestkitctl`
/// (per-container eBPF network policy and LSM MAC).
pub const GUESTKIT_RELAY_METHODS: &[&str] = &[
    "guestkit.netpolicy.apply",
    "guestkit.netpolicy.status",
    "guestkit.lsm.apply",
    "guestkit.lsm.status",
];

const GUESTKITCTL_PATHS: &[&str] = &[
    "/usr/bin/guestkitctl",
    "/usr/local/bin/guestkitctl",
    "/usr/sbin/guestkitctl",
    "/opt/guestkit/bin/guestkitctl",
];

/// `guestkitctl` argv for one relayed call.
pub fn guestkitctl_args(
    method: &str,
    params: &serde_json::Value,
) -> Result<Vec<String>, LibvirtError> {
    if !GUESTKIT_RELAY_METHODS.contains(&method) {
        return Err(LibvirtError::Invalid(format!(
            "guestkit method not relayable: {method}"
        )));
    }
    if !params.is_object() {
        return Err(LibvirtError::Invalid(
            "guestkit params must be a JSON object".into(),
        ));
    }
    Ok(vec![
        "--json".into(),
        "call".into(),
        method.into(),
        "--params".into(),
        params.to_string(),
    ])
}

/// Result of `guestkitctl --json call`: JSON on stdout when it exits 0, an
/// `Error: …` line on stderr otherwise.
pub fn parse_guestkitctl_output(
    code: i32,
    out: &str,
    err: &str,
) -> Result<serde_json::Value, LibvirtError> {
    if code == 0 {
        return serde_json::from_str(out.trim()).map_err(|e| {
            LibvirtError::Operation(format!("guestkitctl returned non-JSON output: {e}"))
        });
    }
    let msg = err.trim().strip_prefix("Error: ").unwrap_or(err.trim());
    let msg = if msg.is_empty() {
        format!("guestkitctl exited with {code}")
    } else {
        msg.to_string()
    };
    if msg.contains("disabled by local policy") {
        return Err(LibvirtError::Forbidden(format!(
            "{msg} (set `capabilities: {{ ebpf: true }}` in /etc/guestkit/agent-policy.yaml inside the guest)"
        )));
    }
    if msg.contains("No such file") || (msg.contains("connect") && msg.contains("agent")) {
        return Err(LibvirtError::Operation(format!(
            "guestkitd is not reachable inside the guest: {msg}"
        )));
    }
    Err(LibvirtError::Operation(msg))
}

/// Invoke a GuestKit agent method inside the guest: QGA guest-exec of
/// `guestkitctl --json call <method> --params <json>` (the guest's own
/// agent then applies its local policy, e.g. the `ebpf` capability toggle).
pub fn guestkit_call(
    vm_name: &str,
    method: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, LibvirtError> {
    let args = guestkitctl_args(method, params)?;
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (vm_name, args);
        Err(LibvirtError::Operation(
            "guest-exec requires Linux libvirt".into(),
        ))
    }
    #[cfg(target_os = "linux")]
    {
        let mut found = None;
        for p in GUESTKITCTL_PATHS {
            let (code, _, _) = guest_exec_command(vm_name, "/bin/test", &["-x", p])?;
            if code == 0 {
                found = Some(*p);
                break;
            }
        }
        let bin = found.ok_or_else(|| {
            LibvirtError::Operation(
                "guestkitctl not found in the guest (install GuestKit tools: guestkitd + guestkitctl)".into(),
            )
        })?;
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, out, err) = guest_exec_command(vm_name, bin, &argv)?;
        parse_guestkitctl_output(code, &out, &err)
    }
}

#[cfg(target_os = "linux")]
fn guest_exec_systemctl(vm_name: &str, args: &[&str]) -> Result<(i32, String), LibvirtError> {
    let systemctl =
        guest_find_bin(vm_name, &["systemctl"]).unwrap_or_else(|| "/usr/bin/systemctl".into());
    let (code, out, _err) = guest_exec_command(vm_name, &systemctl, args)?;
    Ok((code, out))
}

fn validate_iface(iface: &str) -> Result<String, LibvirtError> {
    let i = iface.trim();
    if i.is_empty()
        || i.len() > 32
        || !i
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        || i.starts_with('-')
        || i == "lo"
    {
        return Err(LibvirtError::Invalid(format!("invalid interface: {iface}")));
    }
    Ok(i.to_string())
}

fn validate_cidr(cidr: &str) -> Result<String, LibvirtError> {
    let c = cidr.trim();
    // Minimal: a.b.c.d/nn
    let (ip, prefix) = c.split_once('/').ok_or_else(|| {
        LibvirtError::Invalid("address must be CIDR (e.g. 192.168.122.50/24)".into())
    })?;
    let pref: u8 = prefix
        .parse()
        .map_err(|_| LibvirtError::Invalid("invalid CIDR prefix".into()))?;
    if pref > 32 {
        return Err(LibvirtError::Invalid("CIDR prefix must be 0-32".into()));
    }
    let parts: Vec<_> = ip.split('.').collect();
    if parts.len() != 4 || parts.iter().any(|p| p.parse::<u8>().is_err()) {
        return Err(LibvirtError::Invalid("invalid IPv4 address".into()));
    }
    Ok(format!("{ip}/{pref}"))
}

fn validate_ipv4(ip: &str) -> Result<String, LibvirtError> {
    let i = ip.trim();
    let parts: Vec<_> = i.split('.').collect();
    if parts.len() != 4 || parts.iter().any(|p| p.parse::<u8>().is_err()) {
        return Err(LibvirtError::Invalid(format!("invalid IPv4: {ip}")));
    }
    Ok(i.to_string())
}

fn validate_route_dest(to: &str) -> Result<String, LibvirtError> {
    let t = to.trim().to_ascii_lowercase();
    if t == "default" || t == "0.0.0.0/0" {
        return Ok("default".into());
    }
    validate_cidr(to)
}

fn validate_dns_list(dns: &[String]) -> Result<Vec<String>, LibvirtError> {
    let mut out = Vec::new();
    for d in dns {
        let t = d.trim();
        if t.is_empty() {
            continue;
        }
        out.push(validate_ipv4(t)?);
        if out.len() > 8 {
            return Err(LibvirtError::Invalid("too many DNS servers (max 8)".into()));
        }
    }
    Ok(out)
}

fn validate_static_routes(
    routes: &[GuestStaticRoute],
) -> Result<Vec<(String, String)>, LibvirtError> {
    let mut out = Vec::new();
    for r in routes {
        let to = validate_route_dest(&r.to)?;
        let via = validate_ipv4(&r.via)?;
        if to == "default" {
            continue; // default is handled via gateway field
        }
        out.push((to, via));
        if out.len() > 16 {
            return Err(LibvirtError::Invalid(
                "too many static routes (max 16)".into(),
            ));
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestNetworkInterface {
    pub name: String,
    pub mac: Option<String>,
    pub addresses: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestNetworkConfig {
    pub interfaces: Vec<GuestNetworkInterface>,
    pub routes: Vec<String>,
    pub default_gateway: Option<String>,
    /// Detected guest stack: `networkmanager` | `systemd-networkd` | `netplan` | `wicked` | `iproute2` | `unknown`
    #[serde(default)]
    pub backend: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub backend_detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestStaticRoute {
    /// Destination CIDR (e.g. `10.0.0.0/8`) or `default`.
    pub to: String,
    /// Next hop IPv4.
    pub via: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestNetworkApplyRequest {
    pub iface: String,
    /// IPv4 CIDR e.g. 192.168.122.50/24
    pub address_cidr: String,
    #[serde(default)]
    pub gateway: Option<String>,
    /// When true, replace existing IPv4 config on the iface (flush / method=manual overwrite).
    #[serde(default)]
    pub replace: bool,
    /// DNS nameservers (IPv4). Applied when the guest stack supports it (NM/netplan/networkd).
    #[serde(default)]
    pub dns: Vec<String>,
    /// Extra static routes (in addition to the default gateway).
    #[serde(default)]
    pub routes: Vec<GuestStaticRoute>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuestNetBackend {
    NetworkManager,
    SystemdNetworkd,
    Netplan,
    Wicked,
    Iproute2,
}

impl GuestNetBackend {
    fn as_str(self) -> &'static str {
        match self {
            Self::NetworkManager => "networkmanager",
            Self::SystemdNetworkd => "systemd-networkd",
            Self::Netplan => "netplan",
            Self::Wicked => "wicked",
            Self::Iproute2 => "iproute2",
        }
    }
}

#[cfg(target_os = "linux")]
fn guest_bin_exists(vm_name: &str, path: &str) -> bool {
    guest_exec_command(vm_name, "/bin/test", &["-x", path])
        .map(|(c, _, _)| c == 0)
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn guest_find_bin(vm_name: &str, names: &[&str]) -> Option<String> {
    let prefixes = ["/usr/sbin", "/sbin", "/usr/bin", "/bin"];
    for name in names {
        for pref in prefixes {
            let p = format!("{pref}/{name}");
            if guest_bin_exists(vm_name, &p) {
                return Some(p);
            }
        }
    }
    None
}

/// Prefer the stack that actually owns guest networking on this distro.
#[cfg(target_os = "linux")]
fn detect_guest_net_backend(vm_name: &str) -> (GuestNetBackend, String) {
    // Single guest-exec probe — avoids N round-trips for systemctl/which.
    // No `set -e`: failed is-active checks must not abort before we print a tag.
    let probe = r#"
PATH=/usr/sbin:/sbin:/usr/bin:/bin:$PATH
has() { command -v "$1" >/dev/null 2>&1; }
active() {
  local u="$1"
  systemctl is-active --quiet "$u" 2>/dev/null && return 0
  /bin/systemctl is-active --quiet "$u" 2>/dev/null && return 0
  /usr/bin/systemctl is-active --quiet "$u" 2>/dev/null && return 0
  return 1
}
if active NetworkManager || active NetworkManager.service; then
  if has nmcli; then echo NM; exit 0; fi
fi
NP=0; has netplan && NP=1
ND=0
if active systemd-networkd || active systemd-networkd.service; then ND=1; fi
if [ "$NP" = 1 ] && [ "$ND" = 1 ]; then echo NETPLAN; exit 0; fi
if [ "$ND" = 1 ]; then echo NETWORKD; exit 0; fi
if [ "$NP" = 1 ]; then echo NETPLAN; exit 0; fi
if active wicked || active wickedd || has wicked; then echo WICKED; exit 0; fi
if has ip; then echo IP; exit 0; fi
echo IP
"#;
    let (code, out, _err) = guest_exec_command(vm_name, "/bin/sh", &["-c", probe]).unwrap_or((
        1,
        String::new(),
        String::new(),
    ));
    let tag = if code == 0 {
        out.lines().next().unwrap_or("IP").trim()
    } else {
        "IP"
    };
    match tag {
        "NM" => (
            GuestNetBackend::NetworkManager,
            "NetworkManager is active (nmcli)".into(),
        ),
        "NETWORKD" => (
            GuestNetBackend::SystemdNetworkd,
            "systemd-networkd is active".into(),
        ),
        "NETPLAN" => (GuestNetBackend::Netplan, "netplan detected".into()),
        "WICKED" => (
            GuestNetBackend::Wicked,
            "wicked network service detected".into(),
        ),
        _ => (
            GuestNetBackend::Iproute2,
            "no NM/networkd/netplan/wicked — using iproute2".into(),
        ),
    }
}

#[cfg(target_os = "linux")]
fn guest_ip_bin(vm_name: &str) -> String {
    guest_find_bin(vm_name, &["ip"]).unwrap_or_else(|| "/sbin/ip".into())
}

#[cfg(target_os = "linux")]
fn apply_extra_ip_routes(
    vm_name: &str,
    iface: &str,
    routes: &[(String, String)],
) -> Result<(), LibvirtError> {
    if routes.is_empty() {
        return Ok(());
    }
    let ip = guest_ip_bin(vm_name);
    for (to, via) in routes {
        let (code, _out, err) = guest_exec_command(
            vm_name,
            &ip,
            &["route", "replace", to, "via", via, "dev", iface],
        )?;
        if code != 0 {
            return Err(LibvirtError::Operation(format!(
                "ip route replace {to} via {via} failed: {err}"
            )));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn apply_via_networkmanager(
    vm_name: &str,
    iface: &str,
    cidr: &str,
    gw: Option<&str>,
    dns: &[String],
    routes: &[(String, String)],
) -> Result<String, LibvirtError> {
    let nmcli = guest_find_bin(vm_name, &["nmcli"])
        .ok_or_else(|| LibvirtError::Operation("nmcli not found in guest".into()))?;
    // Resolve active connection for this device.
    let (code, out, err) = guest_exec_command(
        vm_name,
        &nmcli,
        &["-t", "-f", "NAME,DEVICE", "connection", "show", "--active"],
    )?;
    if code != 0 {
        return Err(LibvirtError::Operation(format!(
            "nmcli con show failed: {err}"
        )));
    }
    let mut con = out
        .lines()
        .filter_map(|l| {
            let (name, dev) = l.split_once(':')?;
            if dev == iface {
                Some(name.to_string())
            } else {
                None
            }
        })
        .next();
    if con.is_none() {
        // Create a dedicated connection profile for this NIC.
        let name = format!("machina-{iface}");
        let (c, _, e) = guest_exec_command(
            vm_name,
            &nmcli,
            &[
                "connection",
                "add",
                "type",
                "ethernet",
                "ifname",
                iface,
                "con-name",
                &name,
            ],
        )?;
        if c != 0 {
            return Err(LibvirtError::Operation(format!(
                "nmcli connection add failed: {e}"
            )));
        }
        con = Some(name);
    }
    let con = con.unwrap();
    let mut args = vec![
        "connection".into(),
        "modify".into(),
        con.clone(),
        "ipv4.method".into(),
        "manual".into(),
        "ipv4.addresses".into(),
        cidr.to_string(),
    ];
    if let Some(g) = gw {
        args.push("ipv4.gateway".into());
        args.push(g.to_string());
    } else {
        args.push("ipv4.gateway".into());
        args.push("".into());
    }
    if !dns.is_empty() {
        args.push("ipv4.dns".into());
        args.push(dns.join(" "));
        args.push("ipv4.ignore-auto-dns".into());
        args.push("yes".into());
    }
    if !routes.is_empty() {
        // nmcli wants "dest/prefix via gateway" space-separated entries.
        let route_str = routes
            .iter()
            .map(|(to, via)| format!("{to} {via}"))
            .collect::<Vec<_>>()
            .join(",");
        args.push("ipv4.routes".into());
        args.push(route_str);
    }
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (c, _, e) = guest_exec_command(vm_name, &nmcli, &arg_refs)?;
    if c != 0 {
        return Err(LibvirtError::Operation(format!(
            "nmcli connection modify failed: {e}"
        )));
    }
    let (c, _, e) = guest_exec_command(vm_name, &nmcli, &["connection", "up", &con])?;
    if c != 0 {
        // device reapply is a softer fallback
        let (c2, _, e2) = guest_exec_command(vm_name, &nmcli, &["device", "reapply", iface])?;
        if c2 != 0 {
            return Err(LibvirtError::Operation(format!(
                "nmcli connection up failed: {e}; reapply: {e2}"
            )));
        }
    }
    Ok(format!("NetworkManager profile `{con}` (persistent)"))
}

#[cfg(target_os = "linux")]
fn apply_via_systemd_networkd(
    vm_name: &str,
    iface: &str,
    cidr: &str,
    gw: Option<&str>,
    dns: &[String],
    routes: &[(String, String)],
) -> Result<String, LibvirtError> {
    // Persist under /etc so config survives reboot (unlike a /run drop-in).
    let mut net_body = String::from("[Network]\nDHCP=no\n");
    net_body.push_str(&format!("Address={cidr}\n"));
    if let Some(g) = gw {
        net_body.push_str(&format!("Gateway={g}\n"));
    }
    for d in dns {
        net_body.push_str(&format!("DNS={d}\n"));
    }
    let mut body = format!("[Match]\nName={iface}\n\n{net_body}");
    for (to, via) in routes {
        body.push_str(&format!("\n[Route]\nDestination={to}\nGateway={via}\n"));
    }
    let body_escaped = body.replace('\'', "'\\''");
    let path = format!("/etc/systemd/network/10-machina-{iface}.network");
    let script = format!(
        "mkdir -p /etc/systemd/network && printf '%s' '{body_escaped}' > '{path}' && \
         networkctl reload && networkctl reconfigure '{iface}'",
    );
    let (c, out, err) = guest_exec_command(vm_name, "/bin/sh", &["-c", &script])?;
    if c != 0 {
        return Err(LibvirtError::Operation(format!(
            "systemd-networkd apply failed (exit {c}): {err} {out}"
        )));
    }
    Ok(format!("systemd-networkd persistent unit {path}"))
}

#[cfg(target_os = "linux")]
fn apply_via_netplan(
    vm_name: &str,
    iface: &str,
    cidr: &str,
    gw: Option<&str>,
    dns: &[String],
    routes: &[(String, String)],
) -> Result<String, LibvirtError> {
    let netplan = guest_find_bin(vm_name, &["netplan"])
        .ok_or_else(|| LibvirtError::Operation("netplan not found in guest".into()))?;
    let mut routes_yaml = String::new();
    if gw.is_some() || !routes.is_empty() {
        routes_yaml.push_str("      routes:\n");
        if let Some(g) = gw {
            routes_yaml.push_str(&format!("        - to: default\n          via: {g}\n"));
        }
        for (to, via) in routes {
            routes_yaml.push_str(&format!("        - to: {to}\n          via: {via}\n"));
        }
    }
    let dns_yaml = if dns.is_empty() {
        String::new()
    } else {
        let mut s = String::from("      nameservers:\n        addresses:\n");
        for d in dns {
            s.push_str(&format!("          - {d}\n"));
        }
        s
    };
    let yaml = format!(
        "network:\n  version: 2\n  ethernets:\n    {iface}:\n      dhcp4: false\n      addresses:\n        - {cidr}\n{dns_yaml}{routes_yaml}"
    );
    let yaml_esc = yaml.replace('\'', "'\\''");
    let path = "/etc/netplan/99-machina-guest.yaml";
    let script = format!("printf '%s' '{yaml_esc}' > '{path}' && '{netplan}' apply",);
    let (c, out, err) = guest_exec_command(vm_name, "/bin/sh", &["-c", &script])?;
    if c != 0 {
        return Err(LibvirtError::Operation(format!(
            "netplan apply failed (exit {c}): {err} {out}"
        )));
    }
    Ok(format!("netplan {path} (persistent)"))
}

#[cfg(target_os = "linux")]
fn apply_via_wicked(
    vm_name: &str,
    iface: &str,
    cidr: &str,
    gw: Option<&str>,
    dns: &[String],
    routes: &[(String, String)],
) -> Result<String, LibvirtError> {
    // wicked ifconfig is awkward; prefer writing ifcfg + wicked ifup, else fall through to ip.
    let wicked = guest_find_bin(vm_name, &["wicked"]);
    let (ip_part, prefix) = cidr
        .split_once('/')
        .ok_or_else(|| LibvirtError::Invalid("bad cidr".into()))?;
    let mut ifcfg =
        format!("BOOTPROTO='static'\nSTARTMODE='auto'\nIPADDR='{ip_part}'\nPREFIXLEN='{prefix}'\n");
    if let Some(g) = gw {
        ifcfg.push_str(&format!("DEFAULT_ROUTE='yes'\nGATEWAY='{g}'\n"));
    }
    if !dns.is_empty() {
        ifcfg.push_str(&format!(
            "NETCONFIG_DNS_STATIC_SERVERS='{}'\n",
            dns.join(" ")
        ));
    }
    let ifcfg_esc = ifcfg.replace('\'', "'\\''");
    let path = format!("/etc/sysconfig/network/ifcfg-{iface}");
    let mut script = format!("printf '%s' '{ifcfg_esc}' > '{path}'");
    if let Some(ref w) = wicked {
        script.push_str(&format!(" && '{w}' ifup '{iface}'"));
    }
    let (c, out, err) = guest_exec_command(vm_name, "/bin/sh", &["-c", &script])?;
    if c != 0 {
        return Err(LibvirtError::Operation(format!(
            "wicked/ifcfg apply failed (exit {c}): {err} {out}"
        )));
    }
    apply_extra_ip_routes(vm_name, iface, routes)?;
    Ok(format!("wicked ifcfg-{iface} (persistent)"))
}

#[cfg(target_os = "linux")]
fn apply_via_iproute2(
    vm_name: &str,
    iface: &str,
    cidr: &str,
    gw: Option<&str>,
    replace: bool,
    dns: &[String],
    routes: &[(String, String)],
) -> Result<String, LibvirtError> {
    let ip = guest_ip_bin(vm_name);
    if replace {
        let (code, _out, err) =
            guest_exec_command(vm_name, &ip, &["-4", "addr", "flush", "dev", iface])?;
        if code != 0 {
            return Err(LibvirtError::Operation(format!(
                "ip addr flush {iface} failed: {err}"
            )));
        }
    }
    let (code, _out, err) = guest_exec_command(vm_name, &ip, &["addr", "add", cidr, "dev", iface])?;
    if code != 0 && !(code == 2 && !replace) {
        return Err(LibvirtError::Operation(format!(
            "ip addr add {cidr} dev {iface} failed (exit {code}): {err}"
        )));
    }
    let _ = guest_exec_command(vm_name, &ip, &["link", "set", iface, "up"]);
    if let Some(gateway) = gw {
        let (code, _out, err) = guest_exec_command(
            vm_name,
            &ip,
            &["route", "replace", "default", "via", gateway, "dev", iface],
        )?;
        if code != 0 {
            return Err(LibvirtError::Operation(format!(
                "ip route replace default via {gateway} failed: {err}"
            )));
        }
    }
    apply_extra_ip_routes(vm_name, iface, routes)?;
    if !dns.is_empty() {
        // Best-effort runtime resolv.conf — not managed by a persistent stack.
        let content = dns
            .iter()
            .map(|d| format!("nameserver {d}\n"))
            .collect::<String>();
        let esc = content.replace('\'', "'\\''");
        let script = format!("printf '%s' '{esc}' > /etc/resolv.conf");
        let _ = guest_exec_command(vm_name, "/bin/sh", &["-c", &script]);
    }
    Ok("iproute2 (runtime — reboot may clear addresses/routes)".into())
}

/// Read guest NICs (QGA) + IPv4 routes (guest-exec ip) + detected network backend.
pub fn get_guest_network_config(vm_name: &str) -> Result<GuestNetworkConfig, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = vm_name;
        return Err(LibvirtError::Invalid(
            "guest network requires Linux hypervisor".into(),
        ));
    }
    #[cfg(target_os = "linux")]
    {
        use super::guest_agent::qemu_agent_command;
        let (backend, backend_detail) = detect_guest_net_backend(vm_name);
        let mut interfaces = Vec::new();
        if let Some(v) =
            qemu_agent_command(vm_name, r#"{"execute":"guest-network-get-interfaces"}"#)
        {
            if let Some(arr) = v.get("return").and_then(|r| r.as_array()) {
                for iface in arr {
                    let name = iface
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    if name.is_empty() || name == "lo" {
                        continue;
                    }
                    let mac = iface
                        .get("hardware-address")
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string());
                    let mut addresses = Vec::new();
                    if let Some(ips) = iface.get("ip-addresses").and_then(|a| a.as_array()) {
                        for ip in ips {
                            let typ = ip
                                .get("ip-address-type")
                                .and_then(|t| t.as_str())
                                .unwrap_or("");
                            let addr = ip.get("ip-address").and_then(|a| a.as_str()).unwrap_or("");
                            let prefix = ip.get("prefix").and_then(|p| p.as_u64()).unwrap_or(0);
                            if typ == "ipv4" && !addr.is_empty() {
                                addresses.push(format!("{addr}/{prefix}"));
                            }
                        }
                    }
                    interfaces.push(GuestNetworkInterface {
                        name,
                        mac,
                        addresses,
                    });
                }
            }
        }
        let ip = guest_ip_bin(vm_name);
        if let Ok((0, addr_out, _)) =
            guest_exec_command(vm_name, &ip, &["-4", "-o", "addr", "show"])
        {
            for line in addr_out.lines() {
                let parts: Vec<_> = line.split_whitespace().collect();
                if parts.len() < 4 || parts[2] != "inet" {
                    continue;
                }
                let name = parts[1];
                let cidr = parts[3];
                if let Some(iface) = interfaces.iter_mut().find(|i| i.name == name) {
                    if !iface.addresses.iter().any(|a| a == cidr) {
                        iface.addresses.push(cidr.to_string());
                    }
                } else if name != "lo" {
                    interfaces.push(GuestNetworkInterface {
                        name: name.to_string(),
                        mac: None,
                        addresses: vec![cidr.to_string()],
                    });
                }
            }
        }
        let mut routes = Vec::new();
        let mut default_gateway = None;
        if let Ok((0, route_out, _)) = guest_exec_command(vm_name, &ip, &["-4", "route", "show"]) {
            for line in route_out.lines() {
                let l = line.trim();
                if l.is_empty() {
                    continue;
                }
                routes.push(l.to_string());
                if l.starts_with("default via ") {
                    default_gateway = l.split_whitespace().nth(2).map(|s| s.to_string());
                }
            }
        }
        Ok(GuestNetworkConfig {
            interfaces,
            routes,
            default_gateway,
            backend: backend.as_str().into(),
            backend_detail,
        })
    }
}

/// Apply IPv4 address (+ optional default gateway, DNS, static routes) using the guest's native network stack.
pub fn apply_guest_network_config(
    vm_name: &str,
    req: &GuestNetworkApplyRequest,
) -> Result<GuestAgentActionResult, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (vm_name, req);
        return Err(LibvirtError::Invalid(
            "guest network requires Linux hypervisor".into(),
        ));
    }
    #[cfg(target_os = "linux")]
    {
        let iface = validate_iface(&req.iface)?;
        let cidr = validate_cidr(&req.address_cidr)?;
        let gw = match &req.gateway {
            Some(g) if !g.trim().is_empty() => Some(validate_ipv4(g)?),
            _ => None,
        };
        let dns = validate_dns_list(&req.dns)?;
        let routes = validate_static_routes(&req.routes)?;
        let (backend, detect_detail) = detect_guest_net_backend(vm_name);
        let gw_ref = gw.as_deref();

        let mut used = match backend {
            GuestNetBackend::NetworkManager => {
                apply_via_networkmanager(vm_name, &iface, &cidr, gw_ref, &dns, &routes)
            }
            GuestNetBackend::SystemdNetworkd => {
                apply_via_systemd_networkd(vm_name, &iface, &cidr, gw_ref, &dns, &routes)
            }
            GuestNetBackend::Netplan => {
                apply_via_netplan(vm_name, &iface, &cidr, gw_ref, &dns, &routes)
            }
            GuestNetBackend::Wicked => {
                apply_via_wicked(vm_name, &iface, &cidr, gw_ref, &dns, &routes)
            }
            GuestNetBackend::Iproute2 => {
                apply_via_iproute2(vm_name, &iface, &cidr, gw_ref, req.replace, &dns, &routes)
            }
        };

        // If the preferred stack failed, fall through to iproute2 so the op still works.
        if used.is_err() && backend != GuestNetBackend::Iproute2 {
            let primary_err = used
                .as_ref()
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            used = apply_via_iproute2(vm_name, &iface, &cidr, gw_ref, req.replace, &dns, &routes)
                .map(|s| format!("{s} (fallback after {}: {primary_err})", backend.as_str()));
        }

        let how = used?;
        let cfg = get_guest_network_config(vm_name).ok();
        let extras = {
            let mut bits = Vec::new();
            if !dns.is_empty() {
                bits.push(format!("dns {}", dns.join(",")));
            }
            if !routes.is_empty() {
                bits.push(format!("{} static route(s)", routes.len()));
            }
            if bits.is_empty() {
                String::new()
            } else {
                format!(" · {}", bits.join(" · "))
            }
        };
        Ok(GuestAgentActionResult {
            action: "network_apply".into(),
            ok: true,
            message: format!(
                "{iface} → {cidr}{} via {how}{extras} · {detect_detail}",
                gw.as_ref().map(|g| format!(" gw {g}")).unwrap_or_default()
            ),
            time: None,
            fs_freeze: None,
            fstrim: Vec::new(),
            services: Vec::new(),
            network: cfg,
        })
    }
}

/// Start / stop / restart a guest systemd unit via guestkit-agent (QGA guest-exec).
pub fn run_guest_service_action(
    vm_name: &str,
    unit: &str,
    action: &str,
) -> Result<GuestAgentActionResult, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (vm_name, unit, action);
        return Err(LibvirtError::Invalid(
            "guest service actions require Linux hypervisor".into(),
        ));
    }
    #[cfg(target_os = "linux")]
    {
        let unit = validate_guest_service_unit(unit)?;
        let act = action.trim().to_ascii_lowercase();
        if !matches!(act.as_str(), "start" | "stop" | "restart") {
            return Err(LibvirtError::Invalid(format!(
                "unsupported guest service action: {action}"
            )));
        }
        let (code, out) = guest_exec_systemctl(vm_name, &[act.as_str(), &unit])?;
        if code != 0 {
            return Err(LibvirtError::Operation(format!(
                "systemctl {act} {unit} failed (exit {code}): {out}"
            )));
        }
        let (st_code, status) = guest_exec_systemctl(vm_name, &["is-active", &unit])?;
        let active = st_code == 0 && status.trim() == "active";
        Ok(GuestAgentActionResult {
            action: format!("service_{act}"),
            ok: true,
            message: format!(
                "{unit} {act} ok · is-active={}",
                if active { "active" } else { status.trim() }
            ),
            time: None,
            fs_freeze: None,
            fstrim: Vec::new(),
            services: Vec::new(),
            network: None,
        })
    }
}

/// Inventory of guest systemd services: running units + controllable allowlist.
/// Protected units (ssh/network/agent) appear as read-only when running.
pub fn list_guest_service_units(vm_name: &str) -> Vec<GuestServiceUnit> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = vm_name;
        return Vec::new();
    }
    #[cfg(target_os = "linux")]
    {
        const CONTROLLABLE: &[&str] = &[
            "machina-demo.service",
            "cron.service",
            "cronie.service",
            "atd.service",
            "rsyslog.service",
            "nginx.service",
            "httpd.service",
            "apache2.service",
            "redis.service",
            "redis-server.service",
            "postgresql.service",
            "mysqld.service",
            "mariadb.service",
            "docker.service",
            "containerd.service",
            "chronyd.service",
            "ntpd.service",
            "fail2ban.service",
            "cups.service",
            "snapd.service",
            "unattended-upgrades.service",
        ];

        fn unit_base(unit: &str) -> String {
            unit.strip_suffix(".service")
                .unwrap_or(unit)
                .to_ascii_lowercase()
        }

        fn is_controllable(unit: &str) -> bool {
            if validate_guest_service_unit(unit).is_err() {
                return false;
            }
            let full = if unit.ends_with(".service") {
                unit.to_string()
            } else {
                format!("{unit}.service")
            };
            CONTROLLABLE.iter().any(|c| c.eq_ignore_ascii_case(&full))
        }

        let mut by_name: std::collections::BTreeMap<String, GuestServiceUnit> =
            std::collections::BTreeMap::new();

        // One guest-exec: list running units (inventory).
        if let Ok((0, out)) = guest_exec_systemctl(
            vm_name,
            &[
                "list-units",
                "--type=service",
                "--state=running",
                "--no-legend",
                "--no-pager",
                "--plain",
            ],
        ) {
            for line in out.lines().take(60) {
                let unit = line.split_whitespace().next().unwrap_or("").trim();
                if unit.is_empty() || !unit.ends_with(".service") {
                    continue;
                }
                // Skip template instances with weird chars beyond our validator charset except @.
                if unit.chars().any(|c| {
                    !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '@')
                }) {
                    continue;
                }
                let name = unit_base(unit);
                let controllable = is_controllable(unit);
                by_name.insert(
                    name.clone(),
                    GuestServiceUnit {
                        name,
                        status: "active".into(),
                        detail: if controllable {
                            format!("systemd · {unit} · controllable")
                        } else {
                            format!("systemd · {unit}")
                        },
                        controllable,
                    },
                );
            }
        }

        // Ensure allowlisted units appear even when inactive.
        for unit in CONTROLLABLE {
            if validate_guest_service_unit(unit).is_err() {
                continue;
            }
            let name = unit_base(unit);
            if by_name.contains_key(&name) {
                continue;
            }
            match guest_exec_systemctl(vm_name, &["is-active", unit]) {
                Ok((code, status)) => {
                    let st = status.trim();
                    if code == 4 || st == "not-found" || st.contains("could not be found") {
                        continue;
                    }
                    let active = code == 0 && st == "active";
                    by_name.insert(
                        name.clone(),
                        GuestServiceUnit {
                            name,
                            status: if active {
                                "active".into()
                            } else if st.is_empty() {
                                "unknown".into()
                            } else {
                                st.to_string()
                            },
                            detail: format!("systemd · {unit} · controllable"),
                            controllable: true,
                        },
                    );
                }
                Err(_) => continue,
            }
        }

        by_name.into_values().collect()
    }
}

pub fn run_guest_agent_action(
    conn: &Connect,
    name: &str,
    action: &str,
) -> Result<GuestAgentActionResult, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (conn, name, action);
        return Err(LibvirtError::Invalid(
            "guest agent actions require Linux hypervisor".into(),
        ));
    }
    #[cfg(target_os = "linux")]
    {
        let trimmed = action.trim();
        if let Some(rest) = trimmed.strip_prefix("service_start:") {
            return run_guest_service_action(name, rest, "start");
        }
        if let Some(rest) = trimmed.strip_prefix("service_stop:") {
            return run_guest_service_action(name, rest, "stop");
        }
        if let Some(rest) = trimmed.strip_prefix("service_restart:") {
            return run_guest_service_action(name, rest, "restart");
        }
        if let Some(rest) = trimmed.strip_prefix("network_apply:") {
            let req: GuestNetworkApplyRequest = serde_json::from_str(rest)
                .map_err(|e| LibvirtError::Invalid(format!("network_apply JSON: {e}")))?;
            return apply_guest_network_config(name, &req);
        }
        match trimmed.to_ascii_lowercase().as_str() {
            "sync_time" | "sync-time" => {
                let time = sync_guest_time_to_host(conn, name)?;
                Ok(GuestAgentActionResult {
                    action: "sync_time".into(),
                    ok: true,
                    message: format!("Guest time synced (delta was {} ms)", time.delta_ms),
                    time: Some(time),
                    fs_freeze: None,
                    fstrim: Vec::new(),
                    services: Vec::new(),
                    network: None,
                })
            }
            "fs_freeze_status" | "fs-freeze-status" => {
                let fs = get_fs_freeze_status(name);
                Ok(GuestAgentActionResult {
                    action: "fs_freeze_status".into(),
                    ok: true,
                    message: fs.detail.clone(),
                    time: None,
                    fs_freeze: Some(fs),
                    fstrim: Vec::new(),
                    services: Vec::new(),
                    network: None,
                })
            }
            "fstrim" | "trim" => {
                let fstrim = run_guest_fstrim(name)?;
                let total: u64 = fstrim.iter().map(|r| r.trimmed_bytes).sum();
                Ok(GuestAgentActionResult {
                    action: "fstrim".into(),
                    ok: true,
                    message: format!(
                        "TRIM on {} mount(s), {} bytes reported",
                        fstrim.len(),
                        total
                    ),
                    time: None,
                    fs_freeze: None,
                    fstrim,
                    services: Vec::new(),
                    network: None,
                })
            }
            "list_services" => {
                let services = list_guest_service_units(name);
                let controllable_n = services.iter().filter(|s| s.controllable).count();
                Ok(GuestAgentActionResult {
                    action: "list_services".into(),
                    ok: true,
                    message: format!(
                        "{} unit(s) · {} controllable",
                        services.len(),
                        controllable_n
                    ),
                    time: None,
                    fs_freeze: None,
                    fstrim: Vec::new(),
                    services,
                    network: None,
                })
            }
            "get_network" | "network" => {
                let network = get_guest_network_config(name)?;
                Ok(GuestAgentActionResult {
                    action: "get_network".into(),
                    ok: true,
                    message: format!(
                        "{} iface(s), {} route(s)",
                        network.interfaces.len(),
                        network.routes.len()
                    ),
                    time: None,
                    fs_freeze: None,
                    fstrim: Vec::new(),
                    services: Vec::new(),
                    network: Some(network),
                })
            }
            other => Err(LibvirtError::Invalid(format!(
                "unknown guest agent action: {other}"
            ))),
        }
    }
}

#[cfg(test)]
mod guestkit_relay_tests {
    use super::*;

    #[test]
    fn only_policy_methods_are_relayed() {
        let p = serde_json::json!({ "container": "web" });
        let a = guestkitctl_args("guestkit.netpolicy.apply", &p).unwrap();
        assert_eq!(
            a[..4],
            ["--json", "call", "guestkit.netpolicy.apply", "--params"]
        );
        assert_eq!(serde_json::from_str::<serde_json::Value>(&a[4]).unwrap(), p);
        assert!(guestkitctl_args("guestkit.exec", &p).is_err());
        assert!(guestkitctl_args("guestkit.fileWrite", &p).is_err());
        assert!(guestkitctl_args("guestkit.lsm.status", &serde_json::json!([1])).is_err());
    }

    #[test]
    fn parses_guestkitctl_results() {
        let v = parse_guestkitctl_output(0, r#"{"available": true}"#, "").unwrap();
        assert_eq!(v["available"], true);
        assert!(parse_guestkitctl_output(0, "not json", "").is_err());
        let e = parse_guestkitctl_output(
            1,
            "",
            "Error: agent RPC error -32005: Policy denied: ebpf disabled by local policy",
        )
        .unwrap_err();
        assert!(matches!(e, LibvirtError::Forbidden(ref m) if m.contains("ebpf: true")));
        let e = parse_guestkitctl_output(
            1,
            "",
            "Error: agent RPC error -32602: lease_secs must be 1..=3600",
        )
        .unwrap_err();
        assert!(
            matches!(e, LibvirtError::Operation(ref m) if m.starts_with("agent RPC error -32602"))
        );
        let e = parse_guestkitctl_output(2, "", "").unwrap_err();
        assert!(e.to_string().contains("exited with 2"));
    }
}
