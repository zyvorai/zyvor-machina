// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Host virtualization readiness (BIOS KVM, `/dev/kvm`, libvirt socket).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct VirtualizationHostStatus {
    /// CPU exposes VMX/SVM (Intel VT-x / AMD-V) in `/proc/cpuinfo`.
    pub cpu_virt_supported: bool,
    /// `/dev/kvm` exists and is readable by the daemon user (best-effort stat).
    pub kvm_device_present: bool,
    /// `/var/run/libvirt/libvirt-sock` exists (system libvirt).
    pub libvirt_system_socket_present: bool,
    /// Session libvirt socket (path varies; common XDG location checked).
    pub libvirt_session_socket_present: bool,
    /// Hint for UI when KVM device missing.
    pub hint: &'static str,
}

fn proc_cpu_virt() -> bool {
    #[cfg(target_os = "linux")]
    {
        if let Ok(s) = std::fs::read_to_string("/proc/cpuinfo") {
            let lower = s.to_ascii_lowercase();
            return lower.contains("vmx") || lower.contains("svm");
        }
    }
    false
}

pub fn virtualization_status() -> VirtualizationHostStatus {
    let kvm = std::path::Path::new("/dev/kvm");
    let kvm_ok = kvm.exists();

    let sys_sock = std::path::Path::new("/var/run/libvirt/libvirt-sock");
    let sys_sock_ro = std::path::Path::new("/var/run/libvirt/libvirt-sock-ro");

    let session_sock_candidates = [
        format!(
            "{}/libvirt/libvirt-sock",
            std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/0".into())
        ),
        "/run/user/1000/libvirt/libvirt-sock".into(),
    ];

    let mut sess_ok = false;
    for p in &session_sock_candidates {
        if std::path::Path::new(p).exists() {
            sess_ok = true;
            break;
        }
    }

    let hint: &'static str = if !proc_cpu_virt() {
        "Hardware virtualization appears disabled or unavailable (no vmx/svm in /proc/cpuinfo). Enable VT-x/AMD-V in firmware if you need KVM guests."
    } else if !kvm_ok {
        "/dev/kvm missing — load kvm module or check permissions."
    } else if !sys_sock.exists() && !sys_sock_ro.exists() && !sess_ok {
        "Libvirt sockets not found — ensure libvirtd is running or use session URI."
    } else {
        ""
    };

    VirtualizationHostStatus {
        cpu_virt_supported: proc_cpu_virt(),
        kvm_device_present: kvm_ok,
        libvirt_system_socket_present: sys_sock.exists() || sys_sock_ro.exists(),
        libvirt_session_socket_present: sess_ok,
        hint,
    }
}

#[cfg(target_os = "linux")]
fn systemctl_unit_file_state(unit: &str) -> Option<String> {
    let output = std::process::Command::new("systemctl")
        .args(["show", unit, "-p", "UnitFileState", "--value", "--no-pager"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

#[cfg(target_os = "linux")]
fn systemctl_unit_list_has(unit: &str) -> bool {
    std::process::Command::new("systemctl")
        .args(["list-unit-files", "--no-legend", "--no-pager", unit])
        .output()
        .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}

/// Systemd is not set to start a required libvirt daemon at host boot.
#[derive(Debug, Clone, Serialize)]
pub struct LibvirtBootAutostartIssue {
    pub detail: String,
    /// Pass to `systemctl enable --now <unit>` (e.g. `virtnetworkd.service`).
    pub systemd_unit: String,
}

/// When systemd is not configured to start libvirt at boot.
///
/// Libvirt’s own “autostart” for networks and guests only runs **after** the appropriate libvirt
/// daemon starts. If `libvirtd` (or modular `virtnetworkd` / `virtqemud`) is disabled in systemd,
/// nothing autostarts after a host reboot even when Machina shows autostart enabled.
#[cfg(target_os = "linux")]
pub fn libvirt_boot_autostart_issue() -> Option<LibvirtBootAutostartIssue> {
    let bad = |s: &str| s == "disabled" || s == "masked";

    if let Some(ref state) = systemctl_unit_file_state("libvirtd.service") {
        if bad(state) {
            return Some(LibvirtBootAutostartIssue {
                detail: "libvirtd.service is not enabled to start at boot. Until libvirt runs, libvirt autostart (networks and VMs) does not run after a reboot."
                    .to_string(),
                systemd_unit: "libvirtd.service".to_string(),
            });
        }
    }

    if systemctl_unit_list_has("virtnetworkd.service") {
        if let Some(ref state) = systemctl_unit_file_state("virtnetworkd.service") {
            if bad(state) {
                return Some(LibvirtBootAutostartIssue {
                    detail: "virtnetworkd.service is not enabled at boot (modular libvirt). NAT networks will not come up after reboot."
                        .to_string(),
                    systemd_unit: "virtnetworkd.service".to_string(),
                });
            }
        }
    }

    if systemctl_unit_list_has("virtqemud.service") {
        if let Some(ref state) = systemctl_unit_file_state("virtqemud.service") {
            if bad(state) {
                return Some(LibvirtBootAutostartIssue {
                    detail: "virtqemud.service is not enabled at boot (modular libvirt). Guests with autostart will not start after reboot."
                        .to_string(),
                    systemd_unit: "virtqemud.service".to_string(),
                });
            }
        }
    }

    None
}

#[cfg(not(target_os = "linux"))]
pub fn libvirt_boot_autostart_issue() -> Option<LibvirtBootAutostartIssue> {
    None
}
