// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::net::IpAddr;

use crate::state::CreateVmRequest;
use crate::LibvirtError;

/// Validate a VM/resource name: alphanumeric, dash, underscore, dot. 1-64 chars.
pub fn validate_name(name: &str) -> Result<(), LibvirtError> {
    if name.is_empty() {
        return Err(LibvirtError::Invalid("Name cannot be empty".to_string()));
    }
    if name.len() > 64 {
        return Err(LibvirtError::Invalid(
            "Name too long (max 64 characters)".to_string(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(LibvirtError::Invalid(
            "Name contains invalid characters (allowed: alphanumeric, dash, underscore, dot)"
                .to_string(),
        ));
    }
    if name.starts_with('-') || name.starts_with('.') {
        return Err(LibvirtError::Invalid(
            "Name cannot start with dash or dot".to_string(),
        ));
    }
    Ok(())
}

/// Validate vCPU count: 1-256.
pub fn validate_vcpus(vcpus: u32) -> Result<(), LibvirtError> {
    if vcpus == 0 || vcpus > 256 {
        return Err(LibvirtError::Invalid(
            "vCPUs must be between 1 and 256".to_string(),
        ));
    }
    Ok(())
}

/// Validate memory: 64 MB to 1 TB.
pub fn validate_memory_mb(memory_mb: u64) -> Result<(), LibvirtError> {
    if memory_mb < 64 {
        return Err(LibvirtError::Invalid(
            "Memory must be at least 64 MB".to_string(),
        ));
    }
    if memory_mb > 1_048_576 {
        return Err(LibvirtError::Invalid(
            "Memory cannot exceed 1 TB (1048576 MB)".to_string(),
        ));
    }
    Ok(())
}

/// Validate disk size: 1 GB to 10 TB.
pub fn validate_disk_gb(disk_gb: u64) -> Result<(), LibvirtError> {
    if disk_gb == 0 {
        return Err(LibvirtError::Invalid(
            "Disk size must be at least 1 GB".to_string(),
        ));
    }
    if disk_gb > 10_240 {
        return Err(LibvirtError::Invalid(
            "Disk size cannot exceed 10 TB (10240 GB)".to_string(),
        ));
    }
    Ok(())
}

/// VNC/SPICE `listen=` must be a plain IP (hyper2kvm-style); avoids XML injection and odd libvirt edge cases.
/// `vnc` (noVNC in web) or `spice` (spice-html5 + `/ws/v1/spice/...`).
pub fn validate_graphics_type(t: &str) -> Result<(), LibvirtError> {
    let t = t.trim().to_lowercase();
    if t == "vnc" || t == "spice" || t == "both" {
        return Ok(());
    }
    Err(LibvirtError::Invalid(
        "graphics_type must be 'vnc', 'spice', or 'both'".to_string(),
    ))
}

/// Single graphics device kind for virt-xml add/remove (not `both`).
pub fn validate_graphics_kind(t: &str) -> Result<(), LibvirtError> {
    let t = t.trim().to_lowercase();
    if t == "vnc" || t == "spice" {
        return Ok(());
    }
    Err(LibvirtError::Invalid(
        "graphics kind must be 'vnc' or 'spice'".to_string(),
    ))
}

pub fn validate_graphics_listen(listen: &str) -> Result<(), LibvirtError> {
    let listen = listen.trim();
    if listen.is_empty() {
        return Err(LibvirtError::Invalid(
            "graphics_listen cannot be empty".to_string(),
        ));
    }
    if listen.len() > 64 {
        return Err(LibvirtError::Invalid(
            "graphics_listen is too long".to_string(),
        ));
    }
    listen.parse::<IpAddr>().map_err(|_| {
        LibvirtError::Invalid(
            "graphics_listen must be a valid IP address (e.g. 127.0.0.1 or 0.0.0.0)".to_string(),
        )
    })?;
    Ok(())
}

/// Optional per-request override for `CreateVmRequest.create_backend`.
pub fn validate_create_backend_override(s: &str) -> Result<(), LibvirtError> {
    let t = s.trim();
    if t.is_empty() || t == "libvirt_xml" || t == "virt_install" {
        return Ok(());
    }
    Err(LibvirtError::Invalid(
        "create_backend must be empty, 'libvirt_xml', or 'virt_install'".into(),
    ))
}

/// `CreateVmRequest.template_disk_mode` when using a saved template with `base_image`.
pub fn validate_template_disk_mode(s: &str) -> Result<(), LibvirtError> {
    let t = s.trim();
    if t.is_empty() || t == "backing" || t == "copy" {
        return Ok(());
    }
    Err(LibvirtError::Invalid(
        "template_disk_mode must be empty, 'backing', or 'copy'".into(),
    ))
}

/// `virt-builder` template index name (e.g. `ubuntu-22.04`, `debian-12`).
pub fn validate_virt_builder_os(s: &str) -> Result<(), LibvirtError> {
    if s.is_empty() {
        return Err(LibvirtError::Invalid(
            "virt_builder_os cannot be empty".into(),
        ));
    }
    if s.len() > 128 {
        return Err(LibvirtError::Invalid(
            "virt_builder_os is too long (max 128)".into(),
        ));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(LibvirtError::Invalid(
            "virt_builder_os: allowed characters are letters, digits, dot, underscore, hyphen"
                .into(),
        ));
    }
    // `virt-builder` takes this as its first, bare positional argv element with no
    // preceding flag of its own (see virt_builder.rs::materialize_virt_builder_if_requested),
    // so a leading '-' would let it be parsed as a virt-builder option instead of the
    // template name (same flag-injection class as validate_login_username/validate_service_name).
    if s.starts_with('-') {
        return Err(LibvirtError::Invalid(
            "virt_builder_os must not start with '-'".into(),
        ));
    }
    Ok(())
}

/// Guest `--hostname` for virt-builder (DNS-style; no underscore).
pub fn validate_virt_builder_hostname(s: &str) -> Result<(), LibvirtError> {
    if s.is_empty() {
        return Err(LibvirtError::Invalid("hostname cannot be empty".into()));
    }
    if s.len() > 253 {
        return Err(LibvirtError::Invalid("hostname too long (max 253)".into()));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
    {
        return Err(LibvirtError::Invalid(
            "hostname: use letters, digits, hyphen and dot only".into(),
        ));
    }
    if s.starts_with('-')
        || s.ends_with('-')
        || s.starts_with('.')
        || s.ends_with('.')
        || s.contains("..")
    {
        return Err(LibvirtError::Invalid(
            "hostname: invalid leading/trailing punctuation or empty label".into(),
        ));
    }
    Ok(())
}

pub fn validate_virt_builder_ssh_pubkey_line(s: &str) -> Result<(), LibvirtError> {
    if s.len() > 16_384 {
        return Err(LibvirtError::Invalid(
            "SSH public key line is too long".into(),
        ));
    }
    if s.contains('\n') || s.contains('\r') {
        return Err(LibvirtError::Invalid(
            "virt_builder_ssh_pubkey must be a single line".into(),
        ));
    }
    let ok = s.starts_with("ssh-rsa ")
        || s.starts_with("ssh-ed25519 ")
        || s.starts_with("ssh-dss ")
        || s.starts_with("ecdsa-sha2-");
    if !ok {
        return Err(LibvirtError::Invalid(
            "virt_builder_ssh_pubkey must look like an OpenSSH public key (ssh-rsa, ssh-ed25519, ecdsa-sha2-*, ssh-dss)"
                .into(),
        ));
    }
    Ok(())
}

/// Host-local file used as `--root-password file:…` (must be absolute path to a small regular file).
pub fn validate_virt_builder_password_file(path: &str) -> Result<(), LibvirtError> {
    let p = path.trim();
    if p.is_empty() {
        return Err(LibvirtError::Invalid(
            "virt_builder_root_password_file cannot be empty if set".into(),
        ));
    }
    let pb = std::path::Path::new(p);
    if !pb.is_absolute() {
        return Err(LibvirtError::Invalid(
            "virt_builder_root_password_file must be an absolute path".into(),
        ));
    }
    let meta = std::fs::metadata(pb)
        .map_err(|e| LibvirtError::Invalid(format!("virt_builder_root_password_file: {e}")))?;
    if !meta.is_file() {
        return Err(LibvirtError::Invalid(
            "virt_builder_root_password_file must be a regular file".into(),
        ));
    }
    let len = meta.len();
    if len == 0 || len > 4096 {
        return Err(LibvirtError::Invalid(
            "virt_builder_root_password_file must be non-empty and at most 4096 bytes".into(),
        ));
    }
    Ok(())
}

const VB_LIST_MAX: usize = 64;
const VB_STR_MAX: usize = 2048;

fn validate_virt_builder_string_list(items: &[String], label: &str) -> Result<(), LibvirtError> {
    if items.len() > VB_LIST_MAX {
        return Err(LibvirtError::Invalid(format!(
            "{label}: at most {VB_LIST_MAX} entries"
        )));
    }
    for (i, s) in items.iter().enumerate() {
        if s.is_empty() {
            return Err(LibvirtError::Invalid(format!(
                "{label}: entry {i} is empty"
            )));
        }
        if s.len() > VB_STR_MAX {
            return Err(LibvirtError::Invalid(format!(
                "{label}: entry {i} exceeds {VB_STR_MAX} characters"
            )));
        }
        if s.contains('\0') {
            return Err(LibvirtError::Invalid(format!(
                "{label}: entry {i} contains NUL"
            )));
        }
    }
    Ok(())
}

/// Directory for `mkosi build` (must contain `mkosi.conf`).
pub fn validate_mkosi_workspace(path: &str) -> Result<(), LibvirtError> {
    let p = std::path::Path::new(path.trim());
    if !p.is_absolute() {
        return Err(LibvirtError::Invalid(
            "mkosi_workspace must be an absolute path".into(),
        ));
    }
    let meta =
        std::fs::metadata(p).map_err(|e| LibvirtError::Invalid(format!("mkosi_workspace: {e}")))?;
    if !meta.is_dir() {
        return Err(LibvirtError::Invalid(
            "mkosi_workspace must be a directory".into(),
        ));
    }
    if !p.join("mkosi.conf").is_file() {
        return Err(LibvirtError::Invalid(
            "mkosi_workspace must contain mkosi.conf (see systemd/mkosi)".into(),
        ));
    }
    Ok(())
}

/// Validates `virt_builder_*` vs `mkosi_workspace` mutual exclusion and each subsystem's fields.
pub fn validate_create_vm_disk_image_builders(req: &CreateVmRequest) -> Result<(), LibvirtError> {
    let vb = !req.virt_builder_os.trim().is_empty();
    let mk = !req.mkosi_workspace.trim().is_empty();
    if vb && mk {
        return Err(LibvirtError::Invalid(
            "Use either virt_builder_os or mkosi_workspace to build a root disk, not both".into(),
        ));
    }
    if vb {
        validate_create_vm_virt_builder_fields(req)?;
    }
    if mk {
        validate_mkosi_workspace(req.mkosi_workspace.trim())?;
        if !req.iso.trim().is_empty() {
            return Err(LibvirtError::Invalid(
                "mkosi_workspace produces a full root disk; omit install ISO (`iso`)".into(),
            ));
        }
    }
    validate_create_vm_virt_install_extensions(req)?;
    Ok(())
}

/// Mutual exclusion and basic sanity for Cockpit-style `virt-install` request fields.
pub fn validate_create_vm_virt_install_extensions(
    req: &CreateVmRequest,
) -> Result<(), LibvirtError> {
    let pool = req.root_disk_storage_pool.trim();
    let vol = req.root_disk_storage_volume.trim();
    if pool.is_empty() != vol.is_empty() {
        return Err(LibvirtError::Invalid(
            "Set both root_disk_storage_pool and root_disk_storage_volume, or neither".into(),
        ));
    }
    if !pool.is_empty() {
        validate_virt_install_field(pool, "root_disk_storage_pool")?;
        validate_virt_install_field(vol, "root_disk_storage_volume")?;
    }
    let pxe_net = req.virt_install_pxe_network.trim();
    if !pxe_net.is_empty() {
        validate_virt_install_field(pxe_net, "virt_install_pxe_network")?;
    }
    let loc = req.virt_install_location.trim();
    if !loc.is_empty() && (loc.len() > 4096 || loc.contains('\n') || loc.contains('\r')) {
        return Err(LibvirtError::Invalid(
            "virt_install_location: invalid or too long".into(),
        ));
    }
    let ios = req.virt_install_install_os.trim();
    if !ios.is_empty() {
        validate_virt_install_field(ios, "virt_install_install_os")?;
    }
    let extra = req.virt_install_extra_args.trim();
    if !extra.is_empty() && (extra.len() > 8192 || extra.contains('\n') || extra.contains('\r')) {
        return Err(LibvirtError::Invalid(
            "virt_install_extra_args: invalid or too long".into(),
        ));
    }
    let backing = req.virt_install_disk_backing_store.trim();
    if !backing.is_empty() {
        let p = std::path::Path::new(backing);
        if !p.is_absolute() || !p.is_file() {
            return Err(LibvirtError::Invalid(
                "virt_install_disk_backing_store must be an absolute path to an existing file"
                    .into(),
            ));
        }
    }

    if req.virt_install_define_only {
        if !req.mkosi_workspace.trim().is_empty() || !req.virt_builder_os.trim().is_empty() {
            return Err(LibvirtError::Invalid(
                "virt_install_define_only cannot be combined with mkosi_workspace or virt_builder_os"
                    .into(),
            ));
        }
        if !req.iso.trim().is_empty()
            || !loc.is_empty()
            || req.virt_install_pxe
            || !ios.is_empty()
            || !backing.is_empty()
            || !req.cloud_init_iso.trim().is_empty()
        {
            return Err(LibvirtError::Invalid(
                "virt_install_define_only cannot be combined with install media (iso, virt_install_location, PXE, virt_install_install_os, backing import, cloud_init_iso)".into(),
            ));
        }
    }

    if req.virt_install_pxe && (!req.iso.trim().is_empty() || !loc.is_empty() || !ios.is_empty()) {
        return Err(LibvirtError::Invalid(
                "virt_install_pxe cannot be combined with iso, virt_install_location, or virt_install_install_os"
                    .into(),
            ));
    }

    if !loc.is_empty() && !req.iso.trim().is_empty() {
        return Err(LibvirtError::Invalid(
            "Use either iso (CDROM) or virt_install_location (--location), not both".into(),
        ));
    }

    if !ios.is_empty() && (!req.iso.trim().is_empty() || !loc.is_empty() || req.virt_install_pxe) {
        return Err(LibvirtError::Invalid(
            "virt_install_install_os cannot be combined with iso, virt_install_location, or PXE"
                .into(),
        ));
    }

    if !pool.is_empty() && (!req.existing_disk.trim().is_empty() || !backing.is_empty()) {
        return Err(LibvirtError::Invalid(
                "root_disk_storage_pool/volume cannot be combined with existing_disk or virt_install_disk_backing_store".into(),
            ));
    }
    if !backing.is_empty() && !req.existing_disk.trim().is_empty() {
        return Err(LibvirtError::Invalid(
            "Use either existing_disk or virt_install_disk_backing_store, not both".into(),
        ));
    }
    if !backing.is_empty() && req.virt_install_define_only {
        return Err(LibvirtError::Invalid(
            "virt_install_disk_backing_store cannot be used with virt_install_define_only".into(),
        ));
    }

    Ok(())
}

fn validate_virt_install_field(s: &str, label: &str) -> Result<(), LibvirtError> {
    if s.is_empty() {
        return Ok(());
    }
    if !s
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(LibvirtError::Invalid(format!(
            "{label} may only contain letters, digits, dot, underscore, hyphen"
        )));
    }
    Ok(())
}

/// When `virt_builder_os` is set, validate optional related fields (HTTP handler).
pub fn validate_create_vm_virt_builder_fields(req: &CreateVmRequest) -> Result<(), LibvirtError> {
    let os = req.virt_builder_os.trim();
    if os.is_empty() {
        return Ok(());
    }
    validate_virt_builder_os(os)?;
    let h = req.virt_builder_hostname.trim();
    if !h.is_empty() {
        validate_virt_builder_hostname(h)?;
    }
    let pk = req.virt_builder_ssh_pubkey.trim();
    if !pk.is_empty() {
        validate_virt_builder_ssh_pubkey_line(pk)?;
    }
    let pwf = req.virt_builder_root_password_file.trim();
    if !pwf.is_empty() {
        validate_virt_builder_password_file(pwf)?;
    }
    validate_virt_builder_string_list(&req.virt_builder_packages, "virt_builder_packages")?;
    validate_virt_builder_string_list(
        &req.virt_builder_firstboot_commands,
        "virt_builder_firstboot_commands",
    )?;
    validate_virt_builder_string_list(
        &req.virt_builder_post_customize_install,
        "virt_builder_post_customize_install",
    )?;
    validate_virt_builder_string_list(
        &req.virt_builder_post_customize_run,
        "virt_builder_post_customize_run",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_name_valid() {
        assert!(validate_name("my-vm").is_ok());
        assert!(validate_name("vm_01").is_ok());
        assert!(validate_name("test.vm").is_ok());
        assert!(validate_name("a").is_ok());
    }

    #[test]
    fn test_validate_name_empty() {
        assert!(validate_name("").is_err());
    }

    #[test]
    fn test_validate_name_too_long() {
        let long = "a".repeat(65);
        assert!(validate_name(&long).is_err());
        assert!(validate_name(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn test_validate_name_invalid_chars() {
        assert!(validate_name("my vm").is_err());
        assert!(validate_name("vm@host").is_err());
        assert!(validate_name("foo/bar").is_err());
        assert!(validate_name("a<b").is_err());
    }

    #[test]
    fn test_validate_name_bad_start() {
        assert!(validate_name("-myvm").is_err());
        assert!(validate_name(".myvm").is_err());
    }

    #[test]
    fn test_validate_vcpus() {
        assert!(validate_vcpus(0).is_err());
        assert!(validate_vcpus(1).is_ok());
        assert!(validate_vcpus(256).is_ok());
        assert!(validate_vcpus(257).is_err());
    }

    #[test]
    fn test_validate_memory_mb() {
        assert!(validate_memory_mb(63).is_err());
        assert!(validate_memory_mb(64).is_ok());
        assert!(validate_memory_mb(1_048_576).is_ok());
        assert!(validate_memory_mb(1_048_577).is_err());
    }

    #[test]
    fn test_validate_disk_gb() {
        assert!(validate_disk_gb(0).is_err());
        assert!(validate_disk_gb(1).is_ok());
        assert!(validate_disk_gb(10_240).is_ok());
        assert!(validate_disk_gb(10_241).is_err());
    }

    #[test]
    fn test_validate_graphics_listen() {
        assert!(validate_graphics_listen("127.0.0.1").is_ok());
        assert!(validate_graphics_listen(" 0.0.0.0 ").is_ok());
        assert!(validate_graphics_listen("::1").is_ok());
        assert!(validate_graphics_listen("").is_err());
        assert!(validate_graphics_listen("eth0").is_err());
        assert!(validate_graphics_listen("10.0.0.1'><evil").is_err());
    }

    #[test]
    fn test_validate_graphics_type() {
        assert!(validate_graphics_type("vnc").is_ok());
        assert!(validate_graphics_type("SPICE").is_ok());
        assert!(validate_graphics_type("both").is_ok());
        assert!(validate_graphics_type(" rdp ").is_err());
        assert!(validate_graphics_kind("vnc").is_ok());
        assert!(validate_graphics_kind("both").is_err());
    }

    #[test]
    fn test_validate_create_backend_override() {
        assert!(validate_create_backend_override("").is_ok());
        assert!(validate_create_backend_override("virt_install").is_ok());
        assert!(validate_create_backend_override("bogus").is_err());
    }

    #[test]
    fn test_validate_template_disk_mode() {
        assert!(validate_template_disk_mode("backing").is_ok());
        assert!(validate_template_disk_mode("copy").is_ok());
        assert!(validate_template_disk_mode("raid").is_err());
    }

    #[test]
    fn test_validate_virt_builder_os() {
        assert!(validate_virt_builder_os("ubuntu-22.04").is_ok());
        assert!(validate_virt_builder_os("fedora-44").is_ok());
        assert!(validate_virt_builder_os("").is_err());
        assert!(validate_virt_builder_os("bad os").is_err());
    }

    #[test]
    fn test_validate_virt_builder_hostname() {
        assert!(validate_virt_builder_hostname("web-01").is_ok());
        assert!(validate_virt_builder_hostname("a.b").is_ok());
        assert!(validate_virt_builder_hostname("bad_host").is_err());
        assert!(validate_virt_builder_hostname("-x").is_err());
    }

    #[test]
    fn test_validate_virt_builder_ssh_pubkey_line() {
        assert!(
            validate_virt_builder_ssh_pubkey_line("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIfake")
                .is_ok()
        );
        assert!(validate_virt_builder_ssh_pubkey_line("not-a-key").is_err());
        assert!(validate_virt_builder_ssh_pubkey_line("ssh-ed25519 A\nB").is_err());
    }

    #[test]
    fn test_validate_virt_builder_password_file_rejects_relative() {
        assert!(validate_virt_builder_password_file("relative").is_err());
        assert!(validate_virt_builder_password_file("").is_err());
    }

    #[test]
    fn test_validate_virt_install_extensions_pool_pair() {
        let mut req = CreateVmRequest {
            name: "a".into(),
            root_disk_storage_pool: "p".into(),
            ..Default::default()
        };
        assert!(validate_create_vm_virt_install_extensions(&req).is_err());
        req.root_disk_storage_volume = "v".into();
        assert!(validate_create_vm_virt_install_extensions(&req).is_ok());
    }

    #[test]
    fn test_validate_virt_install_define_only_rejects_iso() {
        let req = CreateVmRequest {
            name: "a".into(),
            virt_install_define_only: true,
            iso: "/x.iso".into(),
            ..Default::default()
        };
        assert!(validate_create_vm_virt_install_extensions(&req).is_err());
    }

    #[test]
    fn test_validate_create_vm_disk_image_builders_mutex() {
        let mut req = CreateVmRequest {
            virt_builder_os: "fedora-44".into(),
            mkosi_workspace: "/tmp/w".into(),
            ..Default::default()
        };
        assert!(validate_create_vm_disk_image_builders(&req).is_err());
        req.mkosi_workspace = String::new();
        assert!(validate_create_vm_disk_image_builders(&req).is_ok());
    }
}
