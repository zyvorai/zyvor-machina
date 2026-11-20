// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};

use crate::error::SpecError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VirtualMachine {
    pub api_version: String,
    pub kind: String,
    pub metadata: VmMetadata,
    pub spec: VmSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VmMetadata {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VmSpec {
    #[serde(default)]
    pub profile: VmProfile,
    pub cpu: CpuSpec,
    pub memory: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_ref: Option<String>,
    #[serde(default)]
    pub storage: Vec<StorageVolumeSpec>,
    #[serde(default)]
    pub network: Vec<NetworkAttachmentSpec>,
    #[serde(default)]
    pub ha: HaSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<BackupSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_init: Option<CloudInitSpec>,
    #[serde(default = "default_firmware")]
    pub firmware: String,
    #[serde(default = "default_graphics")]
    pub graphics: GraphicsSpec,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum VmProfile {
    #[default]
    Development,
    Production,
    Lab,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CpuSpec {
    #[serde(default = "default_sockets")]
    pub sockets: u32,
    #[serde(default = "default_cores")]
    pub cores: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageVolumeSpec {
    pub name: String,
    pub size: String,
    #[serde(default = "default_storage_class")]
    pub class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkAttachmentSpec {
    pub network: String,
    #[serde(default = "default_ip_mode")]
    pub ip_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firewall_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HaSpec {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart_priority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anti_affinity: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudInitSpec {
    #[serde(default = "default_cloud_user")]
    pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_pubkey: Option<String>,
    /// Free-form cloud-init user-data (a `#cloud-config` document or a script), passed to the guest verbatim as the NoCloud
    /// `user-data`; Machina's own generated settings travel in `vendor-data` beside it. At most [`MAX_USER_DATA_BYTES`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_data: Option<String>,
}

/// EC2's user-data limit.
pub const MAX_USER_DATA_BYTES: usize = 16 * 1024;

/// Why this user-data cannot be used, if it cannot: empty, over the limit, or containing a NUL byte.
pub fn validate_user_data(data: &str) -> Result<(), SpecError> {
    if data.trim().is_empty() {
        return Err(SpecError::Validation("user_data must not be empty".into()));
    }
    if data.len() > MAX_USER_DATA_BYTES {
        return Err(SpecError::Validation(format!(
            "user_data is {} bytes; the limit is {MAX_USER_DATA_BYTES}",
            data.len()
        )));
    }
    if data.contains('\0') {
        return Err(SpecError::Validation(
            "user_data must not contain NUL bytes".into(),
        ));
    }
    Ok(())
}

// Never print credentials or user-data (which commonly carries secrets) into logs.
impl std::fmt::Debug for CloudInitSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudInitSpec")
            .field("user", &self.user)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("ssh_pubkey", &self.ssh_pubkey)
            .field(
                "user_data",
                &self
                    .user_data
                    .as_ref()
                    .map(|d| format!("<{} bytes>", d.len())),
            )
            .finish()
    }
}

fn default_cloud_user() -> String {
    "ubuntu".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupSpec {
    pub policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphicsSpec {
    #[serde(default = "default_graphics_type")]
    pub r#type: String,
    #[serde(default = "default_graphics_listen")]
    pub listen: String,
    /// Explicit, clearly-risky opt-in required to bind `listen` to a
    /// non-loopback address. `translate::domain_xml::graphics_block` does
    /// NOT set a libvirt-native console `passwd=` by default (see
    /// `domain_xml_from_spec`'s doc comment for why: the agent's own console
    /// proxy doesn't speak RFB/SPICE and the web VNCViewer always answers
    /// auth challenges with an empty password, so a real `passwd=` would
    /// break every in-app console today) — so a non-loopback listener
    /// exposes a completely unauthenticated VNC/SPICE console on the open
    /// network, bypassing the agent's token-gated console proxy entirely.
    /// Default false: `listen` must be loopback unless this is set.
    #[serde(default)]
    pub allow_public_listen: bool,
}

fn default_firmware() -> String {
    "bios".into()
}
fn default_graphics() -> GraphicsSpec {
    GraphicsSpec {
        r#type: default_graphics_type(),
        listen: default_graphics_listen(),
        allow_public_listen: false,
    }
}
fn default_graphics_type() -> String {
    "both".into()
}
fn default_graphics_listen() -> String {
    "127.0.0.1".into()
}
fn default_sockets() -> u32 {
    1
}
fn default_cores() -> u32 {
    1
}
fn default_storage_class() -> String {
    "silver".into()
}
fn default_ip_mode() -> String {
    "dhcp".into()
}

impl VirtualMachine {
    pub fn new(name: impl Into<String>, memory: impl Into<String>) -> Self {
        Self {
            api_version: crate::API_VERSION.into(),
            kind: "VirtualMachine".into(),
            metadata: VmMetadata {
                name: name.into(),
                project: None,
                labels: None,
            },
            spec: VmSpec {
                profile: VmProfile::default(),
                cpu: CpuSpec {
                    sockets: 1,
                    cores: 1,
                },
                memory: memory.into(),
                template_ref: None,
                cloud_init: None,
                storage: vec![StorageVolumeSpec {
                    name: "root".into(),
                    size: "10Gi".into(),
                    class: "silver".into(),
                    source: None,
                }],
                network: vec![NetworkAttachmentSpec {
                    network: "default".into(),
                    ip_mode: "dhcp".into(),
                    firewall_profile: None,
                }],
                ha: HaSpec::default(),
                backup: None,
                firmware: default_firmware(),
                graphics: default_graphics(),
            },
        }
    }

    pub fn validate(&self) -> Result<(), SpecError> {
        validate_name(&self.metadata.name)?;
        if self.spec.cpu.sockets == 0 || self.spec.cpu.cores == 0 {
            return Err(SpecError::Validation(
                "cpu sockets and cores must be >= 1".into(),
            ));
        }
        if self.spec.cpu.sockets > 64 || self.spec.cpu.cores > 128 {
            return Err(SpecError::Validation(
                "cpu sockets must be <= 64 and cores must be <= 128".into(),
            ));
        }
        parse_memory_mib(&self.spec.memory)?;
        if let Some(data) = self
            .spec
            .cloud_init
            .as_ref()
            .and_then(|c| c.user_data.as_deref())
        {
            validate_user_data(data)?;
        }
        if self.spec.storage.is_empty() {
            return Err(SpecError::Validation(
                "at least one storage volume required".into(),
            ));
        }
        for vol in &self.spec.storage {
            validate_name(&vol.name)?;
            let size_gib = parse_size_gib(&vol.size)?;
            if size_gib == 0 {
                return Err(SpecError::Validation(format!(
                    "storage volume '{}' size must be greater than 0",
                    vol.name
                )));
            }
            if size_gib > MAX_STORAGE_GIB {
                return Err(SpecError::Validation(format!(
                    "storage volume '{}' size exceeds maximum of {MAX_STORAGE_GIB} GiB",
                    vol.name
                )));
            }
            if let Some(source) = vol.source.as_deref() {
                validate_storage_source(source)?;
            }
        }
        if self.spec.network.is_empty() {
            return Err(SpecError::Validation(
                "at least one network attachment required".into(),
            ));
        }
        if let Some(labels) = &self.metadata.labels {
            if let Some(iso) = labels.get("install_iso") {
                validate_install_iso_path(iso)?;
            }
        }
        if !self.spec.graphics.allow_public_listen
            && !is_loopback_listen(&self.spec.graphics.listen)
        {
            return Err(SpecError::Validation(
                "graphics.listen must be loopback (127.0.0.1/::1/localhost) unless \
                 graphics.allow_public_listen is explicitly set to true — a non-loopback \
                 listener exposes an unauthenticated VNC/SPICE console on the network"
                    .into(),
            ));
        }
        Ok(())
    }

    /// Stricter validation for a spec as originally submitted by an operator
    /// (i.e. before any trusted server-side rewrite such as Atlas's
    /// `rbd_source()` resolution). In addition to every `validate()` check,
    /// this outright rejects an inline Ceph `auth=`/`secret=`/`secret_uuid=`
    /// parameter on any storage `source` — those parameters are only ever
    /// legitimate when written by the server itself from cluster-wide config
    /// (see `controller/src/engine/atlas_vm.rs::rbd_source`), never when
    /// supplied directly by the caller. Call this at the one place a raw
    /// operator request body is first validated (`controller/src/api/vms/mod.rs`,
    /// before the Atlas branch resolves and overwrites `spec.storage[].source`).
    /// Do NOT use this for already-resolved specs (e.g. inside
    /// `translate::domain_xml_from_spec`) — a legitimate Atlas-backed VM's
    /// resolved source legitimately carries `secret=<uuid>` and would fail
    /// this check.
    pub fn validate_operator_submission(&self) -> Result<(), SpecError> {
        self.validate()?;
        for vol in &self.spec.storage {
            if let Some(source) = vol.source.as_deref() {
                reject_inline_ceph_auth(source)?;
            }
        }
        Ok(())
    }

    pub fn total_vcpus(&self) -> u32 {
        self.spec.cpu.sockets.saturating_mul(self.spec.cpu.cores)
    }

    pub fn memory_mib(&self) -> Result<u64, SpecError> {
        parse_memory_mib(&self.spec.memory)
    }

    pub fn root_disk_gib(&self) -> Result<u64, SpecError> {
        // Prefer a volume literally named "root"; otherwise fall back to the first
        // volume. validate() only requires >=1 volume (not one named "root"), so
        // without this fallback a validated spec whose sole disk is named e.g.
        // "os"/"data" passed validation yet failed translation with "root volume
        // required".
        self.spec
            .storage
            .iter()
            .find(|v| v.name == "root")
            .or_else(|| self.spec.storage.first())
            .map(|v| parse_size_gib(&v.size))
            .transpose()?
            .ok_or_else(|| SpecError::Validation("root volume required".into()))
    }
}

/// Validate an `rbd:`/`rbd://` storage `source`'s inline Ceph auth params.
///
/// `translate::domain_xml::rbd_disk_xml` renders `secret=<uuid>`/`auth=<user>`
/// query parameters verbatim into `<auth username='..'><secret type='ceph'
/// uuid='..'/></auth>`. This is called from `validate()`, which runs on both
/// operator-original specs AND already-resolved specs (e.g. inside
/// `translate::domain_xml_from_spec` at VM-apply time) — a legitimate
/// Atlas-backed VM's resolved source legitimately carries `secret=<uuid>` at
/// that point, so this check only performs the narrower, always-safe check:
/// any `secret=`/`secret_uuid=` value must be syntactically a well-formed
/// UUID. The stronger check — outright rejecting inline auth/secret params
/// from a caller who shouldn't be supplying them at all — lives in
/// `reject_inline_ceph_auth`, used only by `validate_operator_submission()`
/// at the one call site that sees a spec before Atlas resolution
/// (`controller/src/api/vms/mod.rs`).
fn validate_storage_source(source: &str) -> Result<(), SpecError> {
    let Some(rest) = source
        .strip_prefix("rbd://")
        .or_else(|| source.strip_prefix("rbd:"))
    else {
        return Ok(());
    };
    let Some((_, query)) = rest.split_once('?') else {
        return Ok(());
    };
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if matches!(key, "secret" | "secret_uuid") && !looks_like_uuid(value) {
            return Err(SpecError::Validation(
                "storage volume source's secret/secret_uuid parameter must be a \
                 well-formed UUID"
                    .into(),
            ));
        }
    }
    Ok(())
}

/// Reject a storage `source` outright if it carries an inline `auth=`/
/// `secret=`/`secret_uuid=` Ceph parameter. Used only for operator-submitted
/// specs (see `VirtualMachine::validate_operator_submission`) — these
/// parameters have no legitimate reason to appear in a request body; they are
/// written by the server itself (Atlas resolution) strictly after this check
/// runs.
fn reject_inline_ceph_auth(source: &str) -> Result<(), SpecError> {
    let Some(rest) = source
        .strip_prefix("rbd://")
        .or_else(|| source.strip_prefix("rbd:"))
    else {
        return Ok(());
    };
    let Some((_, query)) = rest.split_once('?') else {
        return Ok(());
    };
    for pair in query.split('&') {
        let Some((key, _value)) = pair.split_once('=') else {
            continue;
        };
        if matches!(key, "auth" | "secret" | "secret_uuid") {
            return Err(SpecError::Validation(
                "storage volume source may not include an inline auth/secret/secret_uuid \
                 parameter — Ceph-backed volumes must be provisioned via the Atlas storage \
                 integration (atlas_root_disk: true), not supplied directly"
                    .into(),
            ));
        }
    }
    Ok(())
}

/// Loosely validate the canonical 8-4-4-4-12 hyphenated-hex UUID form. `spec`
/// has no dependency on the `uuid` crate (kept dependency-free for reuse
/// across daemon/controller/agent), so this is a small manual check.
fn looks_like_uuid(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    bytes.iter().enumerate().all(|(i, b)| {
        if matches!(i, 8 | 13 | 18 | 23) {
            *b == b'-'
        } else {
            b.is_ascii_hexdigit()
        }
    })
}

/// Host directories install-media ISOs (`metadata.labels["install_iso"]`) are
/// allowed to live under. `translate::domain_xml_from_spec` reads this label
/// and emits `<source file='{path}'/>` for a read-only CD-ROM with no
/// filesystem confinement of its own, so an unrestricted path here would let
/// any operator mount an arbitrary host-readable file (e.g. `/etc/shadow`,
/// another project's backup) into their guest.
///
/// `spec` intentionally has no dependency on `machina-core`/filesystem config
/// (kept dependency-free for reuse across daemon/controller/agent), so this
/// mirrors — rather than reads live — the conventional machina-owned image
/// directories also used by `core::libvirt::storage::collect_image_scan_directories`
/// and the default `[libvirt] iso_upload_dir` (`/var/lib/libvirt/images/isos`).
/// A custom (non-default) `iso_upload_dir` configured outside these prefixes
/// would need this list threaded through from config — flagged as a
/// follow-up, not fixed here to keep this change centralized and scope-bound.
const ALLOWED_INSTALL_ISO_PREFIXES: &[&str] =
    &["/var/lib/libvirt/images/", "/var/lib/machina/images/"];

fn validate_install_iso_path(path: &str) -> Result<(), SpecError> {
    let p = path.trim();
    if p.is_empty() {
        // Handled by the caller's `.filter(|s| !s.is_empty())`-style checks
        // elsewhere, but stay defensive here too.
        return Ok(());
    }
    if !p.starts_with('/') {
        return Err(SpecError::Validation(
            "install_iso must be an absolute path".into(),
        ));
    }
    // Lexical traversal guard: the path may name a file on a remote agent
    // host's filesystem, so this can't be resolved with `fs::canonicalize`
    // (nothing to canonicalize against locally) — reject any ".." segment
    // instead, which defeats the `/allowed/dir/../../etc/passwd` trick.
    if p.split('/').any(|seg| seg == "..") {
        return Err(SpecError::Validation(
            "install_iso must not contain '..' path segments".into(),
        ));
    }
    if !ALLOWED_INSTALL_ISO_PREFIXES
        .iter()
        .any(|prefix| p.starts_with(prefix))
    {
        return Err(SpecError::Validation(format!(
            "install_iso must be located under one of: {}",
            ALLOWED_INSTALL_ISO_PREFIXES.join(", ")
        )));
    }
    Ok(())
}

/// True if `addr` is a loopback hostname or parses as a loopback IP. An empty
/// string is deliberately NOT treated as loopback: `domain_xml_from_spec`
/// embeds this value verbatim as `listen='{addr}'`, and libvirt/qemu resolve
/// an empty/omitted listen address via `qemu.conf`'s `vnc_listen`/
/// `spice_listen`, which on some hosts defaults to `0.0.0.0` — silently
/// treating `""` as safe here would let a caller bypass the loopback-only
/// default without ever setting `allow_public_listen`. This mirrors
/// `core::validate::validate_graphics_listen`, which also rejects empty.
fn is_loopback_listen(addr: &str) -> bool {
    let a = addr.trim();
    if a.is_empty() {
        return false;
    }
    if a.eq_ignore_ascii_case("localhost") {
        return true;
    }
    a.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

pub fn validate_name(name: &str) -> Result<(), SpecError> {
    if name.is_empty() || name.len() > 64 {
        return Err(SpecError::InvalidName(name.to_string()));
    }
    if !name
        .chars()
        .next()
        .map(|c| c.is_ascii_alphanumeric())
        .unwrap_or(false)
    {
        return Err(SpecError::InvalidName(name.to_string()));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(SpecError::InvalidName(name.to_string()));
    }
    Ok(())
}

/// Human-readable label for UI-only resources (application groups, blueprints).
pub fn validate_label(name: &str) -> Result<(), SpecError> {
    let name = name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err(SpecError::InvalidName(name.to_string()));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(SpecError::InvalidName(name.to_string()));
    }
    if name.contains('/') || name.contains('\\') || name.contains('<') || name.contains('>') {
        return Err(SpecError::InvalidName(name.to_string()));
    }
    Ok(())
}

/// Upper bound on parsed memory (64 TiB in MiB). Beyond rejecting absurd specs,
/// this keeps downstream KiB math (`memory_mib * 1024`) from overflowing u64.
const MAX_MEMORY_MIB: u64 = 64 * 1024 * 1024;

/// Upper bound on a single storage volume's size (16 TiB in GiB). Unlike
/// memory, `parse_size_gib` can't itself overflow (it parses straight to
/// `u64`, so an out-of-range literal fails to parse rather than saturating),
/// but nothing capped the *value* until now: `agent::libvirt_ops::apply_vm`
/// runs only `VirtualMachine::validate()` before sizing the on-disk qcow2 via
/// `qemu-img create ... <N>G` (see `create_qcow2`) — an unbounded size here
/// let an operator request an absurdly large (e.g. exabyte-scale) disk file,
/// exhausting host storage/inodes. `parse_size_gib`'s `0` is rejected too,
/// since libvirt/qemu-img reject a zero-size disk.
const MAX_STORAGE_GIB: u64 = 16 * 1024;

pub fn parse_memory_mib(raw: &str) -> Result<u64, SpecError> {
    let s = raw.trim();
    // f64 parse accepts "inf"/"nan"; reject them and negatives up front so they
    // can't saturate to u64::MAX (inf) or 0 (nan) via `as u64`.
    let parse_gib = |num: &str| -> Result<u64, SpecError> {
        let n: f64 = num
            .trim()
            .parse()
            .map_err(|e| SpecError::Memory(format!("{raw}: {e}")))?;
        if !n.is_finite() || n < 0.0 {
            return Err(SpecError::Memory(format!("{raw}: invalid memory value")));
        }
        Ok((n * 1024.0).round() as u64)
    };
    let mib = if let Some(num) = s.strip_suffix("Gi") {
        parse_gib(num)?
    } else if let Some(num) = s.strip_suffix("Mi") {
        num.trim()
            .parse::<u64>()
            .map_err(|e| SpecError::Memory(format!("{raw}: {e}")))?
    } else if let Some(num) = s.strip_suffix("G") {
        parse_gib(num)?
    } else {
        s.parse::<u64>()
            .map_err(|e| SpecError::Memory(format!("{raw}: {e}")))?
    };
    // Reject non-positive memory (e.g. "0", "0Gi", or a negative like "-5Gi"
    // which saturates to 0 via `as u64`); libvirt rejects <memory>0</memory>.
    if mib == 0 {
        return Err(SpecError::Memory(format!(
            "{raw}: memory must be greater than 0"
        )));
    }
    if mib > MAX_MEMORY_MIB {
        return Err(SpecError::Memory(format!(
            "{raw}: memory exceeds maximum of {MAX_MEMORY_MIB} MiB"
        )));
    }
    Ok(mib)
}

pub fn parse_size_gib(raw: &str) -> Result<u64, SpecError> {
    let s = raw.trim();
    if let Some(num) = s.strip_suffix("Gi") {
        return num
            .trim()
            .parse()
            .map_err(|e| SpecError::Memory(format!("{raw}: {e}")));
    }
    if let Some(num) = s.strip_suffix("G") {
        return num
            .trim()
            .parse()
            .map_err(|e| SpecError::Memory(format!("{raw}: {e}")));
    }
    s.parse::<u64>()
        .map_err(|e| SpecError::Memory(format!("{raw}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_validate_golden_path() {
        let vm = VirtualMachine::new("payroll-server", "32Gi");
        vm.validate().unwrap();
        assert_eq!(vm.total_vcpus(), 1);
        assert_eq!(vm.memory_mib().unwrap(), 32 * 1024);
    }

    #[test]
    fn validate_rejects_zero_size_storage_volume() {
        let mut vm = VirtualMachine::new("zerodisk", "2Gi");
        vm.spec.storage[0].size = "0Gi".into();
        assert!(vm.validate().is_err());
    }

    #[test]
    fn validate_rejects_oversized_storage_volume() {
        // Nothing capped the parsed size before; an operator could request an
        // exabyte-scale qcow2 file (`agent::libvirt_ops::create_qcow2` runs
        // `qemu-img create ... <N>G` with only `validate()` in front of it).
        let mut vm = VirtualMachine::new("hugedisk", "2Gi");
        vm.spec.storage[0].size = "99999999Gi".into();
        assert!(vm.validate().is_err());
    }

    #[test]
    fn validate_accepts_reasonable_storage_volume() {
        let mut vm = VirtualMachine::new("bigdisk", "2Gi");
        vm.spec.storage[0].size = "4096Gi".into();
        assert!(vm.validate().is_ok());
    }

    #[test]
    fn parse_memory_units() {
        assert_eq!(parse_memory_mib("512Mi").unwrap(), 512);
        assert_eq!(parse_memory_mib("2Gi").unwrap(), 2048);
    }

    #[test]
    fn parse_memory_rejects_overflow_and_non_finite() {
        // Previously "infGi" saturated to u64::MAX and slipped past the ==0
        // guard, overflowing downstream memory_mib*1024.
        assert!(parse_memory_mib("infGi").is_err());
        assert!(parse_memory_mib("NaNGi").is_err());
        assert!(parse_memory_mib("-5Gi").is_err());
        assert!(parse_memory_mib("0Gi").is_err());
        assert!(parse_memory_mib("18446744073709551615Mi").is_err()); // u64::MAX Mi > cap
        assert!(parse_memory_mib("99999999Gi").is_err()); // > 64 TiB
                                                          // Sane values still parse.
        assert_eq!(parse_memory_mib("1.5Gi").unwrap(), 1536);
    }

    #[test]
    fn validate_label_allows_friendly_names() {
        assert!(validate_label("Finance Application").is_ok());
        assert!(validate_label("  Payroll & HR  ").is_ok());
        assert!(validate_label("").is_err());
        assert!(validate_label("bad/name").is_err());
    }

    #[test]
    fn validate_rejects_malformed_rbd_secret_uuid() {
        let mut vm = VirtualMachine::new("cephvm", "2Gi");
        vm.spec.storage[0].source = Some("rbd:pool/img?auth=machina&secret=1a2b3c".into());
        assert!(vm.validate().is_err());
    }

    #[test]
    fn validate_accepts_wellformed_rbd_secret_uuid() {
        let mut vm = VirtualMachine::new("cephvm", "2Gi");
        vm.spec.storage[0].source =
            Some("rbd:pool/img?auth=machina&secret=1a2b3c4d-5e6f-7890-abcd-ef1234567890".into());
        assert!(vm.validate().is_ok());
    }

    #[test]
    fn validate_rejects_install_iso_outside_allowed_dirs() {
        let mut vm = VirtualMachine::new("installer", "4Gi");
        vm.metadata.labels = Some(std::collections::HashMap::from([(
            "install_iso".into(),
            "/etc/shadow".into(),
        )]));
        assert!(vm.validate().is_err());
    }

    #[test]
    fn validate_rejects_install_iso_traversal() {
        let mut vm = VirtualMachine::new("installer", "4Gi");
        vm.metadata.labels = Some(std::collections::HashMap::from([(
            "install_iso".into(),
            "/var/lib/libvirt/images/isos/../../../etc/shadow".into(),
        )]));
        assert!(vm.validate().is_err());
    }

    #[test]
    fn validate_accepts_install_iso_under_allowed_dir() {
        let mut vm = VirtualMachine::new("installer", "4Gi");
        vm.metadata.labels = Some(std::collections::HashMap::from([(
            "install_iso".into(),
            "/var/lib/libvirt/images/isos/ubuntu.iso".into(),
        )]));
        assert!(vm.validate().is_ok());
    }

    #[test]
    fn validate_rejects_public_graphics_listen_by_default() {
        let mut vm = VirtualMachine::new("vncvm", "2Gi");
        vm.spec.graphics.listen = "0.0.0.0".into();
        assert!(vm.validate().is_err());
    }

    #[test]
    fn validate_allows_public_graphics_listen_with_explicit_opt_in() {
        let mut vm = VirtualMachine::new("vncvm", "2Gi");
        vm.spec.graphics.listen = "0.0.0.0".into();
        vm.spec.graphics.allow_public_listen = true;
        assert!(vm.validate().is_ok());
    }

    #[test]
    fn validate_allows_loopback_graphics_listen() {
        let mut vm = VirtualMachine::new("vncvm", "2Gi");
        vm.spec.graphics.listen = "::1".into();
        assert!(vm.validate().is_ok());
    }

    #[test]
    fn validate_rejects_empty_graphics_listen_without_opt_in() {
        // An empty `listen` must NOT be silently treated as loopback: libvirt/qemu
        // resolve an empty/omitted listen address via qemu.conf's vnc_listen/
        // spice_listen, which can default to 0.0.0.0 on some hosts.
        let mut vm = VirtualMachine::new("vncvm", "2Gi");
        vm.spec.graphics.listen = "".into();
        assert!(vm.validate().is_err());
        vm.spec.graphics.allow_public_listen = true;
        assert!(vm.validate().is_ok());
    }

    #[test]
    fn user_data_is_bounded_and_never_debug_printed() {
        assert!(validate_user_data("#cloud-config\npackages: [nginx]\n").is_ok());
        assert!(validate_user_data("   \n").is_err());
        assert!(validate_user_data(&"x".repeat(MAX_USER_DATA_BYTES)).is_ok());
        assert!(validate_user_data(&"x".repeat(MAX_USER_DATA_BYTES + 1)).is_err());
        assert!(validate_user_data("a\0b").is_err());
        let ci = CloudInitSpec {
            user: "u".into(),
            password: Some("hunter2".into()),
            ssh_pubkey: None,
            user_data: Some("#!/bin/sh\necho secret-token".into()),
        };
        let shown = format!("{ci:?}");
        assert!(
            !shown.contains("hunter2") && !shown.contains("secret-token"),
            "{shown}"
        );
        assert!(shown.contains("<redacted>") && shown.contains("bytes"));
    }
}
