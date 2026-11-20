// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Guest OS detection and remote-access probing, shared by the daemon (classic
//! mode) and the agent (platform mode).
//!
//! Both console planners used to carry their own copy of this, and both only
//! matched the literal strings "microsoft windows" / "<os>windows" — which
//! libvirt does not normally emit — so every Windows guest was reported as
//! Linux and offered an `ssh ubuntu@…` line it would refuse. One implementation
//! means fixing it once.

use std::time::Duration;

/// Guess the guest OS from the libvirt domain XML and VM name.
///
/// Signals, strongest first:
///   * libosinfo metadata — `<libosinfo:os id="http://microsoft.com/win/11"/>`
///   * Hyper-V enlightenments — only ever enabled for Windows guests
///   * `<clock offset='localtime'>` — the Windows convention (Linux uses UTC)
///   * disk image / domain names, for hand-rolled domains carrying none of the above
///
/// Returns `windows`, `linux`, or `unknown` (empty XML only).
pub fn detect_os_hint(xml: &str, vm_name: &str) -> String {
    if xml.trim().is_empty() {
        return "unknown".into();
    }
    let lower = xml.to_ascii_lowercase();

    if lower.contains("microsoft.com/win")
        || lower.contains("microsoft windows")
        || lower.contains("<os>windows")
        || lower.contains("<hyperv")
        || lower.contains("offset='localtime'")
        || lower.contains("offset=\"localtime\"")
    {
        return "windows".into();
    }

    let name_lower = vm_name.to_ascii_lowercase();
    const WINDOWS_WORDS: [&str; 7] = [
        "windows", "win10", "win11", "win7", "win2019", "win2022", "msedge",
    ];
    // Only the VM's own name, not the whole XML blob: matching against disk
    // paths/description/comments too made a Linux VM with, say, a disk named
    // `template-windows-compat.qcow2` misclassify as Windows — which is enough
    // to reach the RDP-reachability probe and offer a phantom "rdp" console
    // option for a VM that was never Windows at all. `.vhdx` is kept as a
    // narrow, genuine technical signal (a Hyper-V/Windows-only disk format),
    // not a name/keyword match.
    if WINDOWS_WORDS.iter().any(|w| name_lower.contains(w)) || lower.contains(".vhdx") {
        return "windows".into();
    }

    "linux".into()
}

/// Refine an OS guess with what the guest agent reports, when it is installed.
pub fn refine_os_hint(current: &str, os_pretty_name: &str) -> String {
    let lower = os_pretty_name.to_ascii_lowercase();
    if lower.contains("windows") {
        return "windows".into();
    }
    if current == "unknown" && !lower.trim().is_empty() {
        return "linux".into();
    }
    current.to_string()
}

/// Is the guest listening on RDP right now?
///
/// Remote Desktop is off by default in Windows, so advertising RDP for every
/// Windows guest would hand the operator a button that dials a closed port.
pub fn rdp_reachable(guest_ip: &str) -> bool {
    if guest_ip.trim().is_empty() {
        return false;
    }
    let Ok(addr) = format!("{guest_ip}:3389").parse::<std::net::SocketAddr>() else {
        return false;
    };
    std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(400)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_windows_from_libosinfo_metadata() {
        let xml = r#"<domain><metadata>
            <libosinfo:libosinfo xmlns:libosinfo="http://libosinfo.org/xmlns/libvirt/domain/1.0">
              <libosinfo:os id="http://microsoft.com/win/11"/>
            </libosinfo:libosinfo></metadata></domain>"#;
        assert_eq!(detect_os_hint(xml, "vm1"), "windows");
    }

    #[test]
    fn detects_windows_from_hyperv_enlightenments() {
        let xml = r#"<domain><features><hyperv><relaxed state='on'/></hyperv></features></domain>"#;
        assert_eq!(detect_os_hint(xml, "vm1"), "windows");
    }

    #[test]
    fn detects_windows_from_localtime_clock() {
        assert_eq!(
            detect_os_hint(r#"<domain><clock offset='localtime'/></domain>"#, "vm1"),
            "windows"
        );
    }

    #[test]
    fn detects_windows_from_domain_name() {
        // The regression this module exists for: a Windows guest whose XML carries
        // none of the strong markers was reported as "linux".
        let xml = r#"<domain><devices><graphics type='vnc'/></devices></domain>"#;
        assert_eq!(detect_os_hint(xml, "win10-msedge"), "windows");
    }

    #[test]
    fn a_windows_word_in_a_disk_path_does_not_misclassify_a_linux_vm() {
        // Only the VM's own name is a keyword signal now — matching the whole
        // XML blob let a disk path like this one misclassify an actual Linux VM
        // as Windows, which was enough to reach the RDP probe and offer a
        // phantom "rdp" console option.
        let xml = r#"<domain><clock offset='utc'/>
            <devices><disk><source file='/var/lib/libvirt/images/template-windows-compat.qcow2'/></disk></devices>
        </domain>"#;
        assert_eq!(detect_os_hint(xml, "ubuntu-server"), "linux");
    }

    #[test]
    fn plain_linux_domain_stays_linux() {
        let xml = r#"<domain><clock offset='utc'/>
            <devices><disk><source file='/var/lib/libvirt/images/ubuntu.qcow2'/></disk></devices>
        </domain>"#;
        assert_eq!(detect_os_hint(xml, "ubuntu-server"), "linux");
    }

    #[test]
    fn empty_xml_is_unknown() {
        assert_eq!(detect_os_hint("", "vm1"), "unknown");
        assert_eq!(detect_os_hint("   ", "vm1"), "unknown");
    }

    #[test]
    fn guest_agent_report_refines_the_guess() {
        assert_eq!(refine_os_hint("linux", "Microsoft Windows 11"), "windows");
        assert_eq!(refine_os_hint("unknown", "Ubuntu 24.04 LTS"), "linux");
        // A guest-agent Linux report must not override strong Windows XML markers.
        assert_eq!(refine_os_hint("windows", ""), "windows");
    }

    #[test]
    fn rdp_unreachable_for_empty_or_bogus_ip() {
        assert!(!rdp_reachable(""));
        assert!(!rdp_reachable("not-an-ip"));
    }
}
