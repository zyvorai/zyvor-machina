// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Live BMC power control for bare-metal servers: Redfish, IPMI (`ipmitool`) and a
//! `fake` driver that touches no hardware (tests, labs, demos).
//!
//! The controller talks to the BMC directly, so power works while the host is dead.
//! Nothing here persists anything; `engine::baremetal::set_power` owns the database.

use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

/// Power action, as accepted by `POST /api/v1/baremetal/servers/{id}/power`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    On,
    Off,
    Cycle,
    Reset,
}

impl PowerAction {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "on" => Ok(Self::On),
            "off" => Ok(Self::Off),
            "cycle" => Ok(Self::Cycle),
            "reset" => Ok(Self::Reset),
            _ => anyhow::bail!("action must be on, off, cycle, or reset"),
        }
    }

    /// Redfish `ResetType` (DMTF DSP2046).
    pub fn redfish_reset_type(self) -> &'static str {
        match self {
            Self::On => "On",
            Self::Off => "ForceOff",
            Self::Cycle => "PowerCycle",
            Self::Reset => "ForceRestart",
        }
    }

    /// `ipmitool power <verb>`.
    pub fn ipmi_verb(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Cycle => "cycle",
            Self::Reset => "reset",
        }
    }
}

/// What the BMC says, not what we last asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerState {
    On,
    Off,
    Unknown,
}

impl PowerState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Unknown => "unknown",
        }
    }
}

/// Which device the next boot should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootDevice {
    Pxe,
    Disk,
    Cdrom,
}

impl BootDevice {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pxe" | "network" => Ok(Self::Pxe),
            "disk" | "hdd" => Ok(Self::Disk),
            "cdrom" | "cd" | "dvd" => Ok(Self::Cdrom),
            other => anyhow::bail!("device must be pxe, disk or cdrom (got '{other}')"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pxe => "pxe",
            Self::Disk => "disk",
            Self::Cdrom => "cdrom",
        }
    }

    /// Redfish `BootSourceOverrideTarget`.
    pub fn redfish_target(self) -> &'static str {
        match self {
            Self::Pxe => "Pxe",
            Self::Disk => "Hdd",
            Self::Cdrom => "Cd",
        }
    }

    /// `ipmitool chassis bootdev` argument.
    pub fn ipmi_arg(self) -> &'static str {
        match self {
            Self::Pxe => "pxe",
            Self::Disk => "disk",
            Self::Cdrom => "cdrom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BmcKind {
    Redfish,
    Ipmi,
    Fake,
    /// A libvirt domain on the controller host acting as the machine's BMC (labs and tests:
    /// PXE-boot a throwaway VM through the whole provisioning flow). `address` is the
    /// domain name.
    Libvirt,
}

impl BmcKind {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "redfish" | "" => Ok(Self::Redfish),
            "ipmi" => Ok(Self::Ipmi),
            "fake" => Ok(Self::Fake),
            "libvirt" => Ok(Self::Libvirt),
            other => {
                anyhow::bail!("unsupported bmc_type '{other}' (redfish, ipmi, libvirt or fake)")
            }
        }
    }
}

pub struct BmcTarget<'a> {
    pub kind: BmcKind,
    pub address: &'a str,
    pub username: &'a str,
    pub password: &'a str,
}

/// BMCs ship self-signed certificates, so verification is off unless asked for.
/// The BMC network is a management VLAN; set `MACHINA_BMC_VERIFY_TLS=1` when the
/// BMCs carry certificates from a CA the controller trusts.
fn verify_tls() -> bool {
    matches!(
        std::env::var("MACHINA_BMC_VERIFY_TLS").ok().as_deref(),
        Some("1" | "true" | "yes")
    )
}

/// An address (or `host:port`) that is safe to put on an `ipmitool` argv and in a URL.
pub fn validate_address(addr: &str) -> anyhow::Result<()> {
    let a = addr.trim();
    if a.is_empty() {
        anyhow::bail!("BMC address is not set");
    }
    if a.starts_with('-') {
        anyhow::bail!("invalid BMC address");
    }
    let body = a
        .strip_prefix("https://")
        .or_else(|| a.strip_prefix("http://"))
        .unwrap_or(a)
        .trim_end_matches('/');
    if body.is_empty()
        || !body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '_' | '[' | ']'))
    {
        anyhow::bail!("invalid BMC address '{a}'");
    }
    Ok(())
}

pub fn validate_username(user: &str) -> anyhow::Result<()> {
    if user.is_empty() || user.starts_with('-') || user.chars().any(|c| c.is_control()) {
        anyhow::bail!("invalid BMC username");
    }
    Ok(())
}

/// `https://addr` unless the operator gave a scheme.
pub fn base_url(addr: &str) -> String {
    let a = addr.trim().trim_end_matches('/');
    if a.starts_with("https://") || a.starts_with("http://") {
        a.to_string()
    } else {
        format!("https://{a}")
    }
}

/// First member of a Redfish `Systems` collection document.
pub fn first_system_uri(collection: &Value) -> Option<String> {
    collection
        .get("Members")?
        .as_array()?
        .iter()
        .find_map(|m| m.get("@odata.id").and_then(Value::as_str))
        .map(str::to_string)
}

/// Reset action target from a `ComputerSystem` document, with the standard path as fallback.
pub fn reset_target(system: &Value, system_uri: &str) -> String {
    system
        .pointer("/Actions/#ComputerSystem.Reset/target")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "{}/Actions/ComputerSystem.Reset",
                system_uri.trim_end_matches('/')
            )
        })
}

/// `PowerState` of a Redfish `ComputerSystem` document.
pub fn parse_redfish_power_state(system: &Value) -> PowerState {
    match system.get("PowerState").and_then(Value::as_str) {
        Some("On" | "PoweringOn") => PowerState::On,
        Some("Off" | "PoweringOff") => PowerState::Off,
        _ => PowerState::Unknown,
    }
}

/// The `ResetType` to send: the first value the BMC lists as allowed from our preference
/// order for `action`. A BMC that lists nothing gets the standard value. A list with none of
/// our candidates is an error that says what it does allow (many BMCs lack `PowerCycle`).
pub fn pick_reset_type(system: &Value, action: PowerAction) -> anyhow::Result<String> {
    let preference: &[&str] = match action {
        PowerAction::On => &["On", "ForceOn"],
        PowerAction::Off => &["ForceOff", "GracefulShutdown"],
        PowerAction::Cycle => &["PowerCycle", "ForceRestart", "GracefulRestart"],
        PowerAction::Reset => &["ForceRestart", "GracefulRestart", "PowerCycle"],
    };
    let allowed: Option<Vec<&str>> = system
        .pointer("/Actions/#ComputerSystem.Reset/ResetType@Redfish.AllowableValues")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect());
    match allowed {
        None => Ok(action.redfish_reset_type().to_string()),
        Some(list) => preference
            .iter()
            .find(|p| list.contains(p))
            .map(|p| (*p).to_string())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "BMC does not allow {} (it allows: {})",
                    preference.join("/"),
                    list.join(", ")
                )
            }),
    }
}

/// The `PATCH` body that sets the boot override, checked against what the BMC allows.
pub fn boot_override_body(
    system: &Value,
    device: BootDevice,
    persistent: bool,
    uefi: Option<bool>,
) -> anyhow::Result<Value> {
    let target = device.redfish_target();
    if let Some(list) = system
        .pointer("/Boot/BootSourceOverrideTarget@Redfish.AllowableValues")
        .and_then(Value::as_array)
    {
        let names: Vec<&str> = list.iter().filter_map(Value::as_str).collect();
        if !names.contains(&target) {
            anyhow::bail!(
                "BMC does not allow boot target {target} (it allows: {})",
                names.join(", ")
            );
        }
    }
    let mut boot = json!({
        "BootSourceOverrideTarget": target,
        "BootSourceOverrideEnabled": if persistent { "Continuous" } else { "Once" },
    });
    if let Some(u) = uefi {
        boot["BootSourceOverrideMode"] = json!(if u { "UEFI" } else { "Legacy" });
    }
    Ok(json!({ "Boot": boot }))
}

/// `Chassis Power is on` / `... is off` from `ipmitool chassis power status`.
pub fn parse_ipmi_power_status(out: &str) -> PowerState {
    let l = out.to_ascii_lowercase();
    if l.contains("is on") {
        PowerState::On
    } else if l.contains("is off") {
        PowerState::Off
    } else {
        PowerState::Unknown
    }
}

/// Apply `action` through the BMC; returns a one-line summary of what happened. Powering on
/// something that is on (or off something that is off) is a no-op, not an error.
pub async fn power(target: &BmcTarget<'_>, action: PowerAction) -> anyhow::Result<String> {
    match target.kind {
        BmcKind::Fake => Ok(format!(
            "fake BMC: power {} (no hardware)",
            action.ipmi_verb()
        )),
        BmcKind::Ipmi => ipmi_power(target, action).await,
        BmcKind::Libvirt => libvirt_power(target, action).await,
        BmcKind::Redfish => redfish_power(target, action).await,
    }
}

/// The BMC's own report of the machine's power state.
pub async fn power_state(target: &BmcTarget<'_>) -> anyhow::Result<PowerState> {
    match target.kind {
        BmcKind::Fake => Ok(PowerState::Unknown),
        BmcKind::Ipmi => ipmi_power_state(target).await,
        BmcKind::Libvirt => libvirt_power_state(target).await,
        BmcKind::Redfish => {
            let rf = Redfish::connect(target)?;
            let (_, system) = rf.system().await?;
            Ok(parse_redfish_power_state(&system))
        }
    }
}

/// Choose the device for the next boot (once, or until changed).
pub async fn set_boot_device(
    target: &BmcTarget<'_>,
    device: BootDevice,
    persistent: bool,
    uefi: Option<bool>,
) -> anyhow::Result<String> {
    match target.kind {
        BmcKind::Fake => Ok(format!("fake BMC: next boot {}", device.as_str())),
        BmcKind::Libvirt => Ok(format!(
            "libvirt domain: boot order is fixed in the domain definition; \
             {} override not applied",
            device.as_str()
        )),
        BmcKind::Ipmi => ipmi_set_boot(target, device, persistent, uefi).await,
        BmcKind::Redfish => {
            let rf = Redfish::connect(target)?;
            let (uri, system) = rf.system().await?;
            let body = boot_override_body(&system, device, persistent, uefi)?;
            rf.send("PATCH", &uri, Some(&body)).await?;
            Ok(format!(
                "Redfish: next boot {} ({})",
                device.as_str(),
                if persistent { "until changed" } else { "once" }
            ))
        }
    }
}

/// Domains a libvirt "BMC" may touch must start with this prefix (default `metal-test-`),
/// so a mistyped address cannot power off a real VM on the host. `MACHINA_BMC_LIBVIRT_ANY=1`
/// lifts the rule.
pub fn libvirt_name_allowed(name: &str) -> anyhow::Result<()> {
    if name.is_empty()
        || name.starts_with('-')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        anyhow::bail!("invalid libvirt domain name '{name}'");
    }
    if matches!(
        std::env::var("MACHINA_BMC_LIBVIRT_ANY").ok().as_deref(),
        Some("1" | "true" | "yes")
    ) {
        return Ok(());
    }
    let prefix =
        std::env::var("MACHINA_BMC_LIBVIRT_PREFIX").unwrap_or_else(|_| "metal-test-".into());
    if !name.starts_with(&prefix) {
        anyhow::bail!(
            "libvirt domain '{name}' does not start with '{prefix}' \
             (set MACHINA_BMC_LIBVIRT_PREFIX, or MACHINA_BMC_LIBVIRT_ANY=1 to allow any domain)"
        );
    }
    Ok(())
}

async fn virsh(args: &[&str]) -> anyhow::Result<std::process::Output> {
    let uri = std::env::var("MACHINA_BMC_LIBVIRT_URI").unwrap_or_else(|_| "qemu:///system".into());
    let mut full = vec!["-c", uri.as_str()];
    full.extend_from_slice(args);
    tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new("virsh").args(&full).output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("virsh timed out after 30s"))?
    .map_err(|e| {
        anyhow::anyhow!("virsh exec failed (is libvirt installed on the controller host?): {e}")
    })
}

async fn virsh_ok(args: &[&str]) -> anyhow::Result<()> {
    let out = virsh(args).await?;
    if out.status.success() {
        Ok(())
    } else {
        anyhow::bail!(
            "virsh {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )
    }
}

async fn libvirt_power_state(target: &BmcTarget<'_>) -> anyhow::Result<PowerState> {
    let name = target.address.trim();
    libvirt_name_allowed(name)?;
    let out = virsh(&["domstate", name]).await?;
    if !out.status.success() {
        anyhow::bail!(
            "virsh domstate {name} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(match String::from_utf8_lossy(&out.stdout).trim() {
        "running" | "idle" | "paused" | "in shutdown" => PowerState::On,
        "shut off" | "crashed" => PowerState::Off,
        _ => PowerState::Unknown,
    })
}

async fn libvirt_power(target: &BmcTarget<'_>, action: PowerAction) -> anyhow::Result<String> {
    let name = target.address.trim();
    libvirt_name_allowed(name)?;
    let running = async {
        let out = virsh(&["domstate", name]).await?;
        Ok::<bool, anyhow::Error>(
            out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "running",
        )
    };
    match action {
        PowerAction::On => {
            if !running.await? {
                virsh_ok(&["start", name]).await?;
            }
        }
        PowerAction::Off => {
            if running.await? {
                virsh_ok(&["destroy", name]).await?;
            }
        }
        PowerAction::Reset => virsh_ok(&["reset", name]).await?,
        PowerAction::Cycle => {
            if running.await? {
                virsh_ok(&["destroy", name]).await?;
            }
            virsh_ok(&["start", name]).await?;
        }
    }
    Ok(format!(
        "libvirt domain {name}: power {}",
        action.ipmi_verb()
    ))
}

/// `host` and optional port of a BMC address as an operator types it: `10.0.0.5`,
/// `10.0.0.5:6230`, `bmc1.example:623`, `[fd00::5]:623`, `https://10.0.0.5/`.
pub fn split_host_port(addr: &str) -> (&str, Option<u16>) {
    let a = addr
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    if let Some(rest) = a.strip_prefix('[') {
        // `[v6]` or `[v6]:port`
        if let Some((host, tail)) = rest.split_once(']') {
            let port = tail.strip_prefix(':').and_then(|p| p.parse().ok());
            return (host, port);
        }
        return (a, None);
    }
    // Exactly one colon: host:port. More than one is a bare IPv6 address.
    if a.matches(':').count() == 1 {
        if let Some((h, p)) = a.split_once(':') {
            if let Ok(port) = p.parse::<u16>() {
                return (h, Some(port));
            }
        }
    }
    (a, None)
}

/// Run `ipmitool -I lanplus ... <args>`; the password goes through `IPMI_PASSWORD`, never argv.
async fn ipmitool(target: &BmcTarget<'_>, args: &[&str]) -> anyhow::Result<String> {
    validate_address(target.address)?;
    validate_username(target.username)?;
    // `-H` takes a bare host and `-p` the port: split `host:port` (and `[v6]:port`), and
    // drop a scheme the operator may have typed.
    let (host, port) = split_host_port(target.address);
    let port_arg = port.map(|p| p.to_string());
    let mut full = vec!["-I", "lanplus", "-H", host, "-U", target.username, "-E"];
    if let Some(p) = port_arg.as_deref() {
        full.extend_from_slice(&["-p", p]);
    }
    full.extend_from_slice(args);
    let out = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new("ipmitool")
            .args(&full)
            .env("IPMI_PASSWORD", target.password)
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("ipmitool timed out after 30s"))?
    .map_err(|e| {
        anyhow::anyhow!("ipmitool exec failed (is it installed on the controller host?): {e}")
    })?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        anyhow::bail!(
            "ipmitool {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )
    }
}

async fn ipmi_power_state(target: &BmcTarget<'_>) -> anyhow::Result<PowerState> {
    Ok(parse_ipmi_power_status(
        &ipmitool(target, &["chassis", "power", "status"]).await?,
    ))
}

async fn ipmi_power(target: &BmcTarget<'_>, action: PowerAction) -> anyhow::Result<String> {
    let state = ipmi_power_state(target)
        .await
        .unwrap_or(PowerState::Unknown);
    let host = target.address.trim();
    match (action, state) {
        (PowerAction::On, PowerState::On) => return Ok(format!("IPMI: {host} is already on")),
        (PowerAction::Off, PowerState::Off) => return Ok(format!("IPMI: {host} is already off")),
        _ => {}
    }
    // `power cycle`/`reset` of a machine that is off is refused by BMCs; `on` is what is meant.
    let verb =
        if state == PowerState::Off && matches!(action, PowerAction::Cycle | PowerAction::Reset) {
            "on"
        } else {
            action.ipmi_verb()
        };
    if verb == "cycle" {
        // Some BMCs and emulators do not implement `power cycle` ("Invalid data field in
        // request"). Fall back to `reset`, then to off followed by on.
        if let Err(first) = ipmitool(target, &["chassis", "power", "cycle"]).await {
            tracing::warn!("IPMI power cycle on {host}: {first:#}; trying reset");
            if ipmitool(target, &["chassis", "power", "reset"])
                .await
                .is_err()
            {
                ipmitool(target, &["chassis", "power", "off"]).await?;
                tokio::time::sleep(if cfg!(test) {
                    Duration::from_millis(10)
                } else {
                    Duration::from_secs(3)
                })
                .await;
                ipmitool(target, &["chassis", "power", "on"]).await?;
                return Ok(format!("IPMI power cycle on {host} (off, then on)"));
            }
            return Ok(format!("IPMI power cycle on {host} (as reset)"));
        }
        return Ok(format!("IPMI power cycle on {host}"));
    }
    ipmitool(target, &["chassis", "power", verb]).await?;
    Ok(format!("IPMI power {verb} on {host}"))
}

async fn ipmi_set_boot(
    target: &BmcTarget<'_>,
    device: BootDevice,
    persistent: bool,
    uefi: Option<bool>,
) -> anyhow::Result<String> {
    let mut opts: Vec<&str> = Vec::new();
    if persistent {
        opts.push("persistent");
    }
    if uefi == Some(true) {
        opts.push("efiboot");
    }
    let opt_arg = format!("options={}", opts.join(","));
    let mut args = vec!["chassis", "bootdev", device.ipmi_arg()];
    if !opts.is_empty() {
        args.push(&opt_arg);
    }
    ipmitool(target, &args).await?;
    Ok(format!(
        "IPMI: next boot {} ({})",
        device.as_str(),
        if persistent { "until changed" } else { "once" }
    ))
}

/// A thin Redfish client: Basic auth, bounded retries on transport errors and 502/503/504,
/// and a wait on asynchronous actions (HTTP 202 with a task monitor).
struct Redfish {
    client: reqwest::Client,
    base: String,
    user: String,
    pass: String,
}

impl Redfish {
    fn connect(target: &BmcTarget<'_>) -> anyhow::Result<Self> {
        validate_address(target.address)?;
        validate_username(target.username)?;
        let mut b = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .danger_accept_invalid_certs(!verify_tls());
        // A CA bundle the operator trusts for BMC certificates (PEM, one or more certs).
        if let Ok(path) = std::env::var("MACHINA_BMC_CA_FILE") {
            if !path.trim().is_empty() {
                let pem = std::fs::read(path.trim())
                    .map_err(|e| anyhow::anyhow!("MACHINA_BMC_CA_FILE: {e}"))?;
                for cert in reqwest::Certificate::from_pem_bundle(&pem)
                    .map_err(|e| anyhow::anyhow!("MACHINA_BMC_CA_FILE: {e}"))?
                {
                    b = b.add_root_certificate(cert);
                }
            }
        }
        Ok(Self {
            client: b.build()?,
            base: base_url(target.address),
            user: target.username.to_string(),
            pass: target.password.to_string(),
        })
    }

    fn pause(attempt: u32) -> Duration {
        if cfg!(test) {
            Duration::from_millis(10)
        } else {
            Duration::from_millis(500 * u64::from(attempt))
        }
    }

    /// One request with retries. Returns the final status, the JSON body (if any) and the
    /// `Location` header.
    async fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> anyhow::Result<(reqwest::StatusCode, Option<Value>, Option<String>)> {
        const ATTEMPTS: u32 = 3;
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{}{}", self.base, path)
        };
        let m = reqwest::Method::from_bytes(method.as_bytes())?;
        let mut last = String::new();
        for attempt in 1..=ATTEMPTS {
            let mut req = self
                .client
                .request(m.clone(), &url)
                .basic_auth(&self.user, Some(&self.pass));
            if let Some(b) = body {
                req = req.json(b);
            }
            match req.send().await {
                Ok(resp) => {
                    let st = resp.status();
                    if matches!(st.as_u16(), 502 | 503 | 504) && attempt < ATTEMPTS {
                        last = format!("HTTP {st}");
                        tokio::time::sleep(Self::pause(attempt)).await;
                        continue;
                    }
                    let loc = resp
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string);
                    let text = resp.text().await.unwrap_or_default();
                    let json = serde_json::from_str::<Value>(&text).ok();
                    return Ok((st, json, loc));
                }
                Err(e) => {
                    last = e.to_string();
                    if attempt < ATTEMPTS {
                        tokio::time::sleep(Self::pause(attempt)).await;
                    }
                }
            }
        }
        anyhow::bail!("Redfish {method} {path}: {last}")
    }

    async fn get(&self, path: &str) -> anyhow::Result<Value> {
        let (st, json, _) = self.call("GET", path, None).await?;
        if !st.is_success() {
            anyhow::bail!("Redfish GET {path}: HTTP {st}");
        }
        json.ok_or_else(|| anyhow::anyhow!("Redfish GET {path}: not JSON"))
    }

    /// PATCH/POST that must succeed; a 202 is followed to completion.
    async fn send(&self, method: &str, path: &str, body: Option<&Value>) -> anyhow::Result<()> {
        let (st, json, loc) = self.call(method, path, body).await?;
        if st.as_u16() == 202 {
            if let Some(monitor) = loc {
                return self.wait_task(&monitor).await;
            }
            return Ok(());
        }
        if !st.is_success() {
            let detail = json
                .as_ref()
                .and_then(|j| {
                    j.pointer("/error/message")
                        .or_else(|| j.pointer("/error/@Message.ExtendedInfo/0/Message"))
                })
                .and_then(Value::as_str)
                .unwrap_or("");
            anyhow::bail!("Redfish {method} {path} rejected: HTTP {st} {detail}");
        }
        Ok(())
    }

    /// Poll a task monitor until it stops answering 202 (up to about a minute).
    async fn wait_task(&self, monitor: &str) -> anyhow::Result<()> {
        let tries = if cfg!(test) { 5 } else { 30 };
        for _ in 0..tries {
            let (st, json, _) = self.call("GET", monitor, None).await?;
            if st.as_u16() != 202 {
                if !st.is_success() {
                    anyhow::bail!("Redfish task failed: HTTP {st}");
                }
                if let Some(state) = json
                    .as_ref()
                    .and_then(|j| j.get("TaskState"))
                    .and_then(Value::as_str)
                {
                    if matches!(state, "Exception" | "Killed" | "Cancelled") {
                        anyhow::bail!("Redfish task ended: {state}");
                    }
                }
                return Ok(());
            }
            tokio::time::sleep(if cfg!(test) {
                Duration::from_millis(10)
            } else {
                Duration::from_secs(2)
            })
            .await;
        }
        anyhow::bail!("Redfish task did not finish in time")
    }

    /// The first `ComputerSystem`: its URI and document.
    async fn system(&self) -> anyhow::Result<(String, Value)> {
        let collection = self.get("/redfish/v1/Systems").await?;
        let uri = first_system_uri(&collection)
            .ok_or_else(|| anyhow::anyhow!("Redfish: no ComputerSystem in /redfish/v1/Systems"))?;
        let doc = self.get(&uri).await?;
        Ok((uri, doc))
    }
}

async fn redfish_power(target: &BmcTarget<'_>, action: PowerAction) -> anyhow::Result<String> {
    let rf = Redfish::connect(target)?;
    let (uri, system) = rf.system().await?;
    let state = parse_redfish_power_state(&system);
    let addr = target.address.trim();
    match (action, state) {
        (PowerAction::On, PowerState::On) => return Ok(format!("Redfish: {addr} is already on")),
        (PowerAction::Off, PowerState::Off) => {
            return Ok(format!("Redfish: {addr} is already off"))
        }
        _ => {}
    }
    // A reset or cycle of a machine that is off cannot work; powering it on is what was meant.
    let effective =
        if state == PowerState::Off && matches!(action, PowerAction::Reset | PowerAction::Cycle) {
            PowerAction::On
        } else {
            action
        };
    let reset_type = pick_reset_type(&system, effective)?;
    let reset = reset_target(&system, &uri);
    rf.send("POST", &reset, Some(&json!({ "ResetType": reset_type })))
        .await?;
    Ok(format!("Redfish {reset_type} on {addr}"))
}

// --- read model: what the BMC reports about the machine -------------------------------

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, PartialEq)]
pub struct BmcIdentity {
    pub manufacturer: String,
    pub model: String,
    pub serial_number: String,
    pub sku: String,
    pub bios_version: String,
    pub cpu_count: u64,
    pub cpu_model: String,
    pub memory_gib: f64,
    pub power_state: String,
    pub health: String,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq)]
pub struct BmcTemperature {
    pub name: String,
    pub celsius: f64,
    pub health: String,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq)]
pub struct BmcFirmware {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq)]
pub struct BmcLogEntry {
    pub created: String,
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct BmcInfo {
    pub identity: BmcIdentity,
    pub temperatures: Vec<BmcTemperature>,
    pub power_consumed_watts: Option<f64>,
    pub firmware: Vec<BmcFirmware>,
    pub log: Vec<BmcLogEntry>,
    /// Sections the BMC did not answer (a missing resource is normal, not a failure).
    pub unavailable: Vec<String>,
}

fn s(v: &Value, ptr: &str) -> String {
    v.pointer(ptr)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Identity and health from a `ComputerSystem` document.
pub fn parse_identity(system: &Value) -> BmcIdentity {
    let health = {
        let h = s(system, "/Status/HealthRollup");
        if h.is_empty() {
            s(system, "/Status/Health")
        } else {
            h
        }
    };
    BmcIdentity {
        manufacturer: s(system, "/Manufacturer"),
        model: s(system, "/Model"),
        serial_number: s(system, "/SerialNumber"),
        sku: s(system, "/SKU"),
        bios_version: s(system, "/BiosVersion"),
        cpu_count: system
            .pointer("/ProcessorSummary/Count")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cpu_model: s(system, "/ProcessorSummary/Model"),
        memory_gib: system
            .pointer("/MemorySummary/TotalSystemMemoryGiB")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        power_state: s(system, "/PowerState"),
        health,
    }
}

/// Readings from a `Thermal` document (sensors with no reading are skipped).
pub fn parse_temperatures(thermal: &Value) -> Vec<BmcTemperature> {
    thermal
        .get("Temperatures")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    Some(BmcTemperature {
                        name: s(t, "/Name"),
                        celsius: t.get("ReadingCelsius").and_then(Value::as_f64)?,
                        health: s(t, "/Status/Health"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Total consumed watts from a `Power` document (the first `PowerControl` that reports one).
pub fn parse_power_watts(power: &Value) -> Option<f64> {
    power
        .get("PowerControl")?
        .as_array()?
        .iter()
        .find_map(|p| p.get("PowerConsumedWatts").and_then(Value::as_f64))
}

/// Entries of a log service, newest first, at most `limit`.
pub fn parse_log_entries(entries: &Value, limit: usize) -> Vec<BmcLogEntry> {
    let mut v: Vec<BmcLogEntry> = entries
        .get("Members")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|e| BmcLogEntry {
                    created: s(e, "/Created"),
                    severity: s(e, "/Severity"),
                    message: s(e, "/Message"),
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| b.created.cmp(&a.created));
    v.truncate(limit);
    v
}

fn first_link(doc: &Value, pointer: &str) -> Option<String> {
    doc.pointer(pointer)
        .and_then(|v| match v {
            Value::Array(a) => a.first().and_then(|m| m.get("@odata.id")),
            other => other.get("@odata.id"),
        })
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn member_uris(collection: &Value) -> Vec<String> {
    collection
        .get("Members")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|m| m.get("@odata.id").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Everything the BMC will tell us, section by section. Redfish only; a section the BMC
/// lacks lands in `unavailable` rather than failing the whole read.
pub async fn collect_info(target: &BmcTarget<'_>) -> anyhow::Result<BmcInfo> {
    if target.kind != BmcKind::Redfish {
        anyhow::bail!(
            "BMC details are read over Redfish; this server uses {:?}",
            target.kind
        );
    }
    let rf = Redfish::connect(target)?;
    let (_, system) = rf.system().await?;
    let mut info = BmcInfo {
        identity: parse_identity(&system),
        ..Default::default()
    };

    // Chassis: temperatures and power draw.
    match first_link(&system, "/Links/Chassis") {
        Some(ch_uri) => match rf.get(&ch_uri).await {
            Ok(ch) => {
                if let Some(t) = first_link(&ch, "/Thermal") {
                    match rf.get(&t).await {
                        Ok(doc) => info.temperatures = parse_temperatures(&doc),
                        Err(_) => info.unavailable.push("thermal".into()),
                    }
                } else {
                    info.unavailable.push("thermal".into());
                }
                if let Some(p) = first_link(&ch, "/Power") {
                    match rf.get(&p).await {
                        Ok(doc) => info.power_consumed_watts = parse_power_watts(&doc),
                        Err(_) => info.unavailable.push("power".into()),
                    }
                } else {
                    info.unavailable.push("power".into());
                }
            }
            Err(_) => {
                info.unavailable.push("thermal".into());
                info.unavailable.push("power".into());
            }
        },
        None => {
            info.unavailable.push("thermal".into());
            info.unavailable.push("power".into());
        }
    }

    // Firmware inventory.
    let mut fw_ok = false;
    if let Ok(us) = rf.get("/redfish/v1/UpdateService").await {
        if let Some(inv) = first_link(&us, "/FirmwareInventory") {
            if let Ok(coll) = rf.get(&inv).await {
                fw_ok = true;
                for uri in member_uris(&coll).into_iter().take(40) {
                    if let Ok(doc) = rf.get(&uri).await {
                        let name = s(&doc, "/Name");
                        let version = s(&doc, "/Version");
                        if !name.is_empty() && !version.is_empty() {
                            info.firmware.push(BmcFirmware { name, version });
                        }
                    }
                }
            }
        }
    }
    if !fw_ok {
        info.unavailable.push("firmware".into());
    }

    // Log: the first log service of the first manager that has entries.
    let mut log_ok = false;
    if let Some(mgr_uri) = first_link(&system, "/Links/ManagedBy") {
        if let Ok(mgr) = rf.get(&mgr_uri).await {
            if let Some(ls) = first_link(&mgr, "/LogServices") {
                if let Ok(coll) = rf.get(&ls).await {
                    for svc in member_uris(&coll).into_iter().take(3) {
                        let Ok(doc) = rf.get(&svc).await else {
                            continue;
                        };
                        let Some(entries) = first_link(&doc, "/Entries") else {
                            continue;
                        };
                        if let Ok(list) = rf.get(&entries).await {
                            info.log = parse_log_entries(&list, 50);
                            log_ok = true;
                            break;
                        }
                    }
                }
            }
        }
    }
    if !log_ok {
        info.unavailable.push("log".into());
    }
    Ok(info)
}

// --- virtual media ----------------------------------------------------------------------

/// The first virtual-media member that takes a CD or DVD: its URI and document.
pub fn pick_cd_media(members: &[(String, Value)]) -> Option<(&str, &Value)> {
    members.iter().find_map(|(uri, doc)| {
        let is_cd = doc
            .get("MediaTypes")
            .and_then(Value::as_array)
            .is_some_and(|t| {
                t.iter()
                    .filter_map(Value::as_str)
                    .any(|m| m.eq_ignore_ascii_case("CD") || m.eq_ignore_ascii_case("DVD"))
            });
        is_cd.then_some((uri.as_str(), doc))
    })
}

/// URI of a `VirtualMedia` action with the standard path as the fallback.
pub fn media_action_target(media: &Value, uri: &str, action: &str) -> String {
    media
        .pointer(&format!("/Actions/#VirtualMedia.{action}/target"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "{}/Actions/VirtualMedia.{action}",
                uri.trim_end_matches('/')
            )
        })
}

impl Redfish {
    /// The CD/DVD virtual-media slot of the machine: through the system, else its manager.
    async fn cd_media(&self, system: &Value) -> anyhow::Result<(String, Value)> {
        let mut collections: Vec<String> = Vec::new();
        if let Some(c) = first_link(system, "/VirtualMedia") {
            collections.push(c);
        }
        if let Some(m) = first_link(system, "/Links/ManagedBy") {
            if let Ok(mgr) = self.get(&m).await {
                if let Some(c) = first_link(&mgr, "/VirtualMedia") {
                    collections.push(c);
                }
            }
        }
        for c in collections {
            let Ok(coll) = self.get(&c).await else {
                continue;
            };
            let mut docs = Vec::new();
            for uri in member_uris(&coll).into_iter().take(8) {
                if let Ok(d) = self.get(&uri).await {
                    docs.push((uri, d));
                }
            }
            if let Some((uri, doc)) = pick_cd_media(&docs) {
                return Ok((uri.to_string(), doc.clone()));
            }
        }
        anyhow::bail!("this BMC exposes no CD/DVD virtual media")
    }
}

/// Mount `image_url` (http/https) as the machine's virtual CD. An image already mounted is
/// replaced; the media is write-protected.
pub async fn insert_media(target: &BmcTarget<'_>, image_url: &str) -> anyhow::Result<String> {
    if !(image_url.starts_with("http://") || image_url.starts_with("https://"))
        || image_url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        anyhow::bail!("virtual media image must be an http(s) URL");
    }
    if target.kind != BmcKind::Redfish {
        anyhow::bail!(
            "virtual media needs a Redfish BMC; this server uses {:?}",
            target.kind
        );
    }
    let rf = Redfish::connect(target)?;
    let (_, system) = rf.system().await?;
    let (uri, media) = rf.cd_media(&system).await?;
    let inserted = media
        .get("Inserted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let current = media.get("Image").and_then(Value::as_str).unwrap_or("");
    if inserted && current == image_url {
        return Ok(format!("virtual media: {image_url} is already mounted"));
    }
    if inserted {
        let eject = media_action_target(&media, &uri, "EjectMedia");
        rf.send("POST", &eject, Some(&json!({}))).await?;
    }
    let has_action = media
        .pointer("/Actions/#VirtualMedia.InsertMedia")
        .is_some();
    if has_action {
        let t = media_action_target(&media, &uri, "InsertMedia");
        rf.send(
            "POST",
            &t,
            Some(&json!({ "Image": image_url, "Inserted": true, "WriteProtected": true })),
        )
        .await?;
    } else {
        // Older BMCs: the slot itself is PATCHed.
        rf.send(
            "PATCH",
            &uri,
            Some(&json!({ "Image": image_url, "Inserted": true })),
        )
        .await?;
    }
    Ok(format!("virtual media: mounted {image_url}"))
}

/// Eject whatever is mounted in the CD slot (nothing mounted is not an error).
pub async fn eject_media(target: &BmcTarget<'_>) -> anyhow::Result<String> {
    if target.kind != BmcKind::Redfish {
        anyhow::bail!(
            "virtual media needs a Redfish BMC; this server uses {:?}",
            target.kind
        );
    }
    let rf = Redfish::connect(target)?;
    let (_, system) = rf.system().await?;
    let (uri, media) = rf.cd_media(&system).await?;
    if !media
        .get("Inserted")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok("virtual media: nothing mounted".into());
    }
    let t = media_action_target(&media, &uri, "EjectMedia");
    rf.send("POST", &t, Some(&json!({}))).await?;
    Ok("virtual media: ejected".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_map_to_redfish_and_ipmi() {
        assert_eq!(PowerAction::parse("ON").unwrap().redfish_reset_type(), "On");
        assert_eq!(
            PowerAction::parse("off").unwrap().redfish_reset_type(),
            "ForceOff"
        );
        assert_eq!(
            PowerAction::parse("cycle").unwrap().redfish_reset_type(),
            "PowerCycle"
        );
        assert_eq!(PowerAction::parse("reset").unwrap().ipmi_verb(), "reset");
        assert!(PowerAction::parse("halt").is_err());
    }

    #[test]
    fn bmc_kind_defaults_to_redfish_and_rejects_unknown() {
        assert_eq!(BmcKind::parse("").unwrap(), BmcKind::Redfish);
        assert_eq!(BmcKind::parse("IPMI").unwrap(), BmcKind::Ipmi);
        assert!(BmcKind::parse("amt").is_err());
        assert_eq!(BmcKind::parse("libvirt").unwrap(), BmcKind::Libvirt);
    }

    #[test]
    fn libvirt_domains_are_limited_to_the_test_prefix() {
        // Env vars are process-wide; this test only reads the default rule.
        if std::env::var("MACHINA_BMC_LIBVIRT_ANY").is_ok()
            || std::env::var("MACHINA_BMC_LIBVIRT_PREFIX").is_ok()
        {
            return;
        }
        assert!(libvirt_name_allowed("metal-test-1").is_ok());
        assert!(libvirt_name_allowed("asg-prod-1").is_err());
        assert!(libvirt_name_allowed("-metal-test-1").is_err());
        assert!(libvirt_name_allowed("metal-test-1; rm").is_err());
        assert!(libvirt_name_allowed("").is_err());
    }

    #[test]
    fn address_validation_blocks_argv_and_url_tricks() {
        assert!(validate_address("10.0.0.5").is_ok());
        assert!(validate_address("bmc1.rack2.example:8443").is_ok());
        assert!(validate_address("https://[fd00::5]").is_ok());
        assert!(validate_address("").is_err());
        assert!(validate_address("-oProxyCommand=x").is_err());
        assert!(validate_address("10.0.0.5/../admin").is_err());
        assert!(validate_address("host name").is_err());
        assert!(validate_username("ADMIN").is_ok());
        assert!(validate_username("-U").is_err());
    }

    #[test]
    fn ipmi_addresses_split_into_host_and_port() {
        assert_eq!(split_host_port("10.0.0.5"), ("10.0.0.5", None));
        assert_eq!(split_host_port("10.0.0.5:6230"), ("10.0.0.5", Some(6230)));
        assert_eq!(
            split_host_port("bmc1.example:623"),
            ("bmc1.example", Some(623))
        );
        assert_eq!(split_host_port("[fd00::5]:623"), ("fd00::5", Some(623)));
        assert_eq!(split_host_port("[fd00::5]"), ("fd00::5", None));
        assert_eq!(split_host_port("fd00::5"), ("fd00::5", None));
        assert_eq!(
            split_host_port("https://10.0.0.5:8443/"),
            ("10.0.0.5", Some(8443))
        );
    }

    #[test]
    fn base_url_adds_https_only_without_a_scheme() {
        assert_eq!(base_url("10.0.0.5"), "https://10.0.0.5");
        assert_eq!(base_url("http://10.0.0.5/"), "http://10.0.0.5");
    }

    #[test]
    fn redfish_documents_resolve() {
        let coll = json!({"Members": [{"@odata.id": "/redfish/v1/Systems/System.Embedded.1"}]});
        let uri = first_system_uri(&coll).unwrap();
        assert_eq!(uri, "/redfish/v1/Systems/System.Embedded.1");
        let with_action = json!({"Actions": {"#ComputerSystem.Reset": {"target": "/x/Reset"}}});
        assert_eq!(reset_target(&with_action, &uri), "/x/Reset");
        assert_eq!(
            reset_target(&json!({}), &uri),
            "/redfish/v1/Systems/System.Embedded.1/Actions/ComputerSystem.Reset"
        );
        assert!(first_system_uri(&json!({"Members": []})).is_none());
    }

    #[test]
    fn power_state_and_reset_choice_follow_what_the_bmc_reports() {
        assert_eq!(
            parse_redfish_power_state(&json!({"PowerState": "On"})),
            PowerState::On
        );
        assert_eq!(
            parse_redfish_power_state(&json!({"PowerState": "Off"})),
            PowerState::Off
        );
        assert_eq!(parse_redfish_power_state(&json!({})), PowerState::Unknown);
        assert_eq!(
            parse_ipmi_power_status("Chassis Power is on\n"),
            PowerState::On
        );
        assert_eq!(
            parse_ipmi_power_status("Chassis Power is off"),
            PowerState::Off
        );
        assert_eq!(parse_ipmi_power_status("Error"), PowerState::Unknown);

        let lacks_cycle = json!({"Actions": {"#ComputerSystem.Reset": {
            "ResetType@Redfish.AllowableValues": ["On", "ForceOff", "ForceRestart"]}}});
        assert_eq!(
            pick_reset_type(&lacks_cycle, PowerAction::Cycle).unwrap(),
            "ForceRestart"
        );
        assert_eq!(
            pick_reset_type(&lacks_cycle, PowerAction::Off).unwrap(),
            "ForceOff"
        );
        let only_on = json!({"Actions": {"#ComputerSystem.Reset": {
            "ResetType@Redfish.AllowableValues": ["Nmi"]}}});
        let err = pick_reset_type(&only_on, PowerAction::Off)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Nmi"), "{err}");
        // No list: the standard value.
        assert_eq!(
            pick_reset_type(&json!({}), PowerAction::Cycle).unwrap(),
            "PowerCycle"
        );
    }

    #[test]
    fn boot_override_is_checked_against_the_allowed_targets() {
        let sys = json!({"Boot": {"BootSourceOverrideTarget@Redfish.AllowableValues": ["None", "Pxe", "Hdd"]}});
        let b = boot_override_body(&sys, BootDevice::Pxe, false, Some(true)).unwrap();
        assert_eq!(b["Boot"]["BootSourceOverrideTarget"], "Pxe");
        assert_eq!(b["Boot"]["BootSourceOverrideEnabled"], "Once");
        assert_eq!(b["Boot"]["BootSourceOverrideMode"], "UEFI");
        let c = boot_override_body(&sys, BootDevice::Disk, true, None).unwrap();
        assert_eq!(c["Boot"]["BootSourceOverrideEnabled"], "Continuous");
        assert!(c["Boot"].get("BootSourceOverrideMode").is_none());
        assert!(boot_override_body(&sys, BootDevice::Cdrom, false, None).is_err());
        assert!(BootDevice::parse("floppy").is_err());
    }

    // --- a small Redfish server for the client tests ---------------------------------

    struct Route {
        method: &'static str,
        path: &'static str,
        status: u16,
        body: String,
        location: Option<&'static str>,
        hits_left: Option<u32>,
    }

    fn route(method: &'static str, path: &'static str, status: u16, body: Value) -> Route {
        Route {
            method,
            path,
            status,
            body: body.to_string(),
            location: None,
            hits_left: None,
        }
    }

    type RequestLog = std::sync::Arc<std::sync::Mutex<Vec<(String, String, String)>>>;

    async fn mock_redfish(routes: Vec<Route>) -> (String, RequestLog) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = format!("http://{}", l.local_addr().unwrap());
        let log: RequestLog = Default::default();
        let routes = std::sync::Arc::new(std::sync::Mutex::new(routes));
        let log2 = log.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                let (routes, log) = (routes.clone(), log2.clone());
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 4096];
                    loop {
                        let n = s.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                        let text = String::from_utf8_lossy(&buf).to_string();
                        if let Some(h) = text.find("\r\n\r\n") {
                            let len = text[..h]
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .and_then(|v| v.trim().parse::<usize>().ok())
                                })
                                .unwrap_or(0);
                            if buf.len() >= h + 4 + len {
                                break;
                            }
                        }
                    }
                    let text = String::from_utf8_lossy(&buf).to_string();
                    let mut first = text.lines().next().unwrap_or("").split_whitespace();
                    let (m, p) = (
                        first.next().unwrap_or("").to_string(),
                        first.next().unwrap_or("").to_string(),
                    );
                    let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                    log.lock().unwrap().push((m.clone(), p.clone(), body));
                    let (status, payload, location) = {
                        let mut rs = routes.lock().unwrap();
                        match rs
                            .iter_mut()
                            .find(|r| r.method == m && r.path == p && r.hits_left != Some(0))
                        {
                            Some(r) => {
                                if let Some(h) = r.hits_left.as_mut() {
                                    *h -= 1;
                                }
                                (r.status, r.body.clone(), r.location)
                            }
                            None => (404, "{}".to_string(), None),
                        }
                    };
                    let loc = location
                        .map(|l| format!("Location: {l}\r\n"))
                        .unwrap_or_default();
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{loc}Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                        payload.len()
                    );
                    let _ = s.write_all(resp.as_bytes()).await;
                });
            }
        });
        (addr, log)
    }

    fn systems() -> Vec<Route> {
        vec![route(
            "GET",
            "/redfish/v1/Systems",
            200,
            json!({"Members": [{"@odata.id": "/redfish/v1/Systems/1"}]}),
        )]
    }

    fn target<'a>(addr: &'a str) -> BmcTarget<'a> {
        BmcTarget {
            kind: BmcKind::Redfish,
            address: addr,
            username: "admin",
            password: "pw",
        }
    }

    #[tokio::test]
    async fn redfish_power_on_of_a_running_machine_sends_nothing() {
        let mut r = systems();
        r.push(route(
            "GET",
            "/redfish/v1/Systems/1",
            200,
            json!({"PowerState": "On"}),
        ));
        let (addr, log) = mock_redfish(r).await;
        let out = power(&target(&addr), PowerAction::On).await.unwrap();
        assert!(out.contains("already on"), "{out}");
        assert!(log.lock().unwrap().iter().all(|(m, _, _)| m == "GET"));
        assert_eq!(power_state(&target(&addr)).await.unwrap(), PowerState::On);
    }

    #[tokio::test]
    async fn redfish_cycle_uses_a_reset_type_the_bmc_allows_and_retries_a_503() {
        let mut r = systems();
        // The first answer for the system document is a 503; the retry gets the real one.
        r.push(Route {
            hits_left: Some(1),
            ..route("GET", "/redfish/v1/Systems/1", 503, json!({}))
        });
        r.push(route(
            "GET",
            "/redfish/v1/Systems/1",
            200,
            json!({"PowerState": "On", "Actions": {"#ComputerSystem.Reset": {
                "target": "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset",
                "ResetType@Redfish.AllowableValues": ["On", "ForceOff", "ForceRestart"]}}}),
        ));
        r.push(route(
            "POST",
            "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset",
            204,
            json!({}),
        ));
        let (addr, log) = mock_redfish(r).await;
        let out = power(&target(&addr), PowerAction::Cycle).await.unwrap();
        assert!(out.contains("ForceRestart"), "{out}");
        let log = log.lock().unwrap();
        let post = log.iter().find(|(m, _, _)| m == "POST").unwrap();
        assert!(post.2.contains("ForceRestart"), "{}", post.2);
    }

    #[tokio::test]
    async fn redfish_reset_of_a_powered_off_machine_powers_it_on() {
        let mut r = systems();
        r.push(route(
            "GET",
            "/redfish/v1/Systems/1",
            200,
            json!({"PowerState": "Off"}),
        ));
        r.push(route(
            "POST",
            "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset",
            200,
            json!({}),
        ));
        let (addr, log) = mock_redfish(r).await;
        power(&target(&addr), PowerAction::Reset).await.unwrap();
        let log = log.lock().unwrap();
        let post = log.iter().find(|(m, _, _)| m == "POST").unwrap();
        assert!(post.2.contains("\"On\""), "{}", post.2);
    }

    #[tokio::test]
    async fn redfish_async_reset_is_followed_to_completion() {
        let mut r = systems();
        r.push(route(
            "GET",
            "/redfish/v1/Systems/1",
            200,
            json!({"PowerState": "On"}),
        ));
        r.push(Route {
            location: Some("/redfish/v1/TaskService/Tasks/7"),
            ..route(
                "POST",
                "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset",
                202,
                json!({}),
            )
        });
        r.push(Route {
            hits_left: Some(1),
            ..route("GET", "/redfish/v1/TaskService/Tasks/7", 202, json!({}))
        });
        r.push(route(
            "GET",
            "/redfish/v1/TaskService/Tasks/7",
            200,
            json!({"TaskState": "Completed"}),
        ));
        let (addr, log) = mock_redfish(r).await;
        power(&target(&addr), PowerAction::Reset).await.unwrap();
        let polls = log
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, p, _)| m == "GET" && p.contains("Tasks/7"))
            .count();
        assert_eq!(polls, 2);
    }

    #[tokio::test]
    async fn redfish_boot_override_patches_the_system() {
        let mut r = systems();
        r.push(route(
            "GET",
            "/redfish/v1/Systems/1",
            200,
            json!({"Boot": {"BootSourceOverrideTarget@Redfish.AllowableValues": ["Pxe", "Hdd"]}}),
        ));
        r.push(route("PATCH", "/redfish/v1/Systems/1", 200, json!({})));
        let (addr, log) = mock_redfish(r).await;
        let out = set_boot_device(&target(&addr), BootDevice::Pxe, false, Some(true))
            .await
            .unwrap();
        assert!(out.contains("pxe") && out.contains("once"), "{out}");
        let log = log.lock().unwrap();
        let patch = log.iter().find(|(m, _, _)| m == "PATCH").unwrap();
        let body: Value = serde_json::from_str(&patch.2).unwrap();
        assert_eq!(body["Boot"]["BootSourceOverrideTarget"], "Pxe");
        assert_eq!(body["Boot"]["BootSourceOverrideEnabled"], "Once");
        // A target the BMC does not list is refused before anything is sent.
        drop(log);
        assert!(
            set_boot_device(&target(&addr), BootDevice::Cdrom, false, None)
                .await
                .is_err()
        );
    }

    #[test]
    fn redfish_documents_become_the_read_model() {
        let sys = json!({
            "Manufacturer": "Dell Inc.", "Model": "PowerEdge R760", "SerialNumber": "ABC123",
            "BiosVersion": "2.1.0", "PowerState": "On",
            "ProcessorSummary": {"Count": 2, "Model": "Xeon Gold"},
            "MemorySummary": {"TotalSystemMemoryGiB": 512.0},
            "Status": {"Health": "OK", "HealthRollup": "Warning"}
        });
        let id = parse_identity(&sys);
        assert_eq!(id.manufacturer, "Dell Inc.");
        assert_eq!((id.cpu_count, id.memory_gib), (2, 512.0));
        assert_eq!(
            id.health, "Warning",
            "the rollup wins over the system's own health"
        );

        let thermal = json!({"Temperatures": [
            {"Name": "CPU1", "ReadingCelsius": 54.0, "Status": {"Health": "OK"}},
            {"Name": "Absent", "ReadingCelsius": null}]});
        let t = parse_temperatures(&thermal);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].name, "CPU1");

        let power = json!({"PowerControl": [{"Name": "x"}, {"PowerConsumedWatts": 312.5}]});
        assert_eq!(parse_power_watts(&power), Some(312.5));
        assert_eq!(parse_power_watts(&json!({})), None);

        let log = json!({"Members": [
            {"Created": "2026-10-01T00:00:00Z", "Severity": "OK", "Message": "old"},
            {"Created": "2026-10-12T00:00:00Z", "Severity": "Critical", "Message": "fan failed"}]});
        let l = parse_log_entries(&log, 1);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].message, "fan failed");
    }

    #[tokio::test]
    async fn collect_info_reads_what_exists_and_lists_what_does_not() {
        let mut r = systems();
        r.push(route(
            "GET",
            "/redfish/v1/Systems/1",
            200,
            json!({"Model": "M1", "PowerState": "On", "Status": {"Health": "OK"},
                   "Links": {"Chassis": [{"@odata.id": "/redfish/v1/Chassis/1"}],
                             "ManagedBy": [{"@odata.id": "/redfish/v1/Managers/1"}]}}),
        ));
        r.push(route(
            "GET",
            "/redfish/v1/Chassis/1",
            200,
            json!({"Thermal": {"@odata.id": "/redfish/v1/Chassis/1/Thermal"}}),
        ));
        r.push(route(
            "GET",
            "/redfish/v1/Chassis/1/Thermal",
            200,
            json!({"Temperatures": [{"Name": "Inlet", "ReadingCelsius": 24.0}]}),
        ));
        r.push(route(
            "GET",
            "/redfish/v1/Managers/1",
            200,
            json!({"LogServices": {"@odata.id": "/redfish/v1/Managers/1/LogServices"}}),
        ));
        r.push(route(
            "GET",
            "/redfish/v1/Managers/1/LogServices",
            200,
            json!({"Members": [{"@odata.id": "/redfish/v1/Managers/1/LogServices/Sel"}]}),
        ));
        r.push(route(
            "GET",
            "/redfish/v1/Managers/1/LogServices/Sel",
            200,
            json!({"Entries": {"@odata.id": "/redfish/v1/Managers/1/LogServices/Sel/Entries"}}),
        ));
        r.push(route(
            "GET",
            "/redfish/v1/Managers/1/LogServices/Sel/Entries",
            200,
            json!({"Members": [{"Created": "2026-10-12T00:00:00Z", "Severity": "Warning", "Message": "temp high"}]}),
        ));
        let (addr, _) = mock_redfish(r).await;
        let info = collect_info(&target(&addr)).await.unwrap();
        assert_eq!(info.identity.model, "M1");
        assert_eq!(info.temperatures.len(), 1);
        assert_eq!(info.log[0].message, "temp high");
        // No Power link and no UpdateService on this BMC: reported, not fatal.
        assert!(info.unavailable.contains(&"power".to_string()));
        assert!(info.unavailable.contains(&"firmware".to_string()));
        assert!(!info.unavailable.contains(&"log".to_string()));
        // Only Redfish servers have this read model.
        let t = BmcTarget {
            kind: BmcKind::Ipmi,
            ..target(&addr)
        };
        assert!(collect_info(&t).await.is_err());
    }

    #[test]
    fn virtual_media_slot_selection_and_action_targets() {
        let members = vec![
            (
                "/m/Floppy".to_string(),
                json!({"MediaTypes": ["Floppy", "USBStick"]}),
            ),
            (
                "/m/CD".to_string(),
                json!({"MediaTypes": ["CD", "DVD"],
                "Actions": {"#VirtualMedia.InsertMedia": {"target": "/m/CD/Actions/VirtualMedia.InsertMedia"}}}),
            ),
        ];
        let (uri, doc) = pick_cd_media(&members).unwrap();
        assert_eq!(uri, "/m/CD");
        assert_eq!(
            media_action_target(doc, uri, "InsertMedia"),
            "/m/CD/Actions/VirtualMedia.InsertMedia"
        );
        assert_eq!(
            media_action_target(doc, uri, "EjectMedia"),
            "/m/CD/Actions/VirtualMedia.EjectMedia"
        );
        assert!(pick_cd_media(&members[..1]).is_none());
    }

    fn media_bmc() -> Vec<Route> {
        let mut r = systems();
        r.push(route(
            "GET",
            "/redfish/v1/Systems/1",
            200,
            json!({"VirtualMedia": {"@odata.id": "/redfish/v1/Managers/1/VirtualMedia"}}),
        ));
        r.push(route(
            "GET",
            "/redfish/v1/Managers/1/VirtualMedia",
            200,
            json!({"Members": [{"@odata.id": "/redfish/v1/Managers/1/VirtualMedia/CD1"}]}),
        ));
        r
    }

    #[tokio::test]
    async fn insert_media_posts_the_action_and_skips_an_identical_mount() {
        let mut r = media_bmc();
        r.push(route(
            "GET",
            "/redfish/v1/Managers/1/VirtualMedia/CD1",
            200,
            json!({"MediaTypes": ["CD"], "Inserted": false,
                "Actions": {"#VirtualMedia.InsertMedia": {
                    "target": "/redfish/v1/Managers/1/VirtualMedia/CD1/Actions/VirtualMedia.InsertMedia"}}}),
        ));
        r.push(route(
            "POST",
            "/redfish/v1/Managers/1/VirtualMedia/CD1/Actions/VirtualMedia.InsertMedia",
            204,
            json!({}),
        ));
        let (addr, log) = mock_redfish(r).await;
        let out = insert_media(&target(&addr), "http://ctl/dist/metal-ipxe.iso")
            .await
            .unwrap();
        assert!(out.contains("mounted"), "{out}");
        let post = log
            .lock()
            .unwrap()
            .iter()
            .find(|(m, _, _)| m == "POST")
            .cloned()
            .unwrap();
        let body: Value = serde_json::from_str(&post.2).unwrap();
        assert_eq!(body["Image"], "http://ctl/dist/metal-ipxe.iso");
        assert_eq!(body["Inserted"], true);
        assert!(insert_media(&target(&addr), "ftp://x/a.iso").await.is_err());
        assert!(insert_media(&target(&addr), "http://x/a b.iso")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn a_different_image_is_ejected_first_and_old_bmcs_are_patched() {
        let mut r = media_bmc();
        // Mounted, no InsertMedia action (older BMC): eject, then PATCH the slot.
        r.push(route(
            "GET",
            "/redfish/v1/Managers/1/VirtualMedia/CD1",
            200,
            json!({"MediaTypes": ["CD"], "Inserted": true, "Image": "http://old/x.iso"}),
        ));
        r.push(route(
            "POST",
            "/redfish/v1/Managers/1/VirtualMedia/CD1/Actions/VirtualMedia.EjectMedia",
            204,
            json!({}),
        ));
        r.push(route(
            "PATCH",
            "/redfish/v1/Managers/1/VirtualMedia/CD1",
            200,
            json!({}),
        ));
        let (addr, log) = mock_redfish(r).await;
        insert_media(&target(&addr), "http://ctl/dist/metal-ipxe.iso")
            .await
            .unwrap();
        let methods: Vec<String> = log
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, _, _)| m != "GET")
            .map(|(m, p, _)| format!("{m} {p}"))
            .collect();
        assert_eq!(
            methods,
            vec![
                "POST /redfish/v1/Managers/1/VirtualMedia/CD1/Actions/VirtualMedia.EjectMedia",
                "PATCH /redfish/v1/Managers/1/VirtualMedia/CD1",
            ]
        );
    }

    #[tokio::test]
    async fn eject_media_is_a_noop_when_nothing_is_mounted() {
        let mut r = media_bmc();
        r.push(route(
            "GET",
            "/redfish/v1/Managers/1/VirtualMedia/CD1",
            200,
            json!({"MediaTypes": ["DVD"], "Inserted": false}),
        ));
        let (addr, log) = mock_redfish(r).await;
        let out = eject_media(&target(&addr)).await.unwrap();
        assert!(out.contains("nothing mounted"), "{out}");
        assert!(log.lock().unwrap().iter().all(|(m, _, _)| m == "GET"));
    }

    #[tokio::test]
    async fn fake_driver_needs_no_hardware() {
        let t = BmcTarget {
            kind: BmcKind::Fake,
            address: "",
            username: "",
            password: "",
        };
        let s = power(&t, PowerAction::Cycle).await.unwrap();
        assert!(s.contains("fake BMC"));
    }
}
