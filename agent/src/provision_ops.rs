// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::process::Command;

/// Parse `host:/export/path` for libvirt netfs pools.
fn parse_nfs_source(path: &str) -> anyhow::Result<(String, String)> {
    let path = path.trim();
    let (host, export) = path
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("NFS path must be host:/export (got '{path}')"))?;
    if host.is_empty() {
        anyhow::bail!("NFS host is empty");
    }
    let export = if export.is_empty() {
        "/".into()
    } else if export.starts_with('/') {
        export.to_string()
    } else {
        format!("/{export}")
    };
    Ok((host.to_string(), export))
}

fn local_mount_for_pool(pool_name: &str) -> String {
    format!("/var/lib/machina/nfs/{pool_name}")
}

fn parse_ceph_pool(path: &str) -> String {
    let p = path.trim();
    if let Some(rest) = p.strip_prefix("ceph:") {
        return rest.to_string();
    }
    if let Some(rest) = p.strip_prefix("rbd:") {
        return rest.to_string();
    }
    if p.starts_with("rbd/") {
        return p.to_string();
    }
    format!("rbd/{p}")
}

fn parse_zfs_dataset(path: &str) -> anyhow::Result<(String, String)> {
    let p = path.trim().trim_start_matches('/');
    let (zpool, dataset) = p
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("ZFS path must be zpool/dataset (got '{path}')"))?;
    if zpool.is_empty() || dataset.is_empty() {
        anyhow::bail!("ZFS zpool and dataset must be non-empty");
    }
    Ok((zpool.to_string(), dataset.to_string()))
}

pub fn provision_storage_pool(pool_name: &str, backend: &str, path: &str) -> anyhow::Result<()> {
    // `pool_name` is request-controlled and reaches both `virsh pool-define-as`
    // (leading-dash → argument injection) and root-owned filesystem paths like
    // /var/lib/machina/nfs/{pool_name} (../ → path traversal / create_dir_all as
    // root). Reject anything outside [alnum._-] up front.
    machina_core::validate::validate_name(pool_name).map_err(|e| anyhow::anyhow!("{e}"))?;
    let backend = backend.trim().to_ascii_lowercase();
    let path = path.trim();

    let output = match backend.as_str() {
        "lvm" | "lvm-thin" | "logical" => {
            if !path.starts_with("/dev/") {
                anyhow::bail!("LVM pool path must be a device path under /dev/ (got '{path}')");
            }
            Command::new("virsh")
                .args(["pool-define-as", pool_name, "logical", "--target", path])
                .output()?
        }
        "nfs" | "netfs" => {
            let (host, export) = parse_nfs_source(path)?;
            let target = local_mount_for_pool(pool_name);
            std::fs::create_dir_all(&target)?;
            Command::new("virsh")
                .args([
                    "pool-define-as",
                    pool_name,
                    "netfs",
                    "--source-host",
                    &host,
                    "--source-dir",
                    &export,
                    "--target",
                    &target,
                ])
                .output()?
        }
        "ceph" | "rbd" => {
            let source_dev = parse_ceph_pool(path);
            let target = if source_dev.starts_with("/dev/") {
                source_dev.clone()
            } else {
                format!("/dev/{source_dev}")
            };
            Command::new("virsh")
                .args([
                    "pool-define-as",
                    pool_name,
                    "rbd",
                    "--source-dev",
                    &source_dev,
                    "--target",
                    &target,
                ])
                .output()?
        }
        "iscsi" => {
            if !path.starts_with("iqn.") {
                anyhow::bail!(
                    "iSCSI path must be a target IQN (e.g. iqn.2020-01.com.example:storage)"
                );
            }
            let target = format!("/var/lib/machina/iscsi/{pool_name}");
            std::fs::create_dir_all(&target)?;
            Command::new("virsh")
                .args([
                    "pool-define-as",
                    pool_name,
                    "iscsi",
                    "--source-dev",
                    path,
                    "--target",
                    &target,
                ])
                .output()?
        }
        "zfs" => {
            let (zpool, dataset) = parse_zfs_dataset(path)?;
            let source_dev = format!("{zpool}/{dataset}");
            let target = format!("/{zpool}/{dataset}");
            Command::new("virsh")
                .args([
                    "pool-define-as",
                    pool_name,
                    "zfs",
                    "--source-dev",
                    &source_dev,
                    "--target",
                    &target,
                ])
                .output()?
        }
        "directory" | "dir" => {
            if path.contains(':') {
                anyhow::bail!(
                    "directory backend cannot use host:path NFS syntax — use backend nfs"
                );
            }
            // create_dir_all runs as root — require an absolute path with no `..`
            // so a relative/traversal path can't create dirs outside the target.
            if !std::path::Path::new(path).is_absolute() || path.split('/').any(|c| c == "..") {
                anyhow::bail!("directory pool path must be an absolute path without '..'");
            }
            std::fs::create_dir_all(path)?;
            Command::new("virsh")
                .args(["pool-define-as", pool_name, "dir", "--target", path])
                .output()?
        }
        other => {
            anyhow::bail!(
                "unsupported storage backend '{other}' — use directory, nfs, lvm, ceph, iscsi, or zfs"
            );
        }
    };

    if !output.status.success() {
        anyhow::bail!(
            "virsh pool-define-as failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for sub in ["pool-build", "pool-start"] {
        let out = Command::new("virsh").args([sub, pool_name]).output()?;
        if !out.status.success() {
            let hint = if backend == "nfs" || backend == "netfs" {
                " — verify NFS export is reachable and mount options on the hypervisor"
            } else if backend == "lvm" || backend == "logical" {
                " — verify the logical volume exists and is not in use"
            } else if backend == "ceph" || backend == "rbd" {
                " — verify Ceph cluster, librbd, and pool permissions on the hypervisor"
            } else if backend == "iscsi" {
                " — verify iSCSI target is reachable and libvirt iscsi pool prerequisites"
            } else if backend == "zfs" {
                " — verify ZFS pool/dataset exists and is imported on the host"
            } else {
                ""
            };
            anyhow::bail!(
                "virsh {sub} failed: {}{}",
                String::from_utf8_lossy(&out.stderr),
                hint
            );
        }
    }
    Ok(())
}

pub fn provision_network(
    network_name: &str,
    backend: &str,
    vlan_id: i32,
    bridge: &str,
) -> anyhow::Result<()> {
    // `network_name` reaches `virsh net-start`/`net-autostart` as a positional
    // argument (leading-dash → argument injection); reject anything outside
    // [alnum._-] up front. The escaping/file-safe logic below is kept as
    // defense-in-depth.
    machina_core::validate::validate_name(network_name).map_err(|e| anyhow::anyhow!("{e}"))?;
    // This function only ever defines a libvirt bridge-forward network (optionally
    // VLAN-tagged) below. `backend` used to be accepted and silently discarded, so a
    // caller requesting e.g. "nat"/"isolated"/"vxlan" got a plain bridged network
    // back with no indication anything different happened — a false success that
    // silently drops the requested isolation semantics. Reject anything this code
    // doesn't actually implement instead of pretending to honor it.
    let backend_norm = backend.trim().to_ascii_lowercase();
    if !backend_norm.is_empty() && backend_norm != "bridge" && backend_norm != "linux-bridge" {
        anyhow::bail!(
            "unsupported network backend '{backend}' — this agent only provisions bridge \
             (linux-bridge) networks"
        );
    }
    let bridge_name = if bridge.is_empty() { "virbr0" } else { bridge };
    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
    // Path-safe form for the temp filename: `network_name` is request-controlled,
    // so restrict it to a safe charset to prevent path traversal (e.g. a name of
    // "../../etc/foo" would otherwise write/unlink an arbitrary host path as root).
    let file_safe_name: String = network_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if file_safe_name.is_empty() || file_safe_name == "." || file_safe_name == ".." {
        anyhow::bail!("invalid network name");
    }
    let safe_name = xml_escape(network_name);
    let safe_bridge = xml_escape(bridge_name);
    let xml = if vlan_id > 0 {
        format!(
            "<network><name>{safe_name}</name><bridge name='{safe_bridge}'/><vlan><tag id='{vlan_id}'/></vlan></network>"
        )
    } else {
        format!(
            "<network><name>{safe_name}</name><forward mode='bridge'/><bridge name='{safe_bridge}'/></network>"
        )
    };
    // Use an unpredictable filename and open with `create_new` (fails if the
    // path already exists) so another local user on this hypervisor host can't
    // pre-place a symlink at a guessable path (the network name is only
    // lightly charset-sanitized, so a fixed name-derived path is predictable)
    // and have this root-running agent follow it to clobber an arbitrary file.
    // `uuid` is already a direct dependency of this crate; no new crate added.
    let tmp = std::env::temp_dir().join(format!(
        "machina-net-{file_safe_name}-{}.xml",
        uuid::Uuid::new_v4()
    ));
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(xml.as_bytes())?;
    }
    let define = Command::new("virsh")
        .args(["net-define", tmp.to_string_lossy().as_ref()])
        .output()?;
    let _ = std::fs::remove_file(&tmp);
    if !define.status.success() {
        anyhow::bail!(
            "virsh net-define failed: {}",
            String::from_utf8_lossy(&define.stderr)
        );
    }
    for sub in ["net-start", "net-autostart"] {
        let out = Command::new("virsh").args([sub, network_name]).output()?;
        if !out.status.success() {
            anyhow::bail!(
                "virsh {sub} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_nfs_source_splits_host_export() {
        let (h, e) = parse_nfs_source("10.0.0.5:/export/machina").unwrap();
        assert_eq!(h, "10.0.0.5");
        assert_eq!(e, "/export/machina");
    }

    #[test]
    fn parse_nfs_adds_leading_slash() {
        let (_, e) = parse_nfs_source("nas.local:export").unwrap();
        assert_eq!(e, "/export");
    }

    #[test]
    fn parse_ceph_pool_normalizes() {
        assert_eq!(parse_ceph_pool("vms"), "rbd/vms");
        assert_eq!(parse_ceph_pool("ceph:machina"), "machina");
    }

    #[test]
    fn parse_zfs_dataset_splits() {
        let (z, d) = parse_zfs_dataset("tank/machina").unwrap();
        assert_eq!(z, "tank");
        assert_eq!(d, "machina");
    }
}

/// Idempotent isolated subnet provisioning. A pre-existing network must have our
/// exact UUID before it may be started; never overwrite another operator's net.
pub fn provision_cloud_subnet(id: &str, cidr: &str) -> anyhow::Result<()> {
    provision_cloud_subnet_with(id, cidr, |args| Command::new("virsh").args(args).output())
}

fn provision_cloud_subnet_with(
    id: &str,
    cidr: &str,
    mut run: impl FnMut(&[&str]) -> std::io::Result<std::process::Output>,
) -> anyhow::Result<()> {
    let xml = machina_spec::cloud_network_xml(id, cidr).map_err(anyhow::Error::msg)?;
    let name = format!("mc-{id}");
    let prior = run(&["net-uuid", &name])?;
    if prior.status.success() {
        anyhow::ensure!(
            String::from_utf8_lossy(&prior.stdout).trim() == id,
            "cloud network ownership conflict"
        );
        // Ours by UUID is not the same as still configured as asked: a retry must not report success for a network
        // whose subnet was changed or which was given a forwarding mode (NAT/route would break the isolation promise).
        let dump = run(&["net-dumpxml", &name])?;
        anyhow::ensure!(
            dump.status.success(),
            "could not read the existing network's configuration"
        );
        if let Some(why) = network_drift(cidr, &String::from_utf8_lossy(&dump.stdout)) {
            anyhow::bail!("cloud network {name} has drifted from its subnet: {why}");
        }
    } else {
        // Only create on an authoritative missing-network result. Permission or
        // connection errors are not evidence of absence.
        let list = run(&["net-list", "--all", "--name"])?;
        anyhow::ensure!(
            list.status.success(),
            "could not enumerate libvirt networks"
        );
        anyhow::ensure!(
            !String::from_utf8_lossy(&list.stdout)
                .lines()
                .any(|n| n.trim() == name),
            "existing network UUID could not be checked"
        );
        let path = std::env::temp_dir().join(format!("machina-cloud-{}.xml", uuid::Uuid::new_v4()));
        let result = (|| -> anyhow::Result<()> {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            file.write_all(xml.as_bytes())?;
            let define = run(&["net-define", path.to_string_lossy().as_ref()])?;
            anyhow::ensure!(
                define.status.success(),
                "net-define: {}",
                String::from_utf8_lossy(&define.stderr)
            );
            Ok(())
        })();
        let _ = std::fs::remove_file(path);
        result?;
    }
    let active = run(&["net-list", "--name"])?;
    anyhow::ensure!(active.status.success(), "could not inspect active networks");
    if !String::from_utf8_lossy(&active.stdout)
        .lines()
        .any(|n| n.trim() == name)
    {
        let start = run(&["net-start", &name])?;
        anyhow::ensure!(
            start.status.success(),
            "net-start: {}",
            String::from_utf8_lossy(&start.stderr)
        );
    }
    let auto = run(&["net-autostart", &name])?;
    anyhow::ensure!(
        auto.status.success(),
        "net-autostart: {}",
        String::from_utf8_lossy(&auto.stderr)
    );
    Ok(())
}

/// Prefix length from a libvirt `<ip>` element: IPv4 networks are dumped with `netmask`, not the `prefix` they were defined with.
fn dumped_prefix(xml: &str) -> Option<u8> {
    if let Some(p) = machina_core::xml::extract_attr(xml, "ip", "prefix") {
        return p.parse().ok();
    }
    let mask: std::net::Ipv4Addr = machina_core::xml::extract_attr(xml, "ip", "netmask")?.parse().ok()?;
    let bits = u32::from(mask);
    // A valid netmask is a run of ones then zeros.
    (bits.leading_ones() + bits.trailing_zeros() == 32).then(|| bits.leading_ones() as u8)
}

/// Why an existing network no longer matches the subnet it should be, if it does not: a different gateway or prefix,
/// or any `<forward>` element (isolated networks have none).
fn network_drift(cidr: &str, dumpxml: &str) -> Option<String> {
    let want: machina_spec::CloudCidr = match cidr.parse() {
        Ok(c) => c,
        Err(e) => return Some(format!("expected CIDR {cidr} is invalid: {e}")),
    };
    if dumpxml.contains("<forward") {
        return Some("it has a <forward> element (it is no longer isolated)".into());
    }
    let gateway = want.address(1).ok()?;
    let found_gw = machina_core::xml::extract_attr(dumpxml, "ip", "address").unwrap_or_default();
    let found_prefix = dumped_prefix(dumpxml);
    if found_gw != gateway || found_prefix != Some(want.prefix) {
        return Some(format!(
            "expected gateway {gateway}/{}, found {}/{}",
            want.prefix,
            if found_gw.is_empty() { "none" } else { &found_gw },
            found_prefix.map(|p| p.to_string()).unwrap_or_else(|| "?".into())
        ));
    }
    None
}

#[cfg(all(test, unix))]
mod cloud_subnet_tests {
    use super::*;
    use std::{
        os::unix::process::ExitStatusExt,
        process::{ExitStatus, Output},
    };
    const ID: &str = "12345678-1234-1234-1234-123456789abc";
    const NAME: &str = "mc-12345678-1234-1234-1234-123456789abc";
    const CIDR: &str = "10.20.1.0/24";
    fn result(ok: bool, text: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(if ok { 0 } else { 256 }),
            stdout: text.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }
    /// What `virsh net-dumpxml` prints for a healthy network: libvirt reports IPv4 with a netmask.
    fn healthy_dump() -> String {
        "<network><name>x</name><bridge name='mc123456781234'/><mac address='52:54:00:00:00:01'/>\
         <ip address='10.20.1.1' netmask='255.255.255.0'><dhcp><range start='10.20.1.128' end='10.20.1.254'/></dhcp></ip></network>"
            .to_string()
    }
    /// A fake virsh where the network exists, is ours, looks healthy, and (unless `active` is false) is running.
    fn existing(active: bool, calls: &mut Vec<String>, args: &[&str]) -> std::io::Result<Output> {
        calls.push(args[0].to_string());
        Ok(result(
            true,
            match args[0] {
                "net-uuid" => ID,
                "net-dumpxml" => return Ok(result(true, &healthy_dump())),
                "net-list" if active => NAME,
                _ => "",
            },
        ))
    }

    #[test]
    fn refuses_foreign_network_and_connection_failure() {
        let mut calls = 0;
        let error = provision_cloud_subnet_with(ID, CIDR, |_| {
            calls += 1;
            Ok(result(true, "another-uuid"))
        })
        .unwrap_err();
        assert!(error.to_string().contains("ownership"));
        assert_eq!(calls, 1);
        let mut calls = Vec::new();
        assert!(provision_cloud_subnet_with(ID, CIDR, |args| {
            calls.push(args[0].to_string());
            Ok(result(false, ""))
        })
        .is_err());
        assert_eq!(calls, vec!["net-uuid", "net-list"]);
    }

    #[test]
    fn retry_of_active_owned_network_checks_it_but_does_not_redefine_or_restart() {
        let mut calls = Vec::new();
        provision_cloud_subnet_with(ID, CIDR, |args| existing(true, &mut calls, args)).unwrap();
        assert_eq!(calls, vec!["net-uuid", "net-dumpxml", "net-list", "net-autostart"]);
    }

    #[test]
    fn retry_of_a_defined_but_inactive_network_starts_it() {
        let mut calls = Vec::new();
        provision_cloud_subnet_with(ID, CIDR, |args| existing(false, &mut calls, args)).unwrap();
        assert_eq!(calls, vec!["net-uuid", "net-dumpxml", "net-list", "net-start", "net-autostart"]);
    }

    #[test]
    fn an_owned_network_that_drifted_is_an_error_not_a_success() {
        // different subnet
        let error = provision_cloud_subnet_with(ID, "10.30.0.0/24", |args| existing(true, &mut Vec::new(), args)).unwrap_err();
        assert!(error.to_string().contains("drifted"), "{error}");
        // a forwarding mode was added
        let error = provision_cloud_subnet_with(ID, CIDR, |args| {
            Ok(match args[0] {
                "net-uuid" => result(true, ID),
                "net-dumpxml" => result(true, &healthy_dump().replace("<ip ", "<forward mode='nat'/><ip ")),
                _ => result(true, ""),
            })
        })
        .unwrap_err();
        assert!(error.to_string().contains("<forward>"), "{error}");
        // the configuration cannot be read
        assert!(provision_cloud_subnet_with(ID, CIDR, |args| {
            Ok(match args[0] {
                "net-uuid" => result(true, ID),
                "net-dumpxml" => result(false, ""),
                _ => result(true, ""),
            })
        })
        .is_err());
    }

    #[test]
    fn drift_check_understands_netmask_and_prefix_forms() {
        assert_eq!(network_drift(CIDR, &healthy_dump()), None);
        assert_eq!(network_drift(CIDR, "<network><ip address='10.20.1.1' prefix='24'/></network>"), None);
        assert!(network_drift(CIDR, "<network><ip address='10.20.1.1' netmask='255.255.0.0'/></network>").is_some());
        assert!(network_drift(CIDR, "<network><ip address='10.20.1.9' netmask='255.255.255.0'/></network>").is_some());
        // a non-contiguous netmask is not a prefix
        assert!(network_drift(CIDR, "<network><ip address='10.20.1.1' netmask='255.0.255.0'/></network>").is_some());
        assert!(network_drift(CIDR, "<network></network>").is_some());
    }

    #[test]
    fn new_network_defines_validated_xml_and_cleans_temporary_file() {
        let mut path = String::new();
        let mut calls = Vec::new();
        provision_cloud_subnet_with(ID, CIDR, |args| {
            calls.push(args[0].to_string());
            if args[0] == "net-define" {
                path = args[1].into();
                let xml = std::fs::read_to_string(&path)?;
                // The defined document is exactly what the spec crate generates for this subnet, and it is isolated.
                assert_eq!(xml, machina_spec::cloud_network_xml(ID, CIDR).unwrap());
                assert!(!xml.contains("<forward"));
                assert!(xml.contains("<ip address='10.20.1.1' prefix='24'>"));
            }
            Ok(result(args[0] != "net-uuid", ""))
        })
        .unwrap();
        assert_eq!(calls, vec!["net-uuid", "net-list", "net-define", "net-list", "net-start", "net-autostart"]);
        assert!(!std::path::Path::new(&path).exists());
    }

    #[test]
    fn failing_define_start_or_autostart_is_reported_and_leaves_no_temp_file() {
        for failing in ["net-define", "net-start", "net-autostart"] {
            let mut path = String::new();
            let error = provision_cloud_subnet_with(ID, CIDR, |args| {
                if args[0] == "net-define" {
                    path = args[1].into();
                }
                Ok(result(args[0] != "net-uuid" && args[0] != failing, ""))
            })
            .unwrap_err();
            assert!(error.to_string().contains(failing), "{failing}: {error}");
            if !path.is_empty() {
                assert!(!std::path::Path::new(&path).exists(), "{failing} left a temp file");
            }
        }
    }

    #[test]
    fn a_name_that_exists_but_whose_uuid_cannot_be_read_is_not_overwritten() {
        let mut defined = false;
        let error = provision_cloud_subnet_with(ID, CIDR, |args| {
            if args[0] == "net-define" {
                defined = true;
            }
            Ok(match args[0] {
                "net-uuid" => result(false, ""),
                "net-list" => result(true, NAME),
                _ => result(true, ""),
            })
        })
        .unwrap_err();
        assert!(error.to_string().contains("could not be checked"));
        assert!(!defined);
    }
}
