// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::path::Path;
use std::process::Command;

use virt::connect::Connect;
use virt::domain::Domain;

use crate::config::{LibvirtConfig, VmCreateBackend};
use crate::state::CreateVmRequest;
use crate::LibvirtError;

use super::subprocess::{self, VmCreateLogSink};

/// Maximum vCPU count to declare at creation so online CPU hotplug (`set_vcpus` with
/// AFFECT_LIVE) can hot-add without a reboot. libvirt forbids raising vCPUs above the
/// domain's defined maximum, so a VM created with max == current can never grow live.
/// Headroom is 4x the boot count, capped at 16, and never below the boot count.
pub(crate) fn vcpu_max_for(vcpus: u32) -> u32 {
    vcpus.max(vcpus.saturating_mul(4).min(16))
}

/// [`CreateVmRequest::mkosi_workspace`] set means a Bootable=yes style image (EFI/GPT); BIOS would hang at SeaBIOS.
fn ensure_uefi_for_mkosi_workspace(req: &mut CreateVmRequest) {
    if req.mkosi_workspace.trim().is_empty() {
        return;
    }
    let fw = req.firmware.trim();
    if fw.is_empty() || fw.eq_ignore_ascii_case("bios") {
        tracing::info!(
            "mkosi_workspace set: using firmware=uefi (mkosi bootable disks use systemd-boot/EFI, not legacy BIOS)"
        );
        req.firmware = "uefi".into();
    }
}

fn is_fedora_mkosi_path(ws: &str) -> bool {
    let lower = ws.to_ascii_lowercase();
    Path::new(ws)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|b| b.to_ascii_lowercase().starts_with("fedora"))
        .unwrap_or(false)
        || lower.contains("/fedora")
}

fn fedora_os_variant_from_mkosi_path(ws: &str) -> Option<String> {
    let base = Path::new(ws).file_name()?.to_str()?.to_ascii_lowercase();
    if base.starts_with("fedora") {
        return Some(base.chars().take(32).collect());
    }
    let lower = ws.to_ascii_lowercase();
    let i = lower.rfind("fedora")?;
    let tail: String = lower[i..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    if tail.len() >= 6 {
        Some(tail)
    } else {
        None
    }
}

/// Fedora Bootable=yes images are happier with 2 vCPU / 2 GiB+; match UI defaults when the request still has serde defaults.
fn apply_fedora_mkosi_resource_defaults(req: &mut CreateVmRequest) {
    let ws = req.mkosi_workspace.trim();
    if ws.is_empty() || !is_fedora_mkosi_path(ws) {
        return;
    }
    if req.vcpus == 1 && req.memory_mb == 1024 {
        tracing::info!(
            "mkosi Fedora workspace: applying 2 vCPUs / 2048 MiB RAM / min 20 GiB disk hint (was API defaults 1/1024/10)"
        );
        req.vcpus = 2;
        req.memory_mb = 2048;
        if req.disk_gb < 20 {
            req.disk_gb = 20;
        }
    }
    let ov = req.os_variant.trim();
    if ov.is_empty() || ov.eq_ignore_ascii_case("generic") {
        if let Some(v) = fedora_os_variant_from_mkosi_path(ws) {
            req.os_variant = v;
        }
    }
}

/// Define a new VM using either native libvirt XML or external `virt-install` (see `[libvirt] create_backend`; default is usually `virt_install`).
pub fn create_vm(
    conn: &Connect,
    req: &CreateVmRequest,
    backend: VmCreateBackend,
    libvirt_uri: &str,
    libvirt_cfg: &LibvirtConfig,
    log: Option<&VmCreateLogSink>,
) -> Result<(), LibvirtError> {
    crate::validate::validate_name(&req.name)?;
    let mut req = req.clone();
    super::template_apply::apply_saved_template(conn, &mut req)?;

    // Auto-detect mkosi workspace when the user provided no boot source at all.
    // This avoids creating a blank disk that can never boot.
    let has_boot_source = !req.existing_disk.trim().is_empty()
        || !req.iso.trim().is_empty()
        || !req.virt_builder_os.trim().is_empty()
        || !req.mkosi_workspace.trim().is_empty()
        || !req.virt_install_location.trim().is_empty()
        || req.virt_install_pxe
        || !req.virt_install_install_os.trim().is_empty()
        || !req.virt_install_disk_backing_store.trim().is_empty();
    if !has_boot_source && libvirt_cfg.mkosi_allowed && !req.virt_install_define_only {
        if let Some(ws) = super::mkosi::auto_detect_workspace(&req.name) {
            tracing::info!(
                "Auto-detected mkosi workspace '{}' for VM '{}'",
                ws,
                req.name
            );
            req.mkosi_workspace = ws;
        }
    }

    // Bootable=yes mkosi recipes ship GPT + systemd-boot; SeaBIOS cannot boot them.
    ensure_uefi_for_mkosi_workspace(&mut req);
    apply_fedora_mkosi_resource_defaults(&mut req);

    subprocess::log_line(
        log,
        "machina",
        "Preparing disk image (mkosi / virt-builder / blank)…",
    );
    // Recorded before materialization so a failed define can clean up only what
    // THIS call created — never a path the caller supplied. Without this, a
    // failed virt-install/libvirt-XML define left the freshly materialized
    // mkosi/virt-builder disk or cloud-init seed on disk with nothing pointing
    // at it; repeated failed creates leaked images into the pool directory.
    let existing_disk_was_supplied = !req.existing_disk.trim().is_empty();
    let cloud_init_iso_was_supplied = !req.cloud_init_iso.trim().is_empty();
    super::mkosi::materialize_mkosi_if_requested(conn, &mut req, libvirt_cfg, log)?;
    super::virt_builder::materialize_virt_builder_if_requested(conn, &mut req, libvirt_cfg, log)?;
    super::cloud_init::materialize_cloud_init_seed_if_requested(&mut req, libvirt_cfg, log)?;
    if !req.existing_disk.trim().is_empty() {
        super::guest_agent_provision::inject_guestkit_into_disk(
            req.existing_disk.trim(),
            Some(libvirt_cfg),
            log,
        )?;
    }
    let r = match backend {
        VmCreateBackend::VirtInstall => {
            subprocess::log_line(log, "machina", "Defining VM with virt-install…");
            super::virt_install::create_vm_virt_install(conn, &req, libvirt_uri, log)
        }
        VmCreateBackend::LibvirtXml => {
            if super::virt_install::create_request_uses_virt_install_extensions(&req) {
                return Err(LibvirtError::Invalid(
                    "This request uses virt-install-only fields (virt_install_define_only, virt_install_location, virt_install_pxe, root_disk_storage_pool/volume, virt_install_disk_backing_store). Set create_backend to \"virt_install\" or omit it when the server default is virt_install."
                        .into(),
                ));
            }
            subprocess::log_line(log, "machina", "Defining VM with libvirt XML…");
            create_vm_libvirt_xml(conn, &req, libvirt_cfg, log)
        }
    };
    if r.is_ok() {
        if let Ok(missing) = super::domain::missing_file_disk_paths(conn, &req.name) {
            if !missing.is_empty() {
                subprocess::log_line(
                    log,
                    "machina",
                    &format!(
                        "WARNING: VM '{}' is defined but file-backed disk(s) are missing on the host — start will fail until you create them or fix paths in the domain XML: {}",
                        req.name,
                        missing.join(", ")
                    ),
                );
            }
        }
    } else {
        if !existing_disk_was_supplied && !req.existing_disk.trim().is_empty() {
            let p = req.existing_disk.trim();
            match std::fs::remove_file(p) {
                Ok(()) => subprocess::log_line(
                    log,
                    "machina",
                    &format!("Define failed — removed the freshly materialized disk {p}"),
                ),
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    tracing::warn!("VM '{}': define failed and could not remove the materialized disk {p}: {e}", req.name);
                }
                Err(_) => {}
            }
        }
        if !cloud_init_iso_was_supplied && !req.cloud_init_iso.trim().is_empty() {
            let p = req.cloud_init_iso.trim();
            match std::fs::remove_file(p) {
                Ok(()) => subprocess::log_line(
                    log,
                    "machina",
                    &format!("Define failed — removed the freshly generated cloud-init seed {p}"),
                ),
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    tracing::warn!(
                        "VM '{}': define failed and could not remove the cloud-init seed {p}: {e}",
                        req.name
                    );
                }
                Err(_) => {}
            }
        }
    }
    r
}

fn create_vm_libvirt_xml(
    conn: &Connect,
    req: &CreateVmRequest,
    libvirt_cfg: &crate::config::LibvirtConfig,
    log: Option<&VmCreateLogSink>,
) -> Result<(), LibvirtError> {
    crate::validate::validate_vcpus(req.vcpus)?;
    crate::validate::validate_memory_mb(req.memory_mb)?;

    let firmware = if req.firmware.is_empty() {
        "bios"
    } else {
        &req.firmware
    };
    if firmware != "bios" && firmware != "uefi" {
        return Err(LibvirtError::Invalid(
            "Firmware must be 'bios' or 'uefi'".to_string(),
        ));
    }

    let gl = req.graphics_listen.trim();
    let gl = if gl.is_empty() { "127.0.0.1" } else { gl };
    crate::validate::validate_graphics_listen(gl)?;

    let gt = req.graphics_type.trim();
    let gt = if gt.is_empty() { "vnc" } else { gt };
    crate::validate::validate_graphics_type(gt)?;

    super::trusted_launch::validate(req.secure_boot, req.vtpm, firmware)?;
    super::trusted_launch::preflight(req.secure_boot, req.vtpm)?;

    if firmware == "uefi" && find_ovmf_code().is_none() {
        return Err(LibvirtError::Operation(
                "UEFI firmware (OVMF) not found. Install edk2-ovmf (Fedora/RHEL) or ovmf (Debian/Ubuntu).".to_string()
            ));
    }

    let resolved_iso: Option<std::path::PathBuf> = if !req.iso.is_empty() {
        let iso_path = std::path::Path::new(&req.iso);
        if !iso_path.is_absolute() {
            return Err(LibvirtError::Invalid(
                "ISO path must be absolute".to_string(),
            ));
        }
        let iso_path = iso_path
            .canonicalize()
            .map_err(|e| LibvirtError::Invalid(format!("Cannot resolve ISO path: {e}")))?;
        if !iso_path.is_file() {
            return Err(LibvirtError::Operation(format!(
                "ISO file not found or is not a file: {}",
                iso_path.display()
            )));
        }
        Some(iso_path)
    } else {
        None
    };

    let resolved_cloud_init: Option<std::path::PathBuf> = if !req.cloud_init_iso.is_empty() {
        let p = std::path::Path::new(&req.cloud_init_iso);
        if !p.is_absolute() {
            return Err(LibvirtError::Invalid(
                "cloud_init_iso path must be absolute".to_string(),
            ));
        }
        let p = p.canonicalize().map_err(|e| {
            LibvirtError::Invalid(format!("Cannot resolve cloud_init_iso path: {e}"))
        })?;
        if !p.is_file() {
            return Err(LibvirtError::Operation(format!(
                "cloud_init_iso not found or not a file: {}",
                p.display()
            )));
        }
        Some(p)
    } else {
        None
    };

    let resolved_virtio_win: Option<std::path::PathBuf> = if !req.virtio_win_iso.is_empty() {
        let p = std::path::Path::new(&req.virtio_win_iso);
        if !p.is_absolute() {
            return Err(LibvirtError::Invalid(
                "virtio_win_iso path must be absolute".to_string(),
            ));
        }
        let p = p.canonicalize().map_err(|e| {
            LibvirtError::Invalid(format!("Cannot resolve virtio_win_iso path: {e}"))
        })?;
        if !p.is_file() {
            return Err(LibvirtError::Operation(format!(
                "virtio_win_iso not found or not a file: {}",
                p.display()
            )));
        }
        Some(p)
    } else {
        None
    };

    let disk_path = if !req.existing_disk.is_empty() {
        let disk = std::path::Path::new(&req.existing_disk);
        if !disk.is_absolute() {
            return Err(LibvirtError::Invalid(
                "Existing disk path must be absolute".to_string(),
            ));
        }
        if !disk.is_file() {
            return Err(LibvirtError::Operation(format!(
                "Disk image not found: {}",
                req.existing_disk
            )));
        }
        req.existing_disk.clone()
    } else {
        crate::validate::validate_disk_gb(req.disk_gb)?;
        let path = find_disk_path(conn, &req.name)?;
        create_qcow2_disk(&path, req.disk_gb, log)?;
        super::guest_agent_provision::inject_guestkit_into_disk(&path, Some(libvirt_cfg), log)?;
        path
    };

    // Probe the real on-disk format rather than guessing from the filename. A raw
    // image with no `.raw`/`.img` extension (e.g. `/images/win`) was previously
    // declared `type='qcow2'`, so qemu rejected it as a corrupt qcow2 header and the
    // VM failed to boot. Fall back to the extension heuristic only if the probe fails.
    let disk_driver = probe_disk_format(&disk_path).unwrap_or_else(|| {
        if disk_path.ends_with(".raw") || disk_path.ends_with(".img") {
            "raw".to_string()
        } else {
            "qcow2".to_string()
        }
    });
    let disk_driver = disk_driver.as_str();

    let iso_str = resolved_iso
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let cloud_str = resolved_cloud_init
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let virtio_win_str = resolved_virtio_win
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let xml = generate_domain_xml(
        req,
        &disk_path,
        disk_driver,
        firmware,
        &iso_str,
        &cloud_str,
        &virtio_win_str,
        gl,
        gt,
    );

    Domain::define_xml(conn, &xml)
        .map_err(|e| LibvirtError::Operation(format!("Failed to define VM '{}': {e}", req.name)))?;

    Ok(())
}

pub fn find_disk_path(conn: &Connect, vm_name: &str) -> Result<String, LibvirtError> {
    if let Some(base) = super::storage::primary_vm_disk_base_dir(conn) {
        return Ok(format!("{}/{}.qcow2", base.trim_end_matches('/'), vm_name));
    }
    Ok(format!("/var/lib/libvirt/images/{}.qcow2", vm_name))
}

fn create_qcow2_disk(
    path: &str,
    size_gb: u64,
    log: Option<&VmCreateLogSink>,
) -> Result<(), LibvirtError> {
    if Path::new(path).exists() {
        return Err(LibvirtError::Operation(format!(
            "Disk image already exists: {path}"
        )));
    }

    let summary = format!("$ qemu-img create -f qcow2 {path} {size_gb}G");
    let mut cmd = Command::new("qemu-img");
    cmd.args(["create", "-f", "qcow2", path, &format!("{size_gb}G")]);
    let output = subprocess::run_command_streaming(cmd, &summary, "qemu-img", log)?;

    if !output.status.success() {
        return Err(LibvirtError::Operation(format!(
            "qemu-img failed (exit {}); see streamed log",
            output.status
        )));
    }

    Ok(())
}

/// Probe a disk image's real format via `qemu-img info`. Returns the libvirt driver
/// type string (e.g. "qcow2", "raw") or None if the tool is unavailable / errors, so
/// the caller can fall back to an extension-based guess.
fn probe_disk_format(path: &str) -> Option<String> {
    let out = Command::new("qemu-img")
        .args(["info", "--output=json", path])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    json.get("format")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn find_qemu_binary() -> String {
    let candidates = [
        "/usr/bin/qemu-system-x86_64",
        "/usr/libexec/qemu-kvm",
        "/usr/bin/qemu-kvm",
    ];
    for path in &candidates {
        if Path::new(path).is_file() {
            return path.to_string();
        }
    }
    // PATH fallback — mirrors hyper2kvm's shutil.which behaviour
    for name in &["qemu-system-x86_64", "qemu-kvm"] {
        if let Ok(out) = Command::new("which").arg(name).output() {
            if out.status.success() {
                let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !p.is_empty() {
                    return p;
                }
            }
        }
    }
    "/usr/bin/qemu-system-x86_64".to_string()
}

pub(crate) fn find_ovmf_code() -> Option<String> {
    let candidates = [
        "/usr/share/edk2/ovmf/OVMF_CODE.fd",
        "/usr/share/edk2/ovmf/x64/OVMF_CODE.fd",
        "/usr/share/OVMF/OVMF_CODE.fd",
        "/usr/share/edk2/x64/OVMF_CODE.fd",
        "/usr/share/qemu/OVMF_CODE.fd",
        "/usr/share/edk2/ovmf/OVMF_CODE.secboot.fd",
        "/usr/share/edk2/ovmf/x64/OVMF_CODE.secboot.fd",
        "/usr/share/OVMF/OVMF_CODE_4M.fd",
    ];
    for path in &candidates {
        if Path::new(path).is_file() {
            return Some(path.to_string());
        }
    }
    None
}

pub(crate) fn find_ovmf_vars_template() -> Option<String> {
    let candidates = [
        "/usr/share/edk2/ovmf/OVMF_VARS.fd",
        "/usr/share/OVMF/OVMF_VARS.fd",
        "/usr/share/edk2/ovmf/x64/OVMF_VARS.fd",
        "/usr/share/qemu/OVMF_VARS.fd",
        "/usr/share/edk2/ovmf/OVMF_VARS.secboot.fd",
        "/usr/share/edk2/ovmf/x64/OVMF_VARS.secboot.fd",
        "/usr/share/OVMF/OVMF_VARS_4M.fd",
    ];
    for path in &candidates {
        if Path::new(path).is_file() {
            return Some(path.to_string());
        }
    }
    None
}

/// Detect SPICE availability by probing for libspice-server.so in common lib dirs.
pub(crate) fn has_spice() -> bool {
    let lib_dirs = ["/usr/lib64", "/usr/lib/x86_64-linux-gnu", "/usr/lib"];
    for dir in &lib_dirs {
        let dir_path = Path::new(dir);
        if dir_path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(dir_path) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    if name.to_string_lossy().starts_with("libspice-server.so") {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Render a libvirt `<disk type='network'>` block for an Atlas RBD (Ceph) volume
/// when `disk_path` is an rbd reference (`rbd:<pool>/<image>[?mon=..&auth=..&secret=..]`).
/// Mirrors `machina_translate::domain_xml`. Returns `None` for a normal file path.
fn rbd_disk_xml_from_path(disk_path: &str, disk_boot_order: &str) -> Option<String> {
    let rest = disk_path
        .strip_prefix("rbd://")
        .or_else(|| disk_path.strip_prefix("rbd:"))?;
    let (name, query) = match rest.split_once('?') {
        Some((n, q)) => (n, Some(q)),
        None => (rest, None),
    };
    if name.is_empty() {
        return None;
    }
    let name_esc = crate::xml::escape(name);
    let mut mons: Vec<(String, String)> = Vec::new();
    let mut auth_user: Option<String> = None;
    let mut secret_uuid: Option<String> = None;
    if let Some(q) = query {
        for pair in q.split('&') {
            let Some((k, v)) = pair.split_once('=') else {
                continue;
            };
            match k {
                "mon" | "mons" | "hosts" => {
                    for h in v.split(',').map(str::trim).filter(|h| !h.is_empty()) {
                        let (host, port) = match h.rsplit_once(':') {
                            Some((hh, pp))
                                if !pp.is_empty() && pp.bytes().all(|b| b.is_ascii_digit()) =>
                            {
                                (hh, pp)
                            }
                            _ => (h, "6789"),
                        };
                        mons.push((host.to_string(), port.to_string()));
                    }
                }
                "auth" | "user" | "username" => auth_user = Some(v.to_string()),
                "secret" | "secret_uuid" => secret_uuid = Some(v.to_string()),
                _ => {}
            }
        }
    }
    let auth_xml = match (auth_user.as_deref(), secret_uuid.as_deref()) {
        (Some(u), Some(s)) => format!(
            "\n      <auth username='{}'>\n        <secret type='ceph' uuid='{}'/>\n      </auth>",
            crate::xml::escape(u),
            crate::xml::escape(s)
        ),
        _ => String::new(),
    };
    let hosts_xml = if mons.is_empty() {
        String::new()
    } else {
        let mut s = String::from("\n");
        for (h, p) in &mons {
            s.push_str(&format!(
                "        <host name='{}' port='{}'/>\n",
                crate::xml::escape(h),
                crate::xml::escape(p)
            ));
        }
        s.push_str("      ");
        s
    };
    Some(format!(
        r#"<disk type='network' device='disk'>
      <driver name='qemu' type='raw'/>{auth_xml}
      <source protocol='rbd' name='{name_esc}'>{hosts_xml}</source>
      <target dev='vda' bus='virtio'/>{disk_boot_order}
    </disk>"#
    ))
}

#[allow(clippy::too_many_arguments)]
fn generate_domain_xml(
    req: &CreateVmRequest,
    disk_path: &str,
    disk_driver: &str,
    firmware: &str,
    iso_path: &str,
    cloud_init_iso_path: &str,
    virtio_win_iso_path: &str,
    graphics_listen: &str,
    graphics_type: &str,
) -> String {
    let memory_kib = req.memory_mb * 1024;
    let name = crate::xml::escape(&req.name);
    let network = crate::xml::escape(&req.network);
    let disk_path_esc = crate::xml::escape(disk_path);
    let disk_driver_esc = crate::xml::escape(disk_driver);

    // Resolve graphics block (vnc, spice, or both).
    let graphics_xml =
        super::graphics_convert::graphics_elements_xml(graphics_listen, graphics_type);
    // qxl is not always available (minimal qemu builds); vga is widely supported.
    let video_model = "vga";

    // BIOS: boot device in OS block.  UEFI: boot order on devices instead.
    let is_uefi = firmware == "uefi";
    let bios_boot_dev = if !iso_path.is_empty() { "cdrom" } else { "hd" };

    let os_xml = if req.secure_boot {
        super::trusted_launch::SECURE_BOOT_OS_XML.to_string()
    } else if is_uefi {
        let ovmf_code =
            find_ovmf_code().unwrap_or_else(|| "/usr/share/edk2/ovmf/OVMF_CODE.fd".to_string());
        let vars_template = find_ovmf_vars_template();
        let nvram_template_attr = match &vars_template {
            Some(t) => format!(" template='{}'", crate::xml::escape(t)),
            None => String::new(),
        };
        format!(
            r#"<os>
    <type arch='x86_64' machine='q35'>hvm</type>
    <loader readonly='yes' type='pflash'>{ovmf_code}</loader>
    <nvram{nvram_template_attr}>/var/lib/libvirt/qemu/nvram/{name}_VARS.fd</nvram>
  </os>"#,
            ovmf_code = crate::xml::escape(&ovmf_code),
            nvram_template_attr = nvram_template_attr,
            name = name,
        )
    } else {
        format!(
            r#"<os>
    <type arch='x86_64' machine='q35'>hvm</type>
    <boot dev='{bios_boot_dev}'/>
  </os>"#,
            bios_boot_dev = bios_boot_dev,
        )
    };

    // For UEFI: use per-device boot order so firmware can find the disk/cdrom.
    let disk_boot_order = if is_uefi {
        "\n      <boot order='1'/>"
    } else {
        ""
    };

    // Root disk: an Atlas RBD volume (rbd: disk_path) attaches as a libvirt
    // network disk; otherwise the local qcow2/raw file is used.
    let root_disk_xml = rbd_disk_xml_from_path(disk_path, disk_boot_order).unwrap_or_else(|| {
        format!(
            r#"<disk type='file' device='disk'>
      <driver name='qemu' type='{disk_driver_esc}'/>
      <source file='{disk_path_esc}'/>
      <target dev='vda' bus='virtio'/>{disk_boot_order}
    </disk>"#
        )
    });

    let cdrom_xml = if !iso_path.is_empty() {
        let cdrom_boot = if is_uefi {
            "\n      <boot order='2'/>"
        } else {
            ""
        };
        format!(
            r#"
    <disk type='file' device='cdrom'>
      <driver name='qemu' type='raw'/>
      <source file='{}'/>
      <target dev='sda' bus='sata'/>{cdrom_boot}
      <readonly/>
    </disk>"#,
            crate::xml::escape(iso_path),
            cdrom_boot = cdrom_boot,
        )
    } else {
        String::new()
    };

    let cloud_init_cdrom_xml = if !cloud_init_iso_path.is_empty() {
        format!(
            r#"
    <disk type='file' device='cdrom'>
      <driver name='qemu' type='raw' cache='none'/>
      <source file='{}'/>
      <target dev='sdc' bus='sata'/>
      <readonly/>
    </disk>"#,
            crate::xml::escape(cloud_init_iso_path)
        )
    } else {
        String::new()
    };

    let virtio_win_cdrom_xml = if !virtio_win_iso_path.is_empty() {
        format!(
            r#"
    <disk type='file' device='cdrom'>
      <driver name='qemu' type='raw' cache='none'/>
      <source file='{}'/>
      <target dev='sdb' bus='sata'/>
      <readonly/>
    </disk>"#,
            crate::xml::escape(virtio_win_iso_path)
        )
    } else {
        String::new()
    };

    let emulator = find_qemu_binary();
    // Boot with `vcpus` but declare a higher maximum so online CPU hotplug (`set_vcpus`
    // with AFFECT_LIVE) can add vCPUs without a reboot. Without max > current, libvirt
    // rejects any live increase. Headroom is 4x capped at 16 (QEMU reserves only light
    // per-vCPU state for the ceiling), never below the requested count.
    let vcpu_max = vcpu_max_for(req.vcpus);
    let smm_xml = if req.secure_boot {
        format!("\n    {}", super::trusted_launch::SMM_FEATURE_XML)
    } else {
        String::new()
    };
    let tpm_xml = if req.vtpm {
        super::trusted_launch::TPM_DEVICE_XML
    } else {
        ""
    };
    format!(
        r#"<domain type='kvm'>
  <name>{name}</name>
  <memory unit='KiB'>{memory_kib}</memory>
  <currentMemory unit='KiB'>{memory_kib}</currentMemory>
  <vcpu placement='static' current='{vcpus}'>{vcpu_max}</vcpu>
  {os_xml}
  <features>
    <acpi/>
    <apic/>
    <vmport state='off'/>{smm_xml}
  </features>
  <cpu mode='host-passthrough' check='none'/>
  <clock offset='utc'>
    <timer name='rtc' tickpolicy='catchup'/>
    <timer name='pit' tickpolicy='delay'/>
    <timer name='hpet' present='no'/>
  </clock>
  <on_poweroff>destroy</on_poweroff>
  <on_reboot>restart</on_reboot>
  <on_crash>restart</on_crash>
  <devices>
    <emulator>{emulator}</emulator>
    {root_disk_xml}{cdrom_xml}{virtio_win_cdrom_xml}{cloud_init_cdrom_xml}
    <interface type='network'>
      <source network='{network}'/>
      <model type='virtio'/>
    </interface>
    <serial type='pty'>
      <target port='0'/>
    </serial>
    <console type='pty'>
      <target type='serial' port='0'/>
    </console>
    <channel type='unix'>
      <target type='virtio' name='org.qemu.guest_agent.0'/>
    </channel>
    <!-- VNC: noVNC + /ws/v1/vnc/{{name}}. SPICE: spice-html5 + /ws/v1/spice/{{name}}. -->
    {graphics_xml}
    <video>
      <model type='{video_model}' heads='1'/>
    </video>
    {tpm_xml}
    <controller type='usb' index='0' model='qemu-xhci'/>
    <!-- Spare PCIe root ports so NIC/disk hotplug has free slots on q35.
         Without these, libvirt only creates ports for devices present at define
         time and later attach fails with "No more available PCI slots". -->
    <controller type='pci' model='pcie-root-port'/>
    <controller type='pci' model='pcie-root-port'/>
    <controller type='pci' model='pcie-root-port'/>
    <controller type='pci' model='pcie-root-port'/>
    <controller type='pci' model='pcie-root-port'/>
    <controller type='pci' model='pcie-root-port'/>
    <controller type='pci' model='pcie-root-port'/>
    <controller type='pci' model='pcie-root-port'/>
    <input type='tablet' bus='usb'/>
    <memballoon model='virtio'/>
    <rng model='virtio'>
      <backend model='random'>/dev/urandom</backend>
    </rng>
  </devices>
</domain>"#,
        name = name,
        memory_kib = memory_kib,
        vcpus = req.vcpus,
        os_xml = os_xml,
        cdrom_xml = cdrom_xml,
        virtio_win_cdrom_xml = virtio_win_cdrom_xml,
        cloud_init_cdrom_xml = cloud_init_cdrom_xml,
        network = network,
        graphics_xml = graphics_xml,
        video_model = video_model,
    )
}

#[cfg(test)]
mod tests {
    use super::{generate_domain_xml, vcpu_max_for};
    use crate::state::CreateVmRequest;

    fn domain_xml(secure_boot: bool, vtpm: bool, firmware: &str) -> String {
        let req = CreateVmRequest {
            name: "w11".into(),
            secure_boot,
            vtpm,
            firmware: firmware.into(),
            ..Default::default()
        };
        generate_domain_xml(
            &req,
            "/var/lib/libvirt/images/w11.qcow2",
            "qcow2",
            firmware,
            "",
            "",
            "",
            "127.0.0.1",
            "vnc",
        )
    }

    #[test]
    fn trusted_launch_adds_secure_boot_smm_and_tpm() {
        let x = domain_xml(true, true, "uefi");
        assert!(x.contains("<os firmware='efi'>"));
        assert!(x.contains("name='secure-boot'"));
        assert!(x.contains("<smm state='on'/>"));
        assert!(x.contains("<tpm model='tpm-crb'>"));
        // libvirt selects the firmware, so no hard-coded loader/nvram.
        assert!(!x.contains("<loader"));
        assert!(!x.contains("<nvram"));
    }

    #[test]
    fn default_create_has_no_tpm_or_smm_and_keeps_the_old_uefi_block() {
        let plain = domain_xml(false, false, "bios");
        assert!(!plain.contains("<tpm"));
        assert!(!plain.contains("<smm"));
        let uefi = domain_xml(false, false, "uefi");
        assert!(uefi.contains("<loader readonly='yes' type='pflash'>"));
        assert!(!uefi.contains("secure-boot"));
    }

    #[test]
    fn tpm_alone_works_with_bios() {
        let x = domain_xml(false, true, "bios");
        assert!(x.contains("<tpm model='tpm-crb'>"));
        assert!(!x.contains("<smm"));
    }

    #[test]
    fn vcpu_headroom_gives_room_to_hotplug_but_stays_capped() {
        // 4x headroom for small VMs where hotplug matters most...
        assert_eq!(vcpu_max_for(1), 4);
        assert_eq!(vcpu_max_for(2), 8);
        assert_eq!(vcpu_max_for(4), 16);
        // ...capped at 16 so we don't reserve absurd per-vCPU state...
        assert_eq!(vcpu_max_for(8), 16);
        // ...but never below the requested boot count (large VMs).
        assert_eq!(vcpu_max_for(24), 24);
        assert_eq!(vcpu_max_for(32), 32);
    }
}
