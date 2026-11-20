// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use machina_spec::VirtualMachine;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
        .replace('"', "&quot;")
}

/// Generate a random console (VNC/SPICE) password. Uses the same
/// `rand::thread_rng()` + hex-encode pattern already used elsewhere in this
/// workspace for random-secret generation (e.g. `daemon::auth::create_session`,
/// `controller::config::platform_jwt_secret`). 8 random bytes hex-encoded to 16
/// characters: classic VNC "passwd" (DES-based RFB auth) only honours the first
/// 8 bytes of the password, so anything beyond that is harmless but SPICE's
/// ticket auth uses the whole string, giving it the full 8 bytes of entropy.
///
/// Not called from production yet (see `domain_xml_from_spec`'s doc comment
/// on why `None` means no password today) — kept ready for a future caller
/// that also wires this same value to the console proxy/frontend.
#[allow(dead_code)]
fn generate_console_password() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 8] = rng.gen();
    hex::encode(bytes)
}

/// Emit `<graphics>` elements for the requested type(s). When `passwd` is
/// non-empty, adds a `passwd=` attribute (libvirt/QEMU-native VNC/SPICE auth)
/// as defense in depth so the framebuffer isn't wide open to any other local
/// process/user that can reach the listen address directly, bypassing the
/// app's own token-gated console proxy entirely. An empty `passwd` omits the
/// attribute entirely (no libvirt-native auth) — this is the current default
/// (see `domain_xml_from_spec`'s doc comment for why): the agent's own
/// console proxy doesn't yet know this password, and the web VNCViewer
/// unconditionally answers any auth challenge with an empty string, so
/// setting a real `passwd=` here today would break every in-app console.
fn graphics_block(listen: &str, graphics_type: &str, passwd: &str) -> String {
    let gt = graphics_type.to_ascii_lowercase();
    let want_vnc = gt == "vnc" || gt == "both";
    let want_spice = gt == "spice" || gt == "both";
    let passwd_attr = if passwd.is_empty() {
        String::new()
    } else {
        format!(" passwd='{}'", esc(passwd))
    };
    let mut lines = Vec::new();
    if want_vnc {
        lines.push(format!(
            "<graphics type='vnc' port='-1' autoport='yes' listen='{listen}'{passwd_attr}/>"
        ));
    }
    if want_spice {
        lines.push(format!(
            "<graphics type='spice' port='-1' autoport='yes' listen='{listen}'{passwd_attr}/>"
        ));
    }
    if lines.is_empty() {
        lines.push(format!(
            "<graphics type='vnc' port='-1' autoport='yes' listen='{listen}'{passwd_attr}/>"
        ));
    }
    lines.join("\n    ")
}

/// Parse an RBD (Ceph) disk source and render a libvirt `<disk type='network'>`
/// block. The source is set by the Atlas storage integration when a VM's root
/// disk is an Atlas-provisioned RBD volume. Format:
///
/// ```text
/// rbd:<pool>/<image>[?mon=host1:6789,host2:6789&auth=<cephx-user>&secret=<libvirt-secret-uuid>]
/// ```
///
/// `mon` (Ceph monitor hosts) and `auth`/`secret` (cephx user + the UUID of a
/// libvirt `ceph` secret holding the key) are optional — omit them when the
/// hypervisor's `/etc/ceph/ceph.conf` + keyring already supply them. Returns
/// `None` when `source` is not an rbd reference (caller falls back to a file disk).
///
/// `pub`: also used by `agent::libvirt_ops::attach_disk` to hot-attach an
/// already-provisioned Atlas/RBD volume as a non-root data disk (`target_dev`
/// lets that caller name a device other than the root disk's fixed `vda`).
pub fn rbd_disk_xml(source: &str, target_dev: &str, disk_boot: &str) -> Option<String> {
    let rest = source
        .strip_prefix("rbd://")
        .or_else(|| source.strip_prefix("rbd:"))?;
    let (name, query) = match rest.split_once('?') {
        Some((n, q)) => (n, Some(q)),
        None => (rest, None),
    };
    if name.is_empty() {
        return None;
    }
    let name_esc = esc(name);

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
            esc(u),
            esc(s)
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
                esc(h),
                esc(p)
            ));
        }
        s.push_str("      ");
        s
    };

    let target_dev_esc = esc(target_dev);
    Some(format!(
        r#"<disk type='network' device='disk'>
      <driver name='qemu' type='raw'/>{auth_xml}
      <source protocol='rbd' name='{name_esc}'>{hosts_xml}</source>
      <target dev='{target_dev_esc}' bus='virtio'/>{disk_boot}
    </disk>"#
    ))
}

/// Generate libvirt domain XML from a declarative VM spec (operators never see this).
///
/// `graphics_password`: the libvirt-native VNC/SPICE console password to embed
/// in the `<graphics>` element(s) (see `graphics_block`). Pass `Some(pw)` to
/// set a real password (e.g. once a caller can also plumb that same value to
/// whatever connects to the console). Pass `None` for NO password (current
/// default — see caveat below), not "auto-generate": `generate_console_password`
/// is exercised directly by tests and is available for a future caller, but
/// nothing calls it from here anymore.
///
/// Caveat (flagged for follow-up, not yet wired): the agent's own console
/// proxy (`agent::console_ws::handle_vnc`/`handle_spice`) is a raw byte-level
/// TCP<->WebSocket relay — it does not speak the RFB/SPICE protocol itself,
/// so it never sees or supplies this password. The browser-side client
/// (`web/src/components/VNCViewer.tsx`) answers any `credentialsrequired`
/// challenge with an empty password. Setting a real `passwd=` here without
/// also wiring one of: (a) the real password reaching the frontend so it can
/// answer the credentials challenge, or (b) the console proxy performing the
/// VNC/SPICE handshake itself using the password read back off the live
/// domain's XML (`virDomainGetXMLDesc` includes `passwd=` for the caller that
/// defined it, or use `virDomainOpenGraphics`/`connect --with-password`-style
/// APIs) — would break every existing in-app console immediately (previously
/// shipped as the default here and confirmed to do exactly that; reverted).
/// Until one of those lands, every production call site must keep passing
/// `None` — see `agent/src/libvirt_ops.rs`.
pub fn domain_xml_from_spec(
    vm: &VirtualMachine,
    disk_path: &str,
    disk_driver: &str,
    cloud_init_iso: Option<&str>,
    graphics_password: Option<&str>,
) -> Result<String, machina_spec::SpecError> {
    vm.validate()?;
    let memory_kib = vm.memory_mib()? * 1024;
    let vcpus = vm.total_vcpus();
    let name = esc(&vm.metadata.name);
    let network = vm
        .spec
        .network
        .first()
        .map(|n| esc(&n.network))
        .unwrap_or_else(|| "default".into());
    let disk_path_esc = esc(disk_path);
    let disk_driver_esc = esc(disk_driver);
    let gl = esc(&vm.spec.graphics.listen);
    let gt = vm.spec.graphics.r#type.trim();
    let pw = graphics_password.unwrap_or("");
    let graphics_xml = graphics_block(&gl, gt, pw);
    let firmware = vm.spec.firmware.to_ascii_lowercase();
    let is_uefi = firmware == "uefi";
    let emulator = esc(&crate::qemu::find_qemu_binary());
    let tpm_enabled = vm
        .metadata
        .labels
        .as_ref()
        .and_then(|m| m.get("tpm"))
        .is_some_and(|v| v == "true" || v == "1");

    // Secure Boot is a UEFI-only property: it needs the secboot OVMF build, a
    // `secure='yes'` loader, and SMM. libvirt rejects `secure='yes'` without
    // `<smm state='on'/>`, so the two are always emitted together.
    let secure_boot_enabled = is_uefi
        && vm
            .metadata
            .labels
            .as_ref()
            .and_then(|m| m.get("secure_boot"))
            .is_some_and(|v| v == "true" || v == "1");

    let install_iso = vm
        .metadata
        .labels
        .as_ref()
        .and_then(|m| m.get("install_iso"))
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let os_xml = if is_uefi {
        // UEFI expresses boot order with per-device <boot order> (below), never
        // with <os><boot dev>. libvirt rejects a domain that mixes the two, so the
        // UEFI <os> must NOT carry <boot dev> even when an install ISO is present.
        let (loader_secure, code_fd, vars_fd) = if secure_boot_enabled {
            (
                " secure='yes'",
                "/usr/share/edk2/ovmf/OVMF_CODE.secboot.fd",
                "/usr/share/edk2/ovmf/OVMF_VARS.secboot.fd",
            )
        } else {
            (
                "",
                "/usr/share/edk2/ovmf/OVMF_CODE.fd",
                "/usr/share/edk2/ovmf/OVMF_VARS.fd",
            )
        };
        format!(
            r#"<os>
    <type arch='x86_64' machine='q35'>hvm</type>
    <loader readonly='yes'{loader_secure} type='pflash'>{code_fd}</loader>
    <nvram template='{vars_fd}'>/var/lib/libvirt/qemu/nvram/{name}_VARS.fd</nvram>
  </os>"#
        )
    } else if install_iso.is_some() {
        r#"<os>
    <type arch='x86_64' machine='q35'>hvm</type>
    <boot dev='cdrom'/>
    <boot dev='hd'/>
  </os>"#
            .into()
    } else {
        r#"<os>
    <type arch='x86_64' machine='q35'>hvm</type>
    <boot dev='hd'/>
  </os>"#
            .into()
    };

    // UEFI boot order is per-device. When an install ISO is present the cdrom
    // takes order 1 and the root disk order 2 (install-then-boot-installed-OS);
    // otherwise the disk is the sole bootable device at order 1.
    let disk_boot = if is_uefi {
        if install_iso.is_some() {
            "\n      <boot order='2'/>"
        } else {
            "\n      <boot order='1'/>"
        }
    } else {
        ""
    };

    // Root disk: an Atlas-provisioned RBD volume (when the root storage spec
    // carries an `rbd:` source) is attached as a libvirt network disk; otherwise
    // the local qcow2/raw file created by the agent is used.
    let root_disk_xml = vm
        .spec
        .storage
        .first()
        .and_then(|s| s.source.as_deref())
        .filter(|s| !s.is_empty())
        .and_then(|src| rbd_disk_xml(src, "vda", disk_boot))
        .unwrap_or_else(|| {
            format!(
                r#"<disk type='file' device='disk'>
      <driver name='qemu' type='{disk_driver_esc}'/>
      <source file='{disk_path_esc}'/>
      <target dev='vda' bus='virtio'/>{disk_boot}
    </disk>"#
            )
        });

    let install_iso_xml = install_iso
        .map(|iso| {
            let iso_esc = esc(iso);
            let cdrom_boot = if is_uefi {
                "\n      <boot order='1'/>"
            } else {
                ""
            };
            // SATA, not IDE: every domain here is machine='q35', and q35 has no
            // IDE controller — libvirt rejects the whole definition with
            // "IDE controllers are unsupported for this QEMU binary or machine
            // type", so create-from-ISO failed outright.
            format!(
                r#"    <disk type='file' device='cdrom'>
      <driver name='qemu' type='raw'/>
      <source file='{iso_esc}'/>
      <target dev='sda' bus='sata'/>
      <readonly/>{cdrom_boot}
    </disk>
"#
            )
        })
        .unwrap_or_default();

    let cloud_iso_xml = cloud_init_iso
        .filter(|p| !p.is_empty())
        .map(|iso| {
            let iso_esc = esc(iso);
            let target = if install_iso.is_some() { "sdb" } else { "sda" };
            format!(
                r#"    <disk type='file' device='cdrom'>
      <driver name='qemu' type='raw'/>
      <source file='{iso_esc}'/>
      <target dev='{target}' bus='sata'/>
      <readonly/>
    </disk>
"#
            )
        })
        .unwrap_or_default();

    let tpm_xml = if tpm_enabled {
        r#"    <tpm model='tpm-crb'>
      <backend type='emulator' version='2.0'/>
    </tpm>
"#
    } else {
        ""
    };

    // Boot `vcpus` but declare a higher maximum so online CPU hotplug (set_vcpus with
    // AFFECT_LIVE) can hot-add without a reboot — libvirt forbids raising vCPUs above the
    // domain's defined maximum. Headroom is 4x capped at 16, never below the boot count.
    // (Mirrors core::libvirt::create::vcpu_max_for; inlined to keep `translate` core-free.)
    let vcpu_max = vcpus.max(vcpus.saturating_mul(4).min(16));
    let smm_xml = if secure_boot_enabled {
        "\n    <smm state='on'/>"
    } else {
        ""
    };
    Ok(format!(
        r#"<domain type='kvm'>
  <name>{name}</name>
  <memory unit='KiB'>{memory_kib}</memory>
  <vcpu placement='static' current='{vcpus}'>{vcpu_max}</vcpu>
  {os_xml}
  <features>
    <acpi/>
    <apic/>{smm_xml}
  </features>
  <clock offset='utc'/>
  <on_poweroff>destroy</on_poweroff>
  <on_reboot>restart</on_reboot>
  <on_crash>destroy</on_crash>
  <devices>
    <emulator>{emulator}</emulator>
    {root_disk_xml}
{install_iso_xml}{cloud_iso_xml}    <interface type='network'>
      <source network='{network}'/>
      <model type='virtio'/>
    </interface>
    <channel type='unix'>
      <target type='virtio' name='org.qemu.guest_agent.0'/>
    </channel>
{tpm_xml}    {graphics_xml}
    <video>
      <model type='vga'/>
    </video>
    <console type='pty'/>
  </devices>
</domain>"#
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use machina_spec::VirtualMachine;

    #[test]
    fn generates_domain_xml() {
        let vm = VirtualMachine::new("demo", "1Gi");
        let xml = domain_xml_from_spec(
            &vm,
            "/var/lib/libvirt/images/demo.qcow2",
            "qcow2",
            None,
            None,
        )
        .unwrap();
        assert!(xml.contains("<name>demo</name>"));
        assert!(xml.contains("source network='default'"));
        assert!(xml.contains("type='qcow2'"));
        assert!(xml.contains("type='vnc'"));
        assert!(xml.contains("type='spice'"));
    }

    #[test]
    fn graphics_omit_passwd_by_default() {
        // `None` means NO password today (not auto-generate): the agent's own
        // console proxy is a raw byte-level relay that doesn't speak RFB/SPICE
        // and the web VNCViewer answers any auth challenge with an empty
        // string, so a real passwd= here would break every in-app console
        // until one of those is wired to the same value. See
        // `domain_xml_from_spec`'s doc comment.
        let vm = VirtualMachine::new("pwdemo", "1Gi");
        let xml = domain_xml_from_spec(
            &vm,
            "/var/lib/libvirt/images/pwdemo.qcow2",
            "qcow2",
            None,
            None,
        )
        .unwrap();
        assert!(xml.contains("type='vnc'"));
        assert!(xml.contains("type='spice'"));
        assert!(
            !xml.contains("passwd="),
            "default must not set a libvirt-native console password"
        );
    }

    #[test]
    fn graphics_use_caller_supplied_fixed_passwd() {
        let vm = VirtualMachine::new("pwfixed", "1Gi");
        let xml = domain_xml_from_spec(
            &vm,
            "/var/lib/libvirt/images/pwfixed.qcow2",
            "qcow2",
            None,
            Some("t3stPassw0rd"),
        )
        .unwrap();
        assert!(xml.contains("passwd='t3stPassw0rd'"));
    }

    #[test]
    fn generate_console_password_produces_distinct_nonempty_values() {
        // Not yet called from production (see domain_xml_from_spec's doc
        // comment), but kept ready for a future caller that wires a real
        // password through to the console proxy/frontend — exercised
        // directly here so it isn't dead code in the meantime.
        let a = generate_console_password();
        let b = generate_console_password();
        assert!(!a.is_empty());
        assert_ne!(a, b, "must not reuse the same password across calls");
    }

    #[test]
    fn atlas_rbd_source_renders_network_disk() {
        let mut vm = VirtualMachine::new("cephvm", "2Gi");
        vm.spec.storage[0].source = Some(
            "rbd:rbd-nvme-prod/csi-vol-abc?mon=10.0.0.1:6789,10.0.0.2:6789&auth=machina&secret=1a2b3c4d-5e6f-7890-abcd-ef1234567890"
                .into(),
        );
        let xml = domain_xml_from_spec(
            &vm,
            "/var/lib/libvirt/images/cephvm.qcow2",
            "qcow2",
            None,
            None,
        )
        .unwrap();
        assert!(xml.contains("<disk type='network' device='disk'>"));
        assert!(xml.contains("protocol='rbd' name='rbd-nvme-prod/csi-vol-abc'"));
        assert!(xml.contains("<host name='10.0.0.1' port='6789'/>"));
        assert!(xml.contains("<host name='10.0.0.2' port='6789'/>"));
        assert!(xml.contains("<auth username='machina'>"));
        assert!(xml.contains("<secret type='ceph' uuid='1a2b3c4d-5e6f-7890-abcd-ef1234567890'/>"));
        // The local file disk must NOT be emitted for an RBD root.
        assert!(!xml.contains("source file='/var/lib/libvirt/images/cephvm.qcow2'"));
    }

    #[test]
    fn atlas_rbd_source_without_mons_omits_hosts_and_auth() {
        let mut vm = VirtualMachine::new("cephvm2", "2Gi");
        vm.spec.storage[0].source = Some("rbd:pool/img".into());
        let xml = domain_xml_from_spec(&vm, "/tmp/unused.qcow2", "qcow2", None, None).unwrap();
        assert!(xml.contains("protocol='rbd' name='pool/img'"));
        assert!(!xml.contains("<host "));
        assert!(!xml.contains("<auth "));
    }

    #[test]
    fn non_rbd_source_falls_back_to_file_disk() {
        let mut vm = VirtualMachine::new("filevm", "2Gi");
        vm.spec.storage[0].source = Some("/some/other/path.qcow2".into());
        let xml = domain_xml_from_spec(
            &vm,
            "/var/lib/libvirt/images/filevm.qcow2",
            "qcow2",
            None,
            None,
        )
        .unwrap();
        assert!(xml.contains("<disk type='file' device='disk'>"));
        assert!(xml.contains("source file='/var/lib/libvirt/images/filevm.qcow2'"));
        // (the NIC is always `<interface type='network'>`; assert no network *disk*)
        assert!(!xml.contains("<disk type='network'"));
    }

    #[test]
    fn install_iso_adds_cdrom_boot() {
        let mut vm = VirtualMachine::new("installer", "4Gi");
        vm.metadata.labels = Some(std::collections::HashMap::from([(
            "install_iso".into(),
            "/var/lib/libvirt/images/ubuntu.iso".into(),
        )]));
        let xml = domain_xml_from_spec(
            &vm,
            "/var/lib/libvirt/images/installer.qcow2",
            "qcow2",
            None,
            None,
        )
        .unwrap();
        assert!(xml.contains("boot dev='cdrom'"));
        assert!(xml.contains("ubuntu.iso"));
    }

    #[test]
    fn uefi_install_iso_uses_per_device_boot_not_os_boot() {
        // libvirt rejects a domain mixing <os><boot dev> with per-device
        // <boot order>. For UEFI + install ISO we must use per-device boot only.
        let mut vm = VirtualMachine::new("winst", "4Gi");
        vm.spec.firmware = "uefi".into();
        vm.metadata.labels = Some(std::collections::HashMap::from([(
            "install_iso".into(),
            "/var/lib/libvirt/images/win.iso".into(),
        )]));
        let xml = domain_xml_from_spec(
            &vm,
            "/var/lib/libvirt/images/winst.qcow2",
            "qcow2",
            None,
            None,
        )
        .unwrap();
        // No os/boot elements at all in the UEFI path.
        assert!(
            !xml.contains("<boot dev="),
            "UEFI must not emit <os><boot dev>"
        );
        // cdrom boots first (order 1), installed disk second (order 2).
        assert!(xml.contains("<boot order='1'/>"));
        assert!(xml.contains("<boot order='2'/>"));
        assert!(xml.contains("pflash")); // still UEFI
    }

    #[test]
    fn install_iso_uses_sata_not_ide() {
        // q35 has no IDE controller: libvirt rejected the whole domain with
        // "IDE controllers are unsupported for this QEMU binary or machine type",
        // so create-from-ISO failed before the VM ever existed.
        let mut vm = VirtualMachine::new("isovm", "4Gi");
        vm.metadata.labels = Some(std::collections::HashMap::from([(
            "install_iso".into(),
            "/var/lib/libvirt/images/ubuntu.iso".into(),
        )]));
        let xml = domain_xml_from_spec(
            &vm,
            "/var/lib/libvirt/images/isovm.qcow2",
            "qcow2",
            None,
            None,
        )
        .unwrap();
        assert!(xml.contains("machine='q35'"));
        assert!(!xml.contains("bus='ide'"), "q35 cannot take an IDE CD-ROM");
        assert!(xml.contains("<target dev='sda' bus='sata'/>"));
    }

    #[test]
    fn install_and_cloud_init_isos_get_distinct_targets() {
        // Both are CD-ROMs on the same SATA bus; reusing a target makes libvirt
        // reject the definition with "target sdX already exists".
        let mut vm = VirtualMachine::new("bothiso", "4Gi");
        vm.metadata.labels = Some(std::collections::HashMap::from([(
            "install_iso".into(),
            "/var/lib/libvirt/images/ubuntu.iso".into(),
        )]));
        let xml = domain_xml_from_spec(
            &vm,
            "/var/lib/libvirt/images/bothiso.qcow2",
            "qcow2",
            Some("/var/lib/libvirt/images/seed.iso"),
            None,
        )
        .unwrap();
        assert!(xml.contains("<target dev='sda' bus='sata'/>"));
        assert!(xml.contains("<target dev='sdb' bus='sata'/>"));
    }

    #[test]
    fn secure_boot_label_emits_secure_loader_and_smm() {
        let mut vm = VirtualMachine::new("win11", "8Gi");
        vm.spec.firmware = "uefi".into();
        vm.metadata.labels = Some(std::collections::HashMap::from([(
            "secure_boot".into(),
            "true".into(),
        )]));
        let xml = domain_xml_from_spec(
            &vm,
            "/var/lib/libvirt/images/win11.qcow2",
            "qcow2",
            None,
            None,
        )
        .unwrap();
        assert!(xml.contains("secure='yes'"));
        assert!(xml.contains("OVMF_CODE.secboot.fd"));
        assert!(xml.contains("OVMF_VARS.secboot.fd"));
        // libvirt rejects secure='yes' without SMM.
        assert!(xml.contains("<smm state='on'/>"));
    }

    #[test]
    fn uefi_without_secure_boot_keeps_plain_loader_and_no_smm() {
        let mut vm = VirtualMachine::new("plainuefi", "4Gi");
        vm.spec.firmware = "uefi".into();
        let xml = domain_xml_from_spec(&vm, "/var/lib/libvirt/images/p.qcow2", "qcow2", None, None)
            .unwrap();
        assert!(xml.contains("pflash"));
        assert!(!xml.contains("secure='yes'"));
        assert!(!xml.contains("secboot"));
        assert!(!xml.contains("<smm"));
    }

    #[test]
    fn secure_boot_ignored_on_bios_firmware() {
        // Secure Boot is meaningless without UEFI; a BIOS guest must not get an
        // SMM feature block that its firmware path can't honour.
        let mut vm = VirtualMachine::new("biosvm", "4Gi");
        vm.spec.firmware = "bios".into();
        vm.metadata.labels = Some(std::collections::HashMap::from([(
            "secure_boot".into(),
            "true".into(),
        )]));
        let xml = domain_xml_from_spec(&vm, "/var/lib/libvirt/images/b.qcow2", "qcow2", None, None)
            .unwrap();
        assert!(!xml.contains("secure='yes'"));
        assert!(!xml.contains("<smm"));
        assert!(!xml.contains("pflash"));
    }
}
