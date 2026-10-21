// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! iSCSI storage pools. A libvirt `iscsi` pool logs in to one target and exposes each LUN the array
//! presents as a volume; LUNs are provisioned on the array, not by Machina, so volumes here are only
//! listed and attached. A VM gets a LUN as a raw block disk (`attach_lun`).
//!
//! Everything that becomes XML or a command argument is validated first: the target address and
//! IQNs, the CHAP user and secret. The CHAP secret is stored as a libvirt secret (usage
//! `machina-iscsi-<pool>`) and the pool refers to it by usage, so it never appears in the pool XML.
//! Discovery runs `iscsiadm` without a shell.

use std::net::IpAddr;

use serde::Serialize;
use virt::connect::Connect;
use virt::storage_pool::StoragePool;
use virt::storage_vol::StorageVol;

use super::device::get_domain_flags_pub;
use super::domain::lookup_domain;
use crate::LibvirtError;

pub const DEFAULT_PORT: u16 = 3260;
/// Where udev names iSCSI LUNs; libvirt resolves pool volumes to paths under it.
pub const POOL_TARGET_PATH: &str = "/dev/disk/by-path";
const BUSES: [&str; 3] = ["virtio", "scsi", "sata"];

fn bad(msg: impl Into<String>) -> LibvirtError {
    LibvirtError::Invalid(msg.into())
}

#[derive(Debug, Clone)]
pub struct Chap {
    pub username: String,
    pub secret: String,
}

/// What it takes to log in to a target.
#[derive(Debug, Clone)]
pub struct IscsiSpec {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub target_iqn: String,
    pub initiator_iqn: Option<String>,
    pub chap: Option<Chap>,
}

/// An IP address or a hostname (letters, digits, `.`, `-`), never starting with `-`.
pub fn valid_host(h: &str) -> bool {
    h.parse::<IpAddr>().is_ok()
        || (!h.is_empty()
            && h.len() <= 253
            && !h.starts_with(['-', '.'])
            && h.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.'))
}

/// `iqn.`, `eui.` or `naa.` names (RFC 3720), restricted to the characters they use.
pub fn valid_iqn(s: &str) -> bool {
    let known = ["iqn.", "eui.", "naa."].iter().any(|p| s.starts_with(p));
    known
        && s.len() <= 223
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '_'))
}

fn valid_chap_user(u: &str) -> bool {
    !u.is_empty()
        && u.len() <= 64
        && u.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '@' | ':' | '-'))
}

/// 12 to 64 printable ASCII characters (iSCSI CHAP secrets are at least 12 bytes).
fn valid_chap_secret(s: &str) -> bool {
    (12..=64).contains(&s.len()) && s.chars().all(|c| c.is_ascii_graphic())
}

impl IscsiSpec {
    pub fn validate(&self) -> Result<(), LibvirtError> {
        crate::validate::validate_name(&self.name)?;
        if !valid_host(&self.host) {
            return Err(bad(format!("invalid iSCSI portal address '{}'", self.host)));
        }
        if self.port == 0 {
            return Err(bad("iSCSI port must be 1 to 65535"));
        }
        if !valid_iqn(&self.target_iqn) {
            return Err(bad(format!("invalid target IQN '{}'", self.target_iqn)));
        }
        if let Some(i) = &self.initiator_iqn {
            if !valid_iqn(i) {
                return Err(bad(format!("invalid initiator IQN '{i}'")));
            }
        }
        if let Some(c) = &self.chap {
            if !valid_chap_user(&c.username) {
                return Err(bad("invalid CHAP username"));
            }
            if !valid_chap_secret(&c.secret) {
                return Err(bad("the CHAP secret must be 12 to 64 printable characters"));
            }
        }
        Ok(())
    }
}

/// The libvirt secret usage name for a pool's CHAP password.
pub fn secret_usage(pool_name: &str) -> String {
    format!("machina-iscsi-{pool_name}")
}

pub fn secret_xml(usage: &str) -> String {
    format!(
        "<secret ephemeral='no' private='yes'>\n  <description>CHAP secret for the Machina iSCSI pool {u}</description>\n  <usage type='iscsi'>\n    <target>{u}</target>\n  </usage>\n</secret>",
        u = crate::xml::escape(usage)
    )
}

/// The pool definition. `secret_usage` is set when CHAP is used.
pub fn pool_xml(spec: &IscsiSpec, secret_usage: Option<&str>) -> String {
    let initiator = spec
        .initiator_iqn
        .as_ref()
        .map(|i| {
            format!(
                "\n    <initiator><iqn name='{}'/></initiator>",
                crate::xml::escape(i)
            )
        })
        .unwrap_or_default();
    let auth = match (&spec.chap, secret_usage) {
        (Some(c), Some(u)) => format!(
            "\n    <auth type='chap' username='{}'><secret usage='{}'/></auth>",
            crate::xml::escape(&c.username),
            crate::xml::escape(u)
        ),
        _ => String::new(),
    };
    format!(
        "<pool type='iscsi'>\n  <name>{name}</name>\n  <source>\n    <host name='{host}' port='{port}'/>\n    <device path='{iqn}'/>{initiator}{auth}\n  </source>\n  <target><path>{target}</path></target>\n</pool>",
        name = crate::xml::escape(&spec.name),
        host = crate::xml::escape(&spec.host),
        port = spec.port,
        iqn = crate::xml::escape(&spec.target_iqn),
        target = POOL_TARGET_PATH,
    )
}

/// A raw block disk for a LUN path. `none` caching with native io is what shared block storage wants.
pub fn lun_disk_xml(
    path: &str,
    target: &str,
    bus: &str,
    readonly: bool,
    shareable: bool,
) -> String {
    format!(
        "<disk type='block' device='disk'{share}>\n  <driver name='qemu' type='raw' cache='none' io='native'/>\n  <source dev='{path}'/>\n  <target dev='{target}' bus='{bus}'/>{ro}\n</disk>",
        share = if shareable { " shareable='yes'" } else { "" },
        path = crate::xml::escape(path),
        target = crate::xml::escape(target),
        bus = crate::xml::escape(bus),
        ro = if readonly { "\n  <readonly/>" } else { "" },
    )
}

/// `vdb`, `sdc`, `hdd`: the device name the guest sees.
pub fn valid_disk_target(t: &str) -> bool {
    t.len() == 3 && matches!(&t[..2], "vd" | "sd" | "hd") && t.as_bytes()[2].is_ascii_lowercase()
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DiscoveredTarget {
    /// `host:port` as the array reports it.
    pub portal: String,
    pub iqn: String,
}

/// Parse `iscsiadm -m discovery -t sendtargets` output: `10.0.0.20:3260,1 iqn.2001-04.com.x:disk1`.
pub fn parse_discovery(out: &str) -> Vec<DiscoveredTarget> {
    let mut seen: Vec<DiscoveredTarget> = Vec::new();
    for line in out.lines() {
        let mut it = line.split_whitespace();
        let (Some(portal), Some(iqn)) = (it.next(), it.next()) else {
            continue;
        };
        let portal = portal.split(',').next().unwrap_or(portal).to_string();
        if !valid_iqn(iqn) {
            continue;
        }
        let t = DiscoveredTarget {
            portal,
            iqn: iqn.to_string(),
        };
        if !seen.contains(&t) {
            seen.push(t);
        }
    }
    seen
}

/// Ask a portal which targets it offers (`iscsiadm`, which must be installed on this host).
pub fn discover(host: &str, port: u16) -> Result<Vec<DiscoveredTarget>, LibvirtError> {
    if !valid_host(host) || port == 0 {
        return Err(bad("invalid iSCSI portal"));
    }
    let portal = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let out = std::process::Command::new("iscsiadm")
        .args(["-m", "discovery", "-t", "sendtargets", "-p", &portal])
        .output()
        .map_err(|e| {
            LibvirtError::Operation(format!("cannot run iscsiadm (install open-iscsi): {e}"))
        })?;
    if !out.status.success() {
        return Err(LibvirtError::Operation(format!(
            "iSCSI discovery on {portal} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(parse_discovery(&String::from_utf8_lossy(&out.stdout)))
}

/// Define, start and autostart an iSCSI pool, storing the CHAP secret first when there is one.
pub fn create_iscsi_pool(conn: &Connect, spec: &IscsiSpec) -> Result<(), LibvirtError> {
    spec.validate()?;
    let usage = spec.chap.as_ref().map(|_| secret_usage(&spec.name));
    if let (Some(c), Some(u)) = (&spec.chap, &usage) {
        super::secret::define_secret_with_value(
            conn,
            &secret_xml(u),
            Some(c.secret.as_bytes()),
            false,
            0,
        )?;
    }
    let pool =
        StoragePool::define_xml(conn, &pool_xml(spec, usage.as_deref()), 0).map_err(|e| {
            LibvirtError::Operation(format!("Failed to define iSCSI pool '{}': {e}", spec.name))
        })?;
    if let Err(e) = pool.create(0) {
        // Do not leave a defined-but-dead pool behind: the login failed (address, IQN or CHAP).
        let _ = pool.undefine();
        return Err(LibvirtError::Operation(format!(
            "Failed to log in to the iSCSI target for pool '{}': {e}",
            spec.name
        )));
    }
    pool.set_autostart(true).ok();
    Ok(())
}

/// Attach one LUN of an iSCSI pool to a VM as a raw block disk. Only volumes of an `iscsi` pool
/// are accepted, so this cannot be used to hand a guest an arbitrary host device.
#[allow(clippy::too_many_arguments)]
pub fn attach_lun(
    conn: &Connect,
    vm_name: &str,
    pool_name: &str,
    volume: &str,
    target: &str,
    bus: &str,
    readonly: bool,
    shareable: bool,
) -> Result<(), LibvirtError> {
    if !valid_disk_target(target) {
        return Err(bad(format!(
            "invalid disk target '{target}' (for example vdb)"
        )));
    }
    let bus = if bus.is_empty() { "virtio" } else { bus };
    if !BUSES.contains(&bus) {
        return Err(bad(format!(
            "invalid disk bus '{bus}'. Allowed: {}",
            BUSES.join(", ")
        )));
    }
    let pool = StoragePool::lookup_by_name(conn, pool_name)
        .map_err(|e| LibvirtError::NotFound(format!("Pool '{pool_name}' not found: {e}")))?;
    let pool_xml = pool.get_xml_desc(0).unwrap_or_default();
    if super::storage::storage_pool_backend_from_xml(&pool_xml) != "iscsi" {
        return Err(bad(format!("pool '{pool_name}' is not an iSCSI pool")));
    }
    let vol = StorageVol::lookup_by_name(&pool, volume).map_err(|e| {
        LibvirtError::NotFound(format!("LUN '{volume}' not found in '{pool_name}': {e}"))
    })?;
    let path = vol
        .get_path()
        .map_err(|e| LibvirtError::Operation(format!("cannot resolve the LUN path: {e}")))?;
    if !path.starts_with("/dev/") {
        return Err(LibvirtError::Operation(format!(
            "unexpected LUN path '{path}'"
        )));
    }
    let domain = lookup_domain(conn, vm_name)?;
    let flags = get_domain_flags_pub(&domain);
    domain
        .attach_device_flags(
            &lun_disk_xml(&path, target, bus, readonly, shareable),
            flags,
        )
        .map_err(|e| {
            LibvirtError::Operation(format!("Failed to attach LUN to '{vm_name}': {e}"))
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const IQN: &str = "iqn.2001-04.com.example:storage.disk1";

    fn spec() -> IscsiSpec {
        IscsiSpec {
            name: "san1".into(),
            host: "10.0.0.20".into(),
            port: 3260,
            target_iqn: IQN.into(),
            initiator_iqn: Some("iqn.1993-08.org.debian:01:abcd".into()),
            chap: Some(Chap {
                username: "machina".into(),
                secret: "correct-horse-12".into(),
            }),
        }
    }

    #[test]
    fn validators() {
        assert!(valid_host("10.0.0.20") && valid_host("fd00::5") && valid_host("san.example.com"));
        for bad in ["", "-x", ".x", "a b", "a;b", "san/1"] {
            assert!(!valid_host(bad), "{bad}");
        }
        assert!(
            valid_iqn(IQN)
                && valid_iqn("eui.0123456789ABCDEF")
                && valid_iqn("naa.5000c50012345678")
        );
        for bad in ["", "iqn", "foo.bar", "iqn.2001 04", "iqn.x;rm", "-iqn.x"] {
            assert!(!valid_iqn(bad), "{bad}");
        }
        assert!(valid_disk_target("vdb") && valid_disk_target("sdc"));
        for bad in ["vd", "vdbb", "vd1", "xdb", "VDB"] {
            assert!(!valid_disk_target(bad), "{bad}");
        }
        assert!(spec().validate().is_ok());
    }

    #[test]
    fn validation_refuses_bad_specs() {
        let mut s = spec();
        s.host = "--help".into();
        assert!(s.validate().is_err());
        let mut s = spec();
        s.port = 0;
        assert!(s.validate().is_err());
        let mut s = spec();
        s.target_iqn = "not-an-iqn".into();
        assert!(s.validate().is_err());
        let mut s = spec();
        s.chap = Some(Chap {
            username: "u".into(),
            secret: "short".into(),
        });
        assert!(s.validate().is_err());
        let mut s = spec();
        s.chap = Some(Chap {
            username: "bad user".into(),
            secret: "correct-horse-12".into(),
        });
        assert!(s.validate().is_err());
        let mut s = spec();
        s.name = "bad name!".into();
        assert!(s.validate().is_err());
    }

    #[test]
    fn pool_xml_names_the_target_and_refers_to_the_secret_by_usage() {
        let s = spec();
        let usage = secret_usage(&s.name);
        let x = pool_xml(&s, Some(&usage));
        assert!(x.contains("<pool type='iscsi'>"));
        assert!(x.contains("<host name='10.0.0.20' port='3260'/>"));
        assert!(x.contains(&format!("<device path='{IQN}'/>")));
        assert!(x.contains("<initiator><iqn name='iqn.1993-08.org.debian:01:abcd'/></initiator>"));
        assert!(x.contains(
            "<auth type='chap' username='machina'><secret usage='machina-iscsi-san1'/></auth>"
        ));
        assert!(
            !x.contains("correct-horse-12"),
            "the secret must not be in the pool XML"
        );
        assert!(x.contains("<path>/dev/disk/by-path</path>"));
        // No CHAP, no initiator: neither element appears.
        let mut plain = spec();
        plain.chap = None;
        plain.initiator_iqn = None;
        let x = pool_xml(&plain, None);
        assert!(!x.contains("<auth") && !x.contains("<initiator"));
    }

    #[test]
    fn secret_xml_uses_the_iscsi_usage_type() {
        let x = secret_xml("machina-iscsi-san1");
        assert!(
            x.contains("<usage type='iscsi'>") && x.contains("<target>machina-iscsi-san1</target>")
        );
        assert!(x.contains("private='yes'") && x.contains("ephemeral='no'"));
    }

    #[test]
    fn lun_disk_is_a_raw_uncached_block_device() {
        let x = lun_disk_xml(
            "/dev/disk/by-path/ip-10.0.0.20:3260-iscsi-iqn.x:d-lun-1",
            "vdb",
            "virtio",
            false,
            false,
        );
        assert!(x.contains("<disk type='block' device='disk'>"));
        assert!(x.contains("cache='none' io='native'"));
        assert!(
            x.contains("<source dev='/dev/disk/by-path/ip-10.0.0.20:3260-iscsi-iqn.x:d-lun-1'/>")
        );
        assert!(!x.contains("shareable") && !x.contains("<readonly/>"));
        let y = lun_disk_xml("/dev/sdx", "sdc", "scsi", true, true);
        assert!(
            y.contains("shareable='yes'") && y.contains("<readonly/>") && y.contains("bus='scsi'")
        );
    }

    #[test]
    fn discovery_output_is_parsed_and_junk_is_dropped() {
        let out = "10.0.0.20:3260,1 iqn.2001-04.com.example:disk1\n[fd00::5]:3260,2 iqn.2001-04.com.example:disk2\n10.0.0.20:3260,1 iqn.2001-04.com.example:disk1\ngarbage\n10.0.0.21:3260,1 not-an-iqn\n";
        assert_eq!(
            parse_discovery(out),
            vec![
                DiscoveredTarget {
                    portal: "10.0.0.20:3260".into(),
                    iqn: "iqn.2001-04.com.example:disk1".into()
                },
                DiscoveredTarget {
                    portal: "[fd00::5]:3260".into(),
                    iqn: "iqn.2001-04.com.example:disk2".into()
                },
            ]
        );
        assert!(parse_discovery("").is_empty());
        assert!(discover("--evil", 3260).is_err());
        assert!(discover("10.0.0.1", 0).is_err());
    }
}
