// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Structured libvirt domain hardware summary for Machina Hardware UI.

use serde::{Deserialize, Serialize};
use virt::connect::Connect;
use virt::sys::VIR_DOMAIN_XML_INACTIVE;

use super::cpu_memory::{get_cpu_memory_topology, has_vfio_hostdev};
use super::domain::{get_vm_details, lookup_domain};
use super::guest_health::gather_guest_health;
use super::pending_config::get_pending_config;
use crate::LibvirtError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSection {
    pub label: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub badges: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowsReadinessItem {
    pub label: String,
    pub status: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowsReadinessReport {
    pub is_windows: bool,
    pub items: Vec<WindowsReadinessItem>,
    pub ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatIssue {
    pub severity: String,
    pub category: String,
    pub message: String,
    pub badges: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareCompatReport {
    pub ok: bool,
    pub issues: Vec<CompatIssue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cpu_modes_supported: Vec<String>,
    pub tpm_supported: bool,
    pub uefi_supported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainCapabilitiesReport {
    pub arch: String,
    pub virttype: String,
    pub cpu_modes_supported: Vec<String>,
    pub machine_types: Vec<String>,
    pub tpm_supported: bool,
    pub uefi_supported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmHardwareSummaryReport {
    pub vm_name: String,
    pub state: String,
    pub cpu: HardwareSection,
    pub memory: HardwareSection,
    pub firmware: HardwareSection,
    pub tpm: HardwareSection,
    pub display: HardwareSection,
    pub video: HardwareSection,
    pub disk_bus: HardwareSection,
    pub nic: HardwareSection,
    pub guest_agent: HardwareSection,
    pub host_devices: HardwareSection,
    pub migration: HardwareSection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows_readiness: Option<WindowsReadinessReport>,
    pub balloon_enabled: bool,
    pub secure_boot: bool,
    pub has_vfio_hostdev: bool,
    pub needs_shutdown: bool,
}

fn extract_attr(block: &str, tag: &str, attr: &str) -> Option<String> {
    crate::xml::extract_attr(block, tag, attr)
}

fn count_tag(xml: &str, tag: &str) -> usize {
    let re_pattern = format!("<{tag}");
    xml.match_indices(&re_pattern)
        .filter(|(i, _)| {
            let after = &xml[*i + tag.len() + 1..];
            matches!(after.chars().next(), Some(' ') | Some('>') | Some('/'))
        })
        .count()
}

fn parse_cpu_mode(xml: &str) -> (String, Option<String>) {
    let block = xml
        .match_indices("<cpu")
        .find_map(|(i, _)| {
            let slice = &xml[i..];
            if slice.starts_with("<cpu ")
                || slice.starts_with("<cpu>")
                || slice.starts_with("<cpu\n")
            {
                Some(
                    slice
                        .find("</cpu>")
                        .map(|e| &slice[..e + 6])
                        .unwrap_or_else(|| {
                            slice.find("/>").map(|e| &slice[..e + 2]).unwrap_or(slice)
                        }),
                )
            } else {
                None
            }
        })
        .unwrap_or("");
    let mode = extract_attr(block, "cpu", "mode").unwrap_or_else(|| "custom".into());
    let model =
        extract_attr(block, "model", "fallback").or_else(|| extract_attr(block, "cpu", "model"));
    (mode, model)
}

fn parse_firmware(xml: &str) -> (String, bool) {
    let os_block = xml
        .match_indices("<os")
        .find_map(|(i, _)| {
            let slice = &xml[i..];
            slice
                .find("</os>")
                .map(|e| &slice[..e + 5])
                .or_else(|| slice.find("/>").map(|e| &slice[..e + 2]))
        })
        .unwrap_or("");
    let fw = extract_attr(os_block, "os", "firmware").unwrap_or_default();
    let loader = os_block.to_ascii_lowercase();
    let secure = loader.contains("secure='yes'")
        || loader.contains("secure=\"yes\"")
        || loader.contains("secure=yes");
    let uefi = fw.to_ascii_lowercase().contains("efi")
        || loader.contains("ovmf")
        || loader.contains("edk2")
        || secure;
    let label = if uefi {
        if secure {
            "UEFI · Secure Boot enabled".into()
        } else {
            "UEFI".into()
        }
    } else {
        "BIOS".into()
    };
    (label, secure && uefi)
}

fn parse_tpm(xml: &str) -> (String, bool) {
    if !xml.contains("<tpm") {
        return ("Not configured".into(), false);
    }
    let block = xml
        .match_indices("<tpm")
        .next()
        .map(|(i, _)| {
            let slice = &xml[i..];
            slice
                .find("</tpm>")
                .map(|e| &slice[..e + 6])
                .unwrap_or(slice)
        })
        .unwrap_or("");
    let model = extract_attr(block, "tpm", "model").unwrap_or_else(|| "emulator".into());
    let persistent = if block.contains("persistent") {
        " · persistent"
    } else {
        ""
    };
    let tpm2 = model.contains("crb") || model.contains("tpm2") || block.contains("version='2.0'");
    let label = if tpm2 {
        format!("TPM 2.0 emulator{persistent}")
    } else {
        format!("TPM {model}{persistent}")
    };
    (label, tpm2)
}

fn parse_video(xml: &str) -> String {
    let block = xml
        .match_indices("<video")
        .next()
        .map(|(i, _)| &xml[i..])
        .and_then(|slice| slice.find("/>").map(|e| &slice[..e + 2]))
        .unwrap_or("");
    extract_attr(block, "video", "model")
        .or_else(|| extract_attr(block, "model", "type"))
        .unwrap_or_else(|| "default".into())
}

fn parse_display(xml: &str) -> String {
    let mut parts = Vec::new();
    if xml.contains("type='spice'") || xml.contains("type=\"spice\"") {
        parts.push("SPICE");
    }
    if xml.contains("type='vnc'") || xml.contains("type=\"vnc\"") {
        parts.push("VNC");
    }
    if xml.contains("type='rdp'") || xml.contains("type=\"rdp\"") {
        parts.push("RDP");
    }
    if parts.is_empty() {
        "none".into()
    } else {
        parts.join(" / ")
    }
}

fn parse_balloon(xml: &str) -> bool {
    xml.contains("<memballoon") || xml.contains("<memballoon ")
}

fn is_windows_os(os_hint: &str, os_pretty: Option<&str>) -> bool {
    let combined = format!(
        "{} {}",
        os_hint.to_ascii_lowercase(),
        os_pretty.unwrap_or("").to_ascii_lowercase()
    );
    combined.contains("windows") || combined.contains("win11") || combined.contains("win10")
}

fn virtio_disk_bus(details: &crate::state::VmDetails) -> String {
    let buses: Vec<String> = details
        .disks
        .iter()
        .filter(|d| d.device == "disk")
        .map(|d| {
            if d.bus.is_empty() {
                d.driver.clone()
            } else {
                d.bus.clone()
            }
        })
        .filter(|b| !b.is_empty())
        .collect();
    let uniq: Vec<String> = buses.into_iter().fold(Vec::new(), |mut acc, b| {
        if !acc.contains(&b) {
            acc.push(b);
        }
        acc
    });
    if uniq.is_empty() {
        "virtio".into()
    } else {
        uniq.join(", ")
    }
}

fn nic_summary(details: &crate::state::VmDetails) -> String {
    if details.interfaces.is_empty() {
        return "none".into();
    }
    details
        .interfaces
        .iter()
        .take(2)
        .map(|i| {
            format!(
                "{} · {}",
                if i.source.is_empty() {
                    "network"
                } else {
                    i.source.as_str()
                },
                if i.model.is_empty() {
                    "virtio"
                } else {
                    i.model.as_str()
                }
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn get_hardware_summary(
    conn: &Connect,
    name: &str,
) -> Result<VmHardwareSummaryReport, LibvirtError> {
    let details = get_vm_details(conn, name)?;
    let topology = get_cpu_memory_topology(conn, name).ok();
    let pending = get_pending_config(conn, name).ok();
    let guest = gather_guest_health(conn, name).ok();

    let domain = lookup_domain(conn, name)?;
    let active_xml = domain
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("Failed to get active XML"))?;
    let config_xml = domain
        .get_xml_desc(VIR_DOMAIN_XML_INACTIVE)
        .unwrap_or_else(|_| active_xml.clone());

    let (cpu_mode, cpu_model) = parse_cpu_mode(&config_xml);
    let vcpus = topology.as_ref().map(|t| t.vcpus).unwrap_or(details.vcpus);
    let cpu_value = match cpu_model {
        Some(m) => format!("{vcpus} vCPU · {cpu_mode} · {m}"),
        None => format!("{vcpus} vCPU · {cpu_mode}"),
    };
    let mut cpu_badges = Vec::new();
    if pending.as_ref().is_some_and(|p| p.needs_shutdown) {
        cpu_badges.push("restart_required".into());
    }

    let mem_gib = topology
        .as_ref()
        .map(|t| t.current_memory_kib / 1024 / 1024)
        .unwrap_or(details.memory_mb / 1024);
    let balloon = parse_balloon(&config_xml);
    let memory_value = if balloon {
        format!("{mem_gib} GiB · balloon enabled")
    } else {
        format!("{mem_gib} GiB")
    };
    let mut mem_badges = Vec::new();
    if balloon {
        mem_badges.push("live".into());
    }
    if pending.as_ref().is_some_and(|p| p.needs_shutdown) {
        mem_badges.push("restart_required".into());
    }

    let (firmware_value, secure_boot) = parse_firmware(&config_xml);
    let fw_badges = vec!["restart_required".into()];

    let os_pretty = guest.as_ref().and_then(|g| g.os_pretty_name.as_deref());
    let is_windows = is_windows_os(&details.os_type, os_pretty);

    let (tpm_value, tpm2) = parse_tpm(&config_xml);
    let tpm_badges = if tpm_value == "Not configured" {
        let mut b = vec!["restart_required".to_string()];
        if is_windows {
            b.push("windows_recommended".into());
        }
        b
    } else {
        vec!["restart_required".into()]
    };

    let display_value = parse_display(&config_xml);
    let video_value = parse_video(&config_xml);
    let disk_bus = virtio_disk_bus(&details);
    let nic = nic_summary(&details);

    let agent_running = guest
        .as_ref()
        .map(|g| g.agent_reachable && g.healthy)
        .unwrap_or(false);
    let guest_agent_value = if agent_running {
        "running"
    } else if guest.as_ref().is_some_and(|g| g.agent_reachable) {
        "reachable"
    } else {
        "missing"
    };

    let hostdev_count = count_tag(&config_xml, "hostdev");
    let host_devices_value = if hostdev_count > 0 {
        format!("{hostdev_count} attached")
    } else {
        "none".into()
    };
    let mut hostdev_badges = Vec::new();
    if hostdev_count > 0 {
        hostdev_badges.push("restart_required".into());
        hostdev_badges.push("migration_unsafe".into());
        hostdev_badges.push("advanced".into());
    }

    let vfio = topology
        .as_ref()
        .map(|t| t.has_vfio_hostdev)
        .unwrap_or_else(|| has_vfio_hostdev(&config_xml));
    let needs_shutdown = pending.as_ref().is_some_and(|p| p.needs_shutdown);
    let migration_value = if vfio {
        "warning · VFIO passthrough"
    } else if needs_shutdown {
        "pending restart"
    } else {
        "safe"
    };
    let mut migration_badges = Vec::new();
    if vfio {
        migration_badges.push("migration_unsafe".into());
    }

    let windows_readiness = if is_windows {
        Some(build_windows_readiness(
            tpm2,
            &firmware_value,
            secure_boot,
            agent_running,
            &disk_bus,
            &display_value,
        ))
    } else {
        None
    };

    Ok(VmHardwareSummaryReport {
        vm_name: name.to_string(),
        state: details.state,
        cpu: HardwareSection {
            label: "CPU".into(),
            value: cpu_value,
            badges: cpu_badges,
        },
        memory: HardwareSection {
            label: "Memory".into(),
            value: memory_value,
            badges: mem_badges,
        },
        firmware: HardwareSection {
            label: "Firmware".into(),
            value: firmware_value,
            badges: fw_badges,
        },
        tpm: HardwareSection {
            label: "TPM".into(),
            value: tpm_value,
            badges: tpm_badges,
        },
        display: HardwareSection {
            label: "Display".into(),
            value: display_value.clone(),
            badges: vec![],
        },
        video: HardwareSection {
            label: "Video".into(),
            value: video_value,
            badges: vec![],
        },
        disk_bus: HardwareSection {
            label: "Disk bus".into(),
            value: disk_bus,
            badges: vec![],
        },
        nic: HardwareSection {
            label: "NIC".into(),
            value: nic,
            badges: vec![],
        },
        guest_agent: HardwareSection {
            label: "Guest agent".into(),
            value: guest_agent_value.into(),
            badges: if !agent_running && is_windows {
                vec!["windows_recommended".into()]
            } else {
                vec![]
            },
        },
        host_devices: HardwareSection {
            label: "Host devices".into(),
            value: host_devices_value,
            badges: hostdev_badges,
        },
        migration: HardwareSection {
            label: "Migration".into(),
            value: migration_value.into(),
            badges: migration_badges,
        },
        windows_readiness,
        balloon_enabled: balloon,
        secure_boot,
        has_vfio_hostdev: vfio,
        needs_shutdown,
    })
}

fn build_windows_readiness(
    tpm2: bool,
    firmware: &str,
    secure_boot: bool,
    agent_ok: bool,
    disk_bus: &str,
    display: &str,
) -> WindowsReadinessReport {
    let uefi_ok = firmware.contains("UEFI");
    let virtio_ok = disk_bus.contains("virtio");
    let items = vec![
        WindowsReadinessItem {
            label: "TPM 2.0".into(),
            status: if tpm2 {
                "Enabled".into()
            } else {
                "Missing".into()
            },
            ok: tpm2,
        },
        WindowsReadinessItem {
            label: "UEFI".into(),
            status: if uefi_ok {
                "Enabled".into()
            } else {
                "Missing".into()
            },
            ok: uefi_ok,
        },
        WindowsReadinessItem {
            label: "Secure Boot".into(),
            status: if secure_boot {
                "Enabled".into()
            } else if uefi_ok {
                "Disabled".into()
            } else {
                "Missing".into()
            },
            ok: secure_boot,
        },
        WindowsReadinessItem {
            label: "VirtIO drivers".into(),
            status: if virtio_ok {
                "Installed".into()
            } else {
                "Missing".into()
            },
            ok: virtio_ok,
        },
        WindowsReadinessItem {
            label: "Guest agent".into(),
            status: if agent_ok {
                "Running".into()
            } else {
                "Missing".into()
            },
            ok: agent_ok,
        },
        WindowsReadinessItem {
            label: "Display".into(),
            status: if display.contains("VNC") {
                "VNC fallback".into()
            } else {
                display.into()
            },
            ok: true,
        },
    ];
    let ready = items.iter().filter(|i| i.label != "Display").all(|i| i.ok);
    WindowsReadinessReport {
        is_windows: true,
        items,
        ready,
    }
}

pub fn get_domain_capabilities_xml(
    conn: &Connect,
    emulatorbin: Option<&str>,
    arch: Option<&str>,
    machine: Option<&str>,
    virttype: Option<&str>,
) -> Result<String, LibvirtError> {
    conn.get_domain_capabilities(emulatorbin, arch, machine, virttype, 0)
        .map_err(LibvirtError::map_op("Failed to get domain capabilities"))
}

fn caps_cpu_modes(xml: &str) -> Vec<String> {
    let mut modes = Vec::new();
    for block in crate::xml::split_blocks(xml, "mode") {
        if let Some(name) = extract_attr(&block, "mode", "name") {
            let supported = extract_attr(&block, "mode", "supported").unwrap_or_default();
            if supported.eq_ignore_ascii_case("yes") {
                modes.push(name);
            }
        }
    }
    modes
}

fn caps_feature_supported(xml: &str, feature: &str) -> bool {
    let patterns = [
        format!("<{feature} supported='yes'"),
        format!("<{feature} supported=\"yes\""),
        format!("<{feature} supported='yes'/>"),
    ];
    patterns.iter().any(|p| xml.contains(p.as_str()))
}

fn caps_machine_types(xml: &str) -> Vec<String> {
    let mut machines = Vec::new();
    for block in crate::xml::split_blocks(xml, "machine") {
        if let Some(name) = extract_attr(&block, "machine", "name") {
            let canonical = extract_attr(&block, "machine", "canonical").unwrap_or_default();
            if (canonical.eq_ignore_ascii_case("yes") || machines.is_empty())
                && !machines.contains(&name)
            {
                machines.push(name);
            }
        }
    }
    machines
}

pub fn get_domain_capabilities_report(
    conn: &Connect,
    arch: Option<&str>,
) -> Result<DomainCapabilitiesReport, LibvirtError> {
    let caps_xml = get_domain_capabilities_xml(conn, None, arch, None, Some("kvm"))?;
    let arch_name = arch.unwrap_or("x86_64").to_string();
    let cpu_modes = caps_cpu_modes(&caps_xml);
    Ok(DomainCapabilitiesReport {
        arch: arch_name,
        virttype: "kvm".into(),
        cpu_modes_supported: cpu_modes,
        machine_types: caps_machine_types(&caps_xml),
        tpm_supported: caps_feature_supported(&caps_xml, "tpm"),
        uefi_supported: caps_xml.contains("firmware='efi'")
            || caps_xml.contains("firmware=\"efi\"")
            || caps_xml.contains("<enum name='efi'"),
    })
}

pub fn check_hardware_compat(
    conn: &Connect,
    name: &str,
) -> Result<HardwareCompatReport, LibvirtError> {
    let details = get_vm_details(conn, name)?;
    let domain = lookup_domain(conn, name)?;
    let xml = domain
        .get_xml_desc(VIR_DOMAIN_XML_INACTIVE)
        .or_else(|_| domain.get_xml_desc(0))
        .map_err(LibvirtError::map_op("Failed to get domain XML"))?;

    let arch = if details.arch.is_empty() {
        None
    } else {
        Some(details.arch.as_str())
    };
    let caps_xml = get_domain_capabilities_xml(conn, None, arch, None, Some("kvm")).ok();
    let cpu_modes = caps_xml
        .as_ref()
        .map(|x| caps_cpu_modes(x))
        .unwrap_or_default();
    // Fail closed when the host's domain capabilities XML couldn't be fetched at
    // all (get_domain_capabilities_xml failed, `caps_xml` is None): assume the
    // feature is NOT supported rather than defaulting to "supported". The old
    // `unwrap_or(true)` silently suppressed the tpm/uefi compat warnings below
    // whenever capabilities were unavailable — the exact case where the operator
    // most needs the warning, since compatibility genuinely could not be verified.
    let tpm_supported = caps_xml
        .as_ref()
        .map(|x| caps_feature_supported(x, "tpm"))
        .unwrap_or(false);
    let uefi_supported = caps_xml
        .as_ref()
        .map(|x| {
            x.contains("firmware='efi'")
                || x.contains("firmware=\"efi\"")
                || x.contains("<enum name='efi'")
        })
        .unwrap_or(false);

    let (cpu_mode, _) = parse_cpu_mode(&xml);
    let (tpm_value, _) = parse_tpm(&xml);
    let (firmware_value, _) = parse_firmware(&xml);
    let vfio = has_vfio_hostdev(&xml);

    let mut issues = Vec::new();
    if !cpu_modes.is_empty() && !cpu_modes.iter().any(|m| m == &cpu_mode) {
        issues.push(CompatIssue {
            severity: "error".into(),
            category: "cpu".into(),
            message: format!("CPU mode '{cpu_mode}' is not supported on this host"),
            badges: vec!["host_support_required".into()],
        });
    }
    if tpm_value != "Not configured" && !tpm_supported {
        issues.push(CompatIssue {
            severity: "warn".into(),
            category: "tpm".into(),
            message: "TPM is configured but host domain capabilities do not advertise TPM support"
                .into(),
            badges: vec!["host_support_required".into()],
        });
    }
    if firmware_value.contains("UEFI") && !uefi_supported {
        issues.push(CompatIssue {
            severity: "warn".into(),
            category: "firmware".into(),
            message: "UEFI firmware may not be supported on this host".into(),
            badges: vec!["host_support_required".into()],
        });
    }
    if vfio {
        issues.push(CompatIssue {
            severity: "warn".into(),
            category: "migration".into(),
            message: "VFIO passthrough devices block live migration".into(),
            badges: vec!["migration_unsafe".into()],
        });
    }

    Ok(HardwareCompatReport {
        ok: issues.iter().all(|i| i.severity != "error"),
        issues,
        cpu_modes_supported: cpu_modes,
        tpm_supported,
        uefi_supported,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tpm_and_firmware() {
        let xml = r#"<domain><os firmware='efi'><loader secure='yes'/></os><tpm model='tpm-crb'><backend type='emulator' version='2.0'/></tpm></domain>"#;
        let (fw, sb) = parse_firmware(xml);
        assert!(fw.contains("UEFI"));
        assert!(sb);
        let (tpm, tpm2) = parse_tpm(xml);
        assert!(tpm.contains("TPM 2.0"));
        assert!(tpm2);
    }
}
