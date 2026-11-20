// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Optional `virt-install` backend for VM create (hyper2kvm-style), plus Cockpit-machines-style flags.

use std::path::Path;
use std::process::Command;

use virt::connect::Connect;
use virt::domain::Domain;

use crate::state::CreateVmRequest;
use crate::LibvirtError;

use super::subprocess::{self, VmCreateLogSink};

fn append_virt_install_unattended(args: &mut Vec<String>, req: &CreateVmRequest) {
    let wants = req.virt_install_unattended
        || !req.virt_install_admin_password.is_empty()
        || !req.virt_install_user_login.is_empty()
        || !req.virt_install_user_password.is_empty();
    if !wants {
        return;
    }
    if !req.virt_install_admin_password.is_empty() {
        args.push("--unattended".into());
        args.push(format!(
            "admin-password={}",
            req.virt_install_admin_password
        ));
    }
    if !req.virt_install_user_login.is_empty() {
        args.push("--unattended".into());
        args.push(format!("user-login={}", req.virt_install_user_login));
    }
    if !req.virt_install_user_password.is_empty() {
        args.push("--unattended".into());
        args.push(format!("user-password={}", req.virt_install_user_password));
    }
    if req.virt_install_unattended
        && req.virt_install_admin_password.is_empty()
        && req.virt_install_user_login.is_empty()
        && req.virt_install_user_password.is_empty()
    {
        args.push("--unattended".into());
        args.push("profile=desktop".into());
    }
}

fn is_windows_profile(req: &CreateVmRequest) -> bool {
    let gp = req.guest_profile.trim().to_ascii_lowercase();
    if gp == "windows" {
        return true;
    }
    if gp == "linux" {
        return false;
    }
    // auto / empty: infer from os_variant when provided
    let osv = req.os_variant.trim().to_ascii_lowercase();
    osv.starts_with("win") || osv.starts_with("windows")
}

fn has_spice() -> bool {
    // Best-effort: if the SPICE server library isn't present, `virt-install --graphics spice`
    // will fail on many distros/builds.
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

/// True when the JSON body uses fields only implemented by the `virt-install` create path.
pub fn create_request_uses_virt_install_extensions(req: &CreateVmRequest) -> bool {
    req.virt_install_define_only
        || !req.virt_install_location.trim().is_empty()
        || req.virt_install_pxe
        || !req.virt_install_install_os.trim().is_empty()
        || (!req.root_disk_storage_pool.trim().is_empty()
            && !req.root_disk_storage_volume.trim().is_empty())
        || !req.virt_install_disk_backing_store.trim().is_empty()
}

fn extract_domain_xml_from_print_xml(stdout: &[u8]) -> Result<String, LibvirtError> {
    let s = String::from_utf8_lossy(stdout);
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(LibvirtError::Operation(
            "virt-install --print-xml produced no output".into(),
        ));
    }
    let last = trimmed
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .last()
        .unwrap_or(trimmed)
        .trim();
    if !last.contains("<domain") {
        return Err(LibvirtError::Operation(
            "virt-install --print-xml output did not contain a <domain> document".into(),
        ));
    }
    Ok(last.to_string())
}

/// Run `virt-install` to define a new VM (no libvirt XML emit in machina).
pub fn create_vm_virt_install(
    conn: &Connect,
    req: &CreateVmRequest,
    libvirt_uri: &str,
    log: Option<&VmCreateLogSink>,
) -> Result<(), LibvirtError> {
    let win = is_windows_profile(req);
    crate::validate::validate_vcpus(req.vcpus)?;
    crate::validate::validate_memory_mb(req.memory_mb)?;
    let net = if req.network.trim().is_empty() {
        "default"
    } else {
        req.network.trim()
    };
    validate_virt_install_field(net, "network")?;
    let osv = if req.os_variant.trim().is_empty() {
        "generic"
    } else {
        req.os_variant.trim()
    };
    validate_virt_install_field(osv, "os_variant")?;

    let firmware = if req.firmware.is_empty() {
        "bios"
    } else {
        req.firmware.trim()
    };
    if firmware != "bios" && firmware != "uefi" {
        return Err(LibvirtError::Invalid(
            "Firmware must be 'bios' or 'uefi'".into(),
        ));
    }

    let gl = req.graphics_listen.trim();
    let gl = if gl.is_empty() { "127.0.0.1" } else { gl };
    crate::validate::validate_graphics_listen(gl)?;

    let gt = req.graphics_type.trim();
    let gt = if gt.is_empty() {
        if win {
            "spice"
        } else {
            "vnc"
        }
    } else {
        gt
    };
    // If SPICE isn't available on this host/QEMU build, fall back to VNC (Cockpit-compatible).
    let gt = if gt.eq_ignore_ascii_case("spice") && !has_spice() {
        "vnc"
    } else {
        gt
    };
    crate::validate::validate_graphics_type(gt)?;

    let want_both = gt.eq_ignore_ascii_case("both");
    let gt_install = if want_both {
        if win {
            "spice"
        } else {
            "vnc"
        }
    } else {
        gt
    };

    let define_only = req.virt_install_define_only;
    let pxe = req.virt_install_pxe;
    let location = req.virt_install_location.trim();
    let install_os = req.virt_install_install_os.trim();
    let extra = req.virt_install_extra_args.trim();
    let pool = req.root_disk_storage_pool.trim();
    let vol = req.root_disk_storage_volume.trim();
    let backing = req.virt_install_disk_backing_store.trim();
    let pxe_net = if req.virt_install_pxe_network.trim().is_empty() {
        net
    } else {
        req.virt_install_pxe_network.trim()
    };
    if !req.virt_install_pxe_network.trim().is_empty() {
        validate_virt_install_field(pxe_net, "virt_install_pxe_network")?;
    }

    if !req.iso.is_empty() {
        let p = Path::new(&req.iso);
        if !p.is_absolute() {
            return Err(LibvirtError::Invalid("ISO path must be absolute".into()));
        }
        if !p.is_file() {
            return Err(LibvirtError::Operation(format!(
                "ISO not found: {}",
                req.iso
            )));
        }
    }
    if !req.cloud_init_iso.is_empty() {
        let p = Path::new(&req.cloud_init_iso);
        if !p.is_absolute() || !p.is_file() {
            return Err(LibvirtError::Invalid(
                "cloud_init_iso must be an absolute path to an existing file".into(),
            ));
        }
    }
    if !req.virtio_win_iso.is_empty() {
        let p = Path::new(&req.virtio_win_iso);
        if !p.is_absolute() || !p.is_file() {
            return Err(LibvirtError::Invalid(
                "virtio_win_iso must be an absolute path to an existing file".into(),
            ));
        }
    }
    if !req.existing_disk.is_empty() {
        let disk = Path::new(&req.existing_disk);
        if !disk.is_absolute() || !disk.is_file() {
            return Err(LibvirtError::Invalid(
                "existing_disk must be an absolute path to an existing file".into(),
            ));
        }
    }

    let mut args: Vec<String> = vec![
        "--connect".into(),
        libvirt_uri.to_string(),
        "--quiet".into(),
        "--os-variant".into(),
        osv.to_string(),
        "--memory".into(),
        req.memory_mb.to_string(),
        "--name".into(),
        req.name.clone(),
        "--vcpus".into(),
        // Boot `vcpus` but allow hot-adding up to a higher maximum (online CPU hotplug).
        // Without maxvcpus > vcpus, libvirt rejects any live vCPU increase. Matches the
        // headroom applied in the native libvirt-XML backend (see create.rs::vcpu_max_for).
        format!(
            "{},maxvcpus={}",
            req.vcpus,
            super::create::vcpu_max_for(req.vcpus)
        ),
    ];

    if req.virt_install_path_in_use_check_off
        || !pool.is_empty()
        || !req.existing_disk.is_empty()
        || !backing.is_empty()
    {
        args.push("--check".into());
        args.push("path_in_use=off".into());
    }

    if !define_only {
        // Cockpit-style behavior: define/start the VM and return immediately.
        // Waiting for "install completion" is not reliable for interactive ISOs and makes create feel slow.
        args.push("--noautoconsole".into());
    }

    super::trusted_launch::validate(req.secure_boot, req.vtpm, firmware)?;
    super::trusted_launch::preflight(req.secure_boot, req.vtpm)?;
    if req.secure_boot {
        args.extend(super::trusted_launch::virt_install_secure_boot_args());
    } else if firmware == "uefi" {
        args.push("--boot".into());
        args.push("uefi".into());
    }
    if req.vtpm {
        args.push("--tpm".into());
        args.push(super::trusted_launch::VIRT_INSTALL_TPM.into());
    }

    // NIC (always one libvirt network; PXE adds a second `--network` below.)
    args.push("--network".into());
    if win {
        args.push(format!("network={net},model=virtio"));
    } else {
        args.push(format!("network={net}"));
    }

    // Graphics (virt-install supports one device; `both` adds the second via virt-xml afterward)
    if gt_install.eq_ignore_ascii_case("spice") {
        args.push("--graphics".into());
        args.push(format!("spice,listen={gl}"));
        // qxl is not always available (minimal qemu builds); vga is widely supported.
        args.push("--video".into());
        args.push("vga".into());
    } else {
        args.push("--graphics".into());
        args.push(format!("vnc,listen={gl}"));
    }

    // Root disk
    if define_only {
        if !pool.is_empty() {
            args.push("--disk".into());
            args.push(format!("vol={pool}/{vol},bus=virtio"));
        } else if !req.existing_disk.is_empty() {
            args.push("--disk".into());
            args.push(format!("path={},bus=virtio", req.existing_disk));
        } else {
            crate::validate::validate_disk_gb(req.disk_gb)?;
            args.push("--disk".into());
            let bus = if win { ",bus=virtio" } else { "" };
            args.push(format!("size={},format=qcow2{bus}", req.disk_gb));
        }
    } else if !pool.is_empty() {
        args.push("--disk".into());
        args.push(format!("vol={pool}/{vol},bus=virtio"));
        if !req.iso.is_empty() {
            args.push("--cdrom".into());
            args.push(req.iso.clone());
        }
        if !define_only
            && req.iso.is_empty()
            && !pxe
            && location.is_empty()
            && install_os.is_empty()
            && backing.is_empty()
        {
            // Preallocated volume that already contains a guest disk image.
            args.push("--import".into());
        }
    } else if !req.existing_disk.is_empty() {
        args.push("--disk".into());
        args.push(format!("path={},bus=virtio", req.existing_disk));
        if !req.iso.is_empty() {
            args.push("--cdrom".into());
            args.push(req.iso.clone());
        }
        args.push("--import".into());
    } else if !backing.is_empty() {
        crate::validate::validate_disk_gb(req.disk_gb)?;
        args.push("--disk".into());
        let bus = if win { ",bus=virtio" } else { "" };
        args.push(format!(
            "size={},format=qcow2,backing_store={}{}",
            req.disk_gb, backing, bus
        ));
        if !req.iso.is_empty() {
            args.push("--cdrom".into());
            args.push(req.iso.clone());
        }
        args.push("--import".into());
    } else {
        crate::validate::validate_disk_gb(req.disk_gb)?;
        args.push("--disk".into());
        let bus = if win { ",bus=virtio" } else { "" };
        args.push(format!("size={},format=qcow2{bus}", req.disk_gb));
        if !req.iso.is_empty() {
            args.push("--cdrom".into());
            args.push(req.iso.clone());
        } else if !pxe && location.is_empty() && install_os.is_empty() {
            // Blank qcow2: only use --import when no network/PXE/OS media will drive the installer.
            if firmware == "bios" {
                args.push("--boot".into());
                args.push("hd".into());
                args.push("--import".into());
            } else {
                args.push("--import".into());
            }
        }
    }

    if !req.cloud_init_iso.is_empty() {
        args.push("--disk".into());
        args.push(format!(
            "path={},device=cdrom,bus=sata,readonly=on",
            req.cloud_init_iso
        ));
    }

    if !req.virtio_win_iso.is_empty() {
        args.push("--disk".into());
        args.push(format!(
            "path={},device=cdrom,bus=sata,readonly=on",
            req.virtio_win_iso
        ));
    }

    // Install / boot source (define_only skips — matches cockpit-machines install_machine.py)
    if define_only {
        args.push("--print-xml".into());
        args.push("1".into());
    } else if pxe {
        args.push("--pxe".into());
        args.push("--network".into());
        args.push(format!("network={pxe_net}"));
    } else if !install_os.is_empty() {
        args.push("--install".into());
        args.push(format!("os={install_os}"));
        if !extra.is_empty() {
            args.push("--extra-args".into());
            args.push(extra.to_string());
        }
    } else if !location.is_empty() {
        args.push("--location".into());
        args.push(location.to_string());
        if !extra.is_empty() {
            args.push("--extra-args".into());
            args.push(extra.to_string());
        }
    } else if !req.existing_disk.is_empty() || !backing.is_empty() {
        // import + optional cdrom already applied
    } else if !req.iso.is_empty() && !extra.is_empty() {
        args.push("--extra-args".into());
        args.push(extra.to_string());
    }

    append_virt_install_unattended(&mut args, req);

    tracing::info!("virt-install {}", args.join(" "));

    if define_only {
        subprocess::log_line(
            log,
            "machina",
            "virt-install --print-xml=1 (define only, no installer)…",
        );
        let summary = format!("$ virt-install {}", args.join(" "));
        let mut cmd = Command::new("virt-install");
        cmd.args(&args);
        let out = subprocess::run_command_streaming(cmd, &summary, "virt-install", None)?;
        if !out.status.success() {
            return Err(LibvirtError::Operation(format!(
                "virt-install --print-xml failed (exit {})",
                out.status
            )));
        }
        let xml = extract_domain_xml_from_print_xml(&out.stdout)?;
        Domain::define_xml(conn, &xml).map_err(|e| {
            LibvirtError::Operation(format!(
                "Failed to define VM '{}' from virt-install XML: {e}",
                req.name
            ))
        })?;
        subprocess::log_line(log, "machina", "Domain defined from virt-install XML.");
        if want_both {
            add_secondary_graphics_after_virt_install(libvirt_uri, &req.name, gt_install, gl)?;
        }
        return Ok(());
    }

    let summary = format!("$ virt-install {}", args.join(" "));
    let mut cmd = Command::new("virt-install");
    cmd.args(&args);
    let out = subprocess::run_command_streaming(cmd, &summary, "virt-install", log)?;

    if !out.status.success() {
        return Err(LibvirtError::Operation(format!(
            "virt-install failed (exit {}); see streamed log",
            out.status
        )));
    }

    if want_both {
        add_secondary_graphics_after_virt_install(libvirt_uri, &req.name, gt_install, gl)?;
    }

    Ok(())
}

fn add_secondary_graphics_after_virt_install(
    libvirt_uri: &str,
    vm_name: &str,
    primary: &str,
    listen: &str,
) -> Result<(), LibvirtError> {
    let secondary = if primary.eq_ignore_ascii_case("vnc") {
        "spice"
    } else {
        "vnc"
    };
    if secondary == "spice" && !has_spice() {
        return Ok(());
    }
    super::graphics_convert::virt_xml_add_graphics(libvirt_uri, vm_name, secondary, listen)?;
    Ok(())
}

/// Start installation on a define-only VM: undefine domain XML (keep disks) and run `virt-install`.
pub fn install_defined_vm(
    conn: &Connect,
    libvirt_uri: &str,
    req: &CreateVmRequest,
    log: Option<&VmCreateLogSink>,
) -> Result<(), LibvirtError> {
    use super::domain::{delete_vm_with_options, lookup_domain, UndefineOptions};

    let name = &req.name;
    let domain = lookup_domain(conn, name)?;
    let info = domain
        .get_info()
        .map_err(LibvirtError::map_op("get domain info"))?;
    // VIR_DOMAIN_SHUTOFF
    if info.state != 5 {
        return Err(LibvirtError::Invalid(
            "VM must be shut off to start installation".into(),
        ));
    }

    let has_install = !req.virt_install_location.trim().is_empty()
        || req.virt_install_pxe
        || !req.virt_install_install_os.trim().is_empty()
        || !req.iso.trim().is_empty();
    if !has_install {
        return Err(LibvirtError::Invalid(
            "Configure an install source (location, PXE, install_os, or ISO) before installing"
                .into(),
        ));
    }

    let xml = domain
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("get_xml"))?;
    let mut install_req = req.clone();
    install_req.virt_install_define_only = false;
    install_req.virt_install_path_in_use_check_off = true;

    if install_req.existing_disk.is_empty() {
        if let Some(p) = super::template_apply::primary_disk_path_from_xml(&xml) {
            install_req.existing_disk = p.to_string_lossy().into();
        }
    }

    let keep_nvram = install_req.firmware.eq_ignore_ascii_case("uefi");
    delete_vm_with_options(
        conn,
        name,
        &UndefineOptions {
            keep_nvram,
            ..Default::default()
        },
    )?;

    create_vm_virt_install(conn, &install_req, libvirt_uri, log)
}
