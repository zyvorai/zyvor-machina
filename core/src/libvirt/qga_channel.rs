// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Ensure a domain has the virtio-serial channel the QEMU guest agent talks over.
//!
//! Without `org.qemu.guest_agent.0` in the domain XML, libvirt answers every
//! agent call with "QEMU guest agent is not configured" — no guest IP, no OS
//! report, and no way to install a richer in-guest agent. Machina could create
//! VMs with the channel but had no way to add it to a domain that predated the
//! setting, so such VMs were permanently agent-less.

use virt::connect::Connect;

use super::domain::lookup_domain;
use crate::LibvirtError;

pub const QGA_CHANNEL_NAME: &str = "org.qemu.guest_agent.0";

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChannelOutcome {
    /// The channel is present in the domain definition.
    pub present: bool,
    /// This call added it (false when it was already there).
    pub added: bool,
    /// The guest must be restarted before the channel is usable.
    pub requires_restart: bool,
    /// True when a virtio-serial controller had to be added alongside it.
    pub added_controller: bool,
}

/// A real, structurally-valid guest-agent channel, not just the name string
/// appearing somewhere in the XML.
///
/// A bare substring match reports `present: true` for a malformed leftover
/// entry (wrong `<target>` type, or the literal name sitting in a `<title>` or
/// comment) — `ensure_guest_agent_channel` then silently no-ops on a channel
/// that doesn't actually work, instead of adding a real one.
pub fn has_guest_agent_channel(vm_xml: &str) -> bool {
    crate::xml::split_blocks(vm_xml, "channel")
        .iter()
        .any(|block| {
            let channel_type =
                crate::xml::extract_attr(block, "channel", "type").unwrap_or_default();
            let target_type = crate::xml::extract_attr(block, "target", "type").unwrap_or_default();
            let target_name = crate::xml::extract_attr(block, "target", "name").unwrap_or_default();
            channel_type == "unix" && target_type == "virtio" && target_name == QGA_CHANNEL_NAME
        })
}

fn has_virtio_serial_controller(vm_xml: &str) -> bool {
    vm_xml.contains("type='virtio-serial'") || vm_xml.contains("type=\"virtio-serial\"")
}

/// Add the guest-agent channel if it is missing.
///
/// Idempotent: calling it on a domain that already has the channel reports
/// `added: false` rather than attaching a duplicate.
pub fn ensure_guest_agent_channel(
    conn: &Connect,
    name: &str,
) -> Result<ChannelOutcome, LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    // Both views: the channel and its virtio-serial controller are attached to
    // the persistent config while the guest runs, so checking the live XML alone
    // made this re-add a controller that already existed and fail with
    // "controller index='0' already exists".
    let vm_xml = super::domain::domain_xml_live_and_config(&domain);

    if has_guest_agent_channel(&vm_xml) {
        return Ok(ChannelOutcome {
            present: true,
            added: false,
            requires_restart: false,
            added_controller: false,
        });
    }

    let is_running = domain.get_info().map(|i| i.state == 1).unwrap_or(false);
    let both = virt::sys::VIR_DOMAIN_AFFECT_LIVE | virt::sys::VIR_DOMAIN_AFFECT_CONFIG;
    let config_only = virt::sys::VIR_DOMAIN_AFFECT_CONFIG;

    // The channel needs a virtio-serial controller to hang off. Adding one to a
    // running guest is not reliable, so when it is missing we go config-only and
    // tell the caller a restart is required rather than half-applying.
    let mut added_controller = false;
    let mut must_restart = false;
    if !has_virtio_serial_controller(&vm_xml) {
        let controller_xml = "<controller type='virtio-serial' index='0'/>";
        let flags = config_only;
        domain
            .attach_device_flags(controller_xml, flags)
            .map_err(|e| {
                LibvirtError::Operation(format!("failed to add virtio-serial controller: {e}"))
            })?;
        added_controller = true;
        must_restart = is_running;
    }

    let channel_xml = format!(
        r#"<channel type='unix'>
  <target type='virtio' name='{QGA_CHANNEL_NAME}'/>
</channel>"#
    );

    if is_running && !must_restart {
        // Try live+config; a guest that refuses the hotplug still gets it on next boot.
        if domain.attach_device_flags(&channel_xml, both).is_err() {
            domain
                .attach_device_flags(&channel_xml, config_only)
                .map_err(|e| {
                    LibvirtError::Operation(format!("failed to add guest agent channel: {e}"))
                })?;
            must_restart = true;
        }
    } else {
        domain
            .attach_device_flags(&channel_xml, config_only)
            .map_err(|e| {
                LibvirtError::Operation(format!("failed to add guest agent channel: {e}"))
            })?;
        // A stopped guest picks the channel up the moment it starts.
        must_restart = is_running;
    }

    Ok(ChannelOutcome {
        present: true,
        added: true,
        requires_restart: must_restart,
        added_controller,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_an_existing_channel() {
        let xml = r#"<domain><devices>
            <channel type='unix'>
              <target type='virtio' name='org.qemu.guest_agent.0'/>
            </channel>
        </devices></domain>"#;
        assert!(has_guest_agent_channel(xml));
    }

    #[test]
    fn detects_a_domain_without_one() {
        // This is win10-msedge: a Windows guest with no agent channel at all,
        // which is why every QGA call answered "not configured".
        let xml = r#"<domain><devices>
            <disk device='disk'><target dev='sda' bus='sata'/></disk>
            <interface type='network'/>
        </devices></domain>"#;
        assert!(!has_guest_agent_channel(xml));
    }

    #[test]
    fn spots_the_virtio_serial_controller_in_both_quote_styles() {
        assert!(has_virtio_serial_controller(
            "<controller type='virtio-serial' index='0'/>"
        ));
        assert!(has_virtio_serial_controller(
            "<controller type=\"virtio-serial\" index=\"0\"/>"
        ));
        assert!(!has_virtio_serial_controller(
            "<controller type='sata' index='0'/>"
        ));
    }
}

#[cfg(test)]
mod staged_device_tests {
    use super::*;

    #[test]
    fn sees_a_controller_that_exists_only_in_the_persistent_config() {
        // The failure this pins: the controller was added config-only on a
        // running guest, so the live XML lacked it and the next call tried to
        // add it again — libvirt answered "controller index='0' already exists".
        let live = "<domain><devices><disk device='disk'/></devices></domain>";
        let inactive =
            "<domain><devices><controller type='virtio-serial' index='0'/></devices></domain>";
        assert!(!has_virtio_serial_controller(live));
        let combined = format!("{live}\n{inactive}");
        assert!(has_virtio_serial_controller(&combined));
    }

    #[test]
    fn sees_a_channel_that_exists_only_in_the_persistent_config() {
        let live = "<domain><devices><disk device='disk'/></devices></domain>";
        let inactive = "<domain><devices><channel type='unix'><target type='virtio' name='org.qemu.guest_agent.0'/></channel></devices></domain>";
        assert!(!has_guest_agent_channel(live));
        assert!(has_guest_agent_channel(&format!("{live}\n{inactive}")));
    }
}
