// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Restore points, rewind and forks on qcow2 backing chains.
//!
//! A restore point is an external disk-only snapshot taken without libvirt
//! metadata: the file the VM was writing becomes a read-only layer holding the
//! disk exactly as it was, and the VM carries on in a fresh overlay. Rewind and
//! fork stack a new overlay on a frozen layer, so neither copies data and the
//! source of a fork never stops.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use virt::connect::Connect;
use virt::domain::Domain;
use virt::domain_snapshot::DomainSnapshot;
use virt::network::Network;
use virt::sys;

use super::domain::lookup_domain;
use super::snapshot::{guest_fs_freeze, ThawGuard};
use crate::LibvirtError;

/// L2-only libvirt network (no forward, no DHCP, no host address) that memory
/// forks and isolated forks are attached to.
pub const ISOLATED_NETWORK: &str = "machina-fork";
const ISOLATED_BRIDGE: &str = "mfork0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layer {
    pub target: String,
    pub file: String,
}

#[derive(Debug, Clone, Default)]
pub struct ForkOptions {
    /// Freeze the source now when empty; otherwise fork from these layers.
    pub layers: Vec<Layer>,
    /// Carry the source's RAM across (implies `isolate`, keeps MAC addresses).
    pub memory: bool,
    /// Attach every NIC to [`ISOLATED_NETWORK`].
    pub isolate: bool,
    /// New cloud-init identity (instance-id, hostname, machine-id) when the
    /// source boots from a NoCloud seed.
    pub reseed: bool,
    /// Boot the fork once defined (memory forks always run).
    pub start: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ForkResult {
    pub uuid: String,
    pub frozen: Vec<Layer>,
    pub disks: Vec<Layer>,
    pub quiesced: bool,
    pub reseeded: bool,
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DiskKind {
    File(String),
    Skip,
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Disk {
    target: String,
    kind: DiskKind,
    seed: bool,
}

fn domain_disks(xml: &str) -> Vec<Disk> {
    crate::xml::split_blocks(xml, "disk")
        .into_iter()
        .filter_map(|b| {
            let target = crate::xml::extract_attr(&b, "target", "dev")?;
            let device =
                crate::xml::extract_attr(&b, "disk", "device").unwrap_or_else(|| "disk".into());
            let ty = crate::xml::extract_attr(&b, "disk", "type").unwrap_or_else(|| "file".into());
            let file = crate::xml::extract_attr(&b, "source", "file").unwrap_or_default();
            let shared = b.contains("<readonly/>") || b.contains("<shareable/>");
            let lower = file.to_ascii_lowercase();
            let seed = device == "cdrom"
                && (lower.contains("seed")
                    || lower.contains("cidata")
                    || lower.contains("cloud-init"));
            let kind = if device != "disk" || shared {
                DiskKind::Skip
            } else if ty == "file" && !file.is_empty() {
                DiskKind::File(file)
            } else if ty == "file" {
                DiskKind::Skip
            } else {
                DiskKind::Unsupported(ty)
            };
            Some(Disk { target, kind, seed })
        })
        .collect()
}

fn file_disks(xml: &str) -> Result<Vec<Layer>, LibvirtError> {
    let mut out = Vec::new();
    for d in domain_disks(xml) {
        match d.kind {
            DiskKind::File(file) => out.push(Layer {
                target: d.target,
                file,
            }),
            DiskKind::Unsupported(ty) => {
                return Err(LibvirtError::Invalid(format!(
                "disk '{}' is {ty}-backed; restore points and forks need file-backed qcow2 disks",
                d.target
            )))
            }
            DiskKind::Skip => {}
        }
    }
    if out.is_empty() {
        return Err(LibvirtError::Invalid("VM has no file-backed disks".into()));
    }
    Ok(out)
}

pub fn validate_label(label: &str) -> Result<(), LibvirtError> {
    let ok = !label.is_empty()
        && label.len() <= 48
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(LibvirtError::Invalid(format!(
            "invalid label '{label}' (use 1-48 of [A-Za-z0-9_-])"
        )))
    }
}

/// `{dir of file}/{owner}-{target}.{label}.qcow2`.
pub fn overlay_path(file: &str, owner: &str, target: &str, label: &str) -> String {
    let dir = Path::new(file)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/var/lib/libvirt/images".into());
    format!(
        "{}/{owner}-{target}.{label}.qcow2",
        dir.trim_end_matches('/')
    )
}

/// True for overlays this module created (`*.rp-*.qcow2`, `*.rw-*`, `*.fk-*`),
/// the only files it is willing to delete.
pub fn is_managed_overlay(path: &str) -> bool {
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.ends_with(".qcow2") && [".rp-", ".rw-", ".fk-"].iter().any(|p| name.contains(p))
}

fn snapshot_xml(label: &str, disks: &[Disk], overlays: &[Layer], memory: Option<&str>) -> String {
    let mut s = format!(
        "<domainsnapshot>\n  <name>{}</name>\n",
        crate::xml::escape(label)
    );
    match memory {
        Some(f) => s.push_str(&format!(
            "  <memory snapshot='external' file='{}'/>\n",
            crate::xml::escape(f)
        )),
        None => s.push_str("  <memory snapshot='no'/>\n"),
    }
    s.push_str("  <disks>\n");
    for d in disks {
        match overlays.iter().find(|o| o.target == d.target) {
            Some(o) => s.push_str(&format!(
                "    <disk name='{}' snapshot='external' type='file'>\n      <driver type='qcow2'/>\n      <source file='{}'/>\n    </disk>\n",
                crate::xml::escape(&d.target),
                crate::xml::escape(&o.file)
            )),
            None => s.push_str(&format!(
                "    <disk name='{}' snapshot='no'/>\n",
                crate::xml::escape(&d.target)
            )),
        }
    }
    s.push_str("  </disks>\n</domainsnapshot>\n");
    s
}

/// The snapshot XML that freezes every file disk of `vm` under `label`, the
/// layers it freezes and the overlays the VM continues in.
fn freeze_plan(
    vm: &str,
    label: &str,
    xml: &str,
    memory: Option<&str>,
) -> Result<(String, Vec<Layer>, Vec<Layer>), LibvirtError> {
    validate_label(label)?;
    let frozen = file_disks(xml)?;
    let overlays: Vec<Layer> = frozen
        .iter()
        .map(|l| Layer {
            target: l.target.clone(),
            file: overlay_path(&l.file, vm, &l.target, label),
        })
        .collect();
    for o in &overlays {
        if Path::new(&o.file).exists() {
            return Err(LibvirtError::Invalid(format!(
                "refusing to overwrite existing {}",
                o.file
            )));
        }
    }
    Ok((
        snapshot_xml(label, &domain_disks(xml), &overlays, memory),
        frozen,
        overlays,
    ))
}

/// Freeze `vm`'s disks as they are now. Returns the frozen layers (the
/// restore point) and whether the guest filesystems were quiesced.
pub fn create_restore_point(
    conn: &Connect,
    vm: &str,
    label: &str,
) -> Result<(Vec<Layer>, bool), LibvirtError> {
    let dom = lookup_domain(conn, vm)?;
    let active = dom.is_active().unwrap_or(false);
    let xml = dom
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("Failed to get XML"))?;
    let (snap, frozen, _) = freeze_plan(vm, label, &xml, None)?;
    let mut guard = ThawGuard {
        domain: &dom,
        armed: false,
    };
    if active {
        guard.armed = matches!(guest_fs_freeze(&dom), Ok(n) if n >= 0);
    }
    let quiesced = guard.armed;
    DomainSnapshot::create_xml(
        &dom,
        &snap,
        sys::VIR_DOMAIN_SNAPSHOT_CREATE_DISK_ONLY
            | sys::VIR_DOMAIN_SNAPSHOT_CREATE_ATOMIC
            | sys::VIR_DOMAIN_SNAPSHOT_CREATE_NO_METADATA,
    )
    .map_err(LibvirtError::map_op("Failed to freeze disks"))?;
    drop(guard);
    Ok((frozen, quiesced))
}

fn qemu_img(args: &[&str]) -> Result<String, LibvirtError> {
    let out = Command::new("qemu-img")
        .args(args)
        .output()
        .map_err(|e| LibvirtError::Operation(format!("qemu-img: {e}")))?;
    if !out.status.success() {
        return Err(LibvirtError::Operation(format!(
            "qemu-img {}: {}",
            args.first().copied().unwrap_or(""),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn image_info(file: &str) -> Result<(String, u64), LibvirtError> {
    let v: serde_json::Value =
        serde_json::from_str(&qemu_img(&["info", "-U", "--output=json", file])?)
            .map_err(|e| LibvirtError::Operation(format!("qemu-img info {file}: {e}")))?;
    let fmt = v["format"].as_str().unwrap_or("qcow2").to_string();
    let size = v["virtual-size"].as_u64().unwrap_or(0);
    Ok((fmt, size))
}

/// Files in the backing chain of `file`, top first.
pub fn backing_chain(file: &str) -> Result<Vec<String>, LibvirtError> {
    let v: serde_json::Value = serde_json::from_str(&qemu_img(&[
        "info",
        "-U",
        "--backing-chain",
        "--output=json",
        file,
    ])?)
    .map_err(|e| LibvirtError::Operation(format!("qemu-img info {file}: {e}")))?;
    Ok(v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|i| i["filename"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

fn create_overlay(backing: &str, dest: &str) -> Result<(), LibvirtError> {
    if Path::new(dest).exists() {
        return Err(LibvirtError::Invalid(format!(
            "refusing to overwrite existing {dest}"
        )));
    }
    if !Path::new(backing).is_file() {
        return Err(LibvirtError::NotFound(format!(
            "layer {backing} is missing"
        )));
    }
    let (fmt, size) = image_info(backing)?;
    qemu_img(&[
        "create",
        "-q",
        "-f",
        "qcow2",
        "-F",
        &fmt,
        "-b",
        backing,
        "-u",
        dest,
        &size.to_string(),
    ])?;
    Ok(())
}

fn create_overlays(layers: &[Layer], owner: &str, label: &str) -> Result<Vec<Layer>, LibvirtError> {
    let mut made: Vec<Layer> = Vec::new();
    for l in layers {
        let dest = overlay_path(&l.file, owner, &l.target, label);
        if let Err(e) = create_overlay(&l.file, &dest) {
            for m in &made {
                let _ = std::fs::remove_file(&m.file);
            }
            return Err(e);
        }
        made.push(Layer {
            target: l.target.clone(),
            file: dest,
        });
    }
    Ok(made)
}

/// Removes every `<backingStore>` element (nested or self-closing).
fn strip_backing_store(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(pos) = rest.find("<backingStore") {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let Some(gt) = tail.find('>') else {
            rest = "";
            break;
        };
        if tail[..gt].ends_with('/') {
            rest = &tail[gt + 1..];
            continue;
        }
        let mut depth = 0usize;
        let mut i = 0usize;
        let mut end = tail.len();
        while i < tail.len() {
            let t = &tail[i..];
            if t.starts_with("</backingStore>") {
                depth -= 1;
                i += "</backingStore>".len();
                if depth == 0 {
                    end = i;
                    break;
                }
            } else if t.starts_with("<backingStore") {
                let g = t.find('>').map(|g| g + 1).unwrap_or(t.len());
                if !t[..g].ends_with("/>") {
                    depth += 1;
                }
                i += g;
            } else {
                i += t.chars().next().map(char::len_utf8).unwrap_or(1);
            }
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    let mut cleaned = String::with_capacity(out.len());
    for line in out.lines() {
        if !line.trim().is_empty() {
            cleaned.push_str(line);
            cleaned.push('\n');
        }
    }
    cleaned
}

/// Points the `<source file>` of each disk named in `map` (by target) at its new file.
fn repoint_targets(xml: &str, map: &[Layer]) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(pos) = rest.find("<disk ") {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let end = tail
            .find("</disk>")
            .map(|e| e + "</disk>".len())
            .unwrap_or(tail.len());
        let block = &tail[..end];
        let target = crate::xml::extract_attr(block, "target", "dev").unwrap_or_default();
        match (
            map.iter().find(|l| l.target == target),
            crate::xml::extract_attr(block, "source", "file"),
        ) {
            (Some(l), Some(old)) => {
                let from_sq = format!("file='{}'", crate::xml::escape(&old));
                let from_raw = format!("file='{old}'");
                let from_dq = format!("file=\"{old}\"");
                let to = format!("file='{}'", crate::xml::escape(&l.file));
                let replaced = if block.contains(&from_sq) {
                    block.replacen(&from_sq, &to, 1)
                } else if block.contains(&from_raw) {
                    block.replacen(&from_raw, &to, 1)
                } else {
                    block.replacen(&from_dq, &to, 1)
                };
                out.push_str(&replaced);
            }
            _ => out.push_str(block),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

fn replace_seed(xml: &str, iso: &str) -> (String, bool) {
    let seed = domain_disks(xml)
        .into_iter()
        .find(|d| d.seed)
        .map(|d| d.target);
    match seed {
        Some(target) => {
            let mut out = String::with_capacity(xml.len());
            let mut rest = xml;
            while let Some(pos) = rest.find("<disk ") {
                out.push_str(&rest[..pos]);
                let tail = &rest[pos..];
                let end = tail
                    .find("</disk>")
                    .map(|e| e + "</disk>".len())
                    .unwrap_or(tail.len());
                let block = &tail[..end];
                if crate::xml::extract_attr(block, "target", "dev").as_deref() == Some(&target) {
                    let old = crate::xml::extract_attr(block, "source", "file").unwrap_or_default();
                    out.push_str(&block.replacen(
                        &format!("file='{old}'"),
                        &format!("file='{}'", crate::xml::escape(iso)),
                        1,
                    ));
                } else {
                    out.push_str(block);
                }
                rest = &tail[end..];
            }
            out.push_str(rest);
            (out, true)
        }
        None => (xml.to_string(), false),
    }
}

fn isolate_interfaces(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(pos) = rest.find("<interface ") {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let end = tail
            .find("</interface>")
            .map(|e| e + "</interface>".len())
            .unwrap_or(tail.len());
        let block = &tail[..end];
        let mut lines = Vec::new();
        for (i, line) in block.lines().enumerate() {
            let t = line.trim_start();
            let indent = &line[..line.len() - t.len()];
            if i == 0 {
                lines.push(format!("{indent}<interface type='network'>"));
            } else if t.starts_with("<source ") {
                lines.push(format!("{indent}<source network='{ISOLATED_NETWORK}'/>"));
            } else if t.starts_with("<target ") || t.starts_with("<virtualport") {
                continue;
            } else {
                lines.push(line.to_string());
            }
        }
        out.push_str(&lines.join("\n"));
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

fn replace_name(xml: &str, new_name: &str) -> Result<String, LibvirtError> {
    let start = xml
        .find("<name>")
        .ok_or_else(|| LibvirtError::Operation("domain XML has no <name>".into()))?;
    let end = xml
        .find("</name>")
        .ok_or_else(|| LibvirtError::Operation("domain XML has no </name>".into()))?;
    Ok(format!(
        "{}<name>{}</name>{}",
        &xml[..start],
        crate::xml::escape(new_name),
        &xml[end + "</name>".len()..]
    ))
}

fn remove_uuid(xml: &str) -> String {
    match (xml.find("<uuid>"), xml.find("</uuid>")) {
        (Some(s), Some(e)) if e > s => {
            let after = &xml[e + "</uuid>".len()..];
            format!("{}{}", &xml[..s], after.trim_start_matches([' ', '\n']))
        }
        _ => xml.to_string(),
    }
}

fn random_u64() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut h = RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    h.finish()
}

fn fresh_mac() -> String {
    let b = random_u64().to_le_bytes();
    format!("52:54:00:{:02x}:{:02x}:{:02x}", b[0], b[1], b[2])
}

fn randomize_macs(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(pos) = rest.find("<mac address=") {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let end = tail.find("/>").map(|e| e + 2).unwrap_or(tail.len());
        out.push_str(&format!("<mac address='{}'/>", fresh_mac()));
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// The fork's domain XML: renamed, new UUID, disks on `disks`, and optionally
/// fresh MACs, isolated NICs and a replacement cloud-init seed.
fn fork_xml(
    source_xml: &str,
    new_name: &str,
    disks: &[Layer],
    keep_macs: bool,
    isolate: bool,
    seed_iso: Option<&str>,
    keep_uuid: bool,
) -> Result<(String, bool), LibvirtError> {
    let mut xml = replace_name(source_xml, new_name)?;
    if !keep_uuid {
        xml = remove_uuid(&xml);
    }
    if !keep_macs {
        xml = randomize_macs(&xml);
    }
    xml = strip_backing_store(&repoint_targets(&xml, disks));
    if isolate {
        xml = isolate_interfaces(&xml);
    }
    let mut reseeded = false;
    if let Some(iso) = seed_iso {
        let (x, r) = replace_seed(&xml, iso);
        xml = x;
        reseeded = r;
    }
    Ok((xml, reseeded))
}

pub fn ensure_isolated_network(conn: &Connect) -> Result<(), LibvirtError> {
    let net = match Network::lookup_by_name(conn, ISOLATED_NETWORK) {
        Ok(n) => n,
        Err(_) => {
            let xml = format!(
                "<network>\n  <name>{ISOLATED_NETWORK}</name>\n  <bridge name='{ISOLATED_BRIDGE}' stp='on' delay='0'/>\n</network>\n"
            );
            Network::define_xml(conn, &xml)
                .map_err(LibvirtError::map_op("Failed to define isolated network"))?
        }
    };
    if !net.is_active().unwrap_or(false) {
        net.create()
            .map_err(LibvirtError::map_op("Failed to start isolated network"))?;
    }
    let _ = net.set_autostart(true);
    Ok(())
}

fn yaml_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// A NoCloud seed giving the fork its own instance-id, hostname and (once)
/// a fresh machine-id.
fn write_seed(dir: &str, new_name: &str) -> Result<String, LibvirtError> {
    let tmp = std::env::temp_dir().join(format!("machina-fork-seed-{:016x}", random_u64()));
    std::fs::create_dir_all(&tmp).map_err(|e| LibvirtError::Operation(format!("seed dir: {e}")))?;
    let meta = super::cloud_init::nocloud_meta_data(&format!("{new_name}-{:016x}", random_u64()), new_name);
    let user = format!(
        "#cloud-config\nhostname: {}\npreserve_hostname: false\nbootcmd:\n  - [cloud-init-per, instance, machina-fork-id, sh, -c, \"rm -f /etc/machine-id /var/lib/dbus/machine-id && systemd-machine-id-setup\"]\n",
        yaml_quote(new_name)
    );
    // The first DHCP request happens before bootcmd replaces the machine-id, and
    // a machine-id derived DUID would claim the source's lease.
    let network = "version: 2\nethernets:\n  forked:\n    match:\n      name: \"e*\"\n    dhcp4: true\n    dhcp-identifier: mac\n";
    let iso = format!("{}/{new_name}-seed.fk.iso", dir.trim_end_matches('/'));
    let res = (|| {
        std::fs::write(tmp.join("meta-data"), meta)
            .map_err(|e| LibvirtError::Operation(format!("meta-data: {e}")))?;
        std::fs::write(tmp.join("user-data"), user)
            .map_err(|e| LibvirtError::Operation(format!("user-data: {e}")))?;
        std::fs::write(tmp.join("network-config"), network)
            .map_err(|e| LibvirtError::Operation(format!("network-config: {e}")))?;
        if Path::new(&iso).exists() {
            return Err(LibvirtError::Invalid(format!(
                "refusing to overwrite existing {iso}"
            )));
        }
        let tmp_s = tmp.to_string_lossy().into_owned();
        for tool in ["genisoimage", "mkisofs"] {
            if let Ok(o) = Command::new(tool)
                .args([
                    "-quiet", "-output", &iso, "-V", "cidata", "-r", "-J", &tmp_s,
                ])
                .output()
            {
                if o.status.success() {
                    return Ok(iso.clone());
                }
            }
        }
        Err(LibvirtError::Operation(
            "cannot build cloud-init seed: install genisoimage".into(),
        ))
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    res
}

/// Fork `source` into a new domain `new_name` backed by frozen layers.
pub fn fork(
    conn: &Connect,
    source: &str,
    new_name: &str,
    label: &str,
    opts: &ForkOptions,
) -> Result<ForkResult, LibvirtError> {
    crate::validate::validate_name(new_name)?;
    validate_label(label)?;
    if Domain::lookup_by_name(conn, new_name).is_ok() {
        return Err(LibvirtError::Invalid(format!(
            "VM '{new_name}' already exists"
        )));
    }
    if opts.memory {
        return memory_fork(conn, source, new_name, label);
    }
    let dom = lookup_domain(conn, source)?;
    let (frozen, quiesced) = if opts.layers.is_empty() {
        create_restore_point(conn, source, label)?
    } else {
        (opts.layers.clone(), false)
    };
    let source_xml = dom
        .get_xml_desc(sys::VIR_DOMAIN_XML_INACTIVE)
        .map_err(LibvirtError::map_op("Failed to get XML"))?;
    let current = file_disks(&source_xml)?;
    for l in &frozen {
        if !current.iter().any(|c| c.target == l.target) {
            return Err(LibvirtError::Invalid(format!(
                "VM '{source}' has no disk '{}' any more",
                l.target
            )));
        }
    }
    let disks = create_overlays(&frozen, new_name, label)?;
    let cleanup = |iso: &Option<String>| {
        for d in &disks {
            let _ = std::fs::remove_file(&d.file);
        }
        if let Some(i) = iso {
            let _ = std::fs::remove_file(i);
        }
    };
    let has_seed = domain_disks(&source_xml).iter().any(|d| d.seed);
    let seed_iso = if opts.reseed && has_seed {
        let dir = Path::new(&disks[0].file)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/var/lib/libvirt/images".into());
        match write_seed(&dir, new_name) {
            Ok(i) => Some(i),
            Err(e) => {
                cleanup(&None);
                return Err(e);
            }
        }
    } else {
        None
    };
    if opts.isolate {
        if let Err(e) = ensure_isolated_network(conn) {
            cleanup(&seed_iso);
            return Err(e);
        }
    }
    let (xml, reseeded) = match fork_xml(
        &source_xml,
        new_name,
        &disks,
        false,
        opts.isolate,
        seed_iso.as_deref(),
        false,
    ) {
        Ok(x) => x,
        Err(e) => {
            cleanup(&seed_iso);
            return Err(e);
        }
    };
    let xml = super::graphics_convert::ensure_graphics_present(&xml, "127.0.0.1");
    let new_dom = match Domain::define_xml(conn, &xml) {
        Ok(d) => d,
        Err(e) => {
            cleanup(&seed_iso);
            return Err(LibvirtError::Operation(format!("define fork: {e}")));
        }
    };
    let running = opts.start
        && match new_dom.create() {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!("fork '{new_name}' defined but did not start: {e}");
                false
            }
        };
    Ok(ForkResult {
        uuid: new_dom
            .get_uuid_string()
            .map_err(LibvirtError::map_op("Failed to read fork UUID"))?,
        frozen,
        disks,
        quiesced,
        reseeded,
        running,
    })
}

const SAVE_MAGIC: &[u8; 16] = b"LibvirtQemudSave";
/// magic[16], version, data_len, was_running, compressed, cookie_offset, unused[14].
const SAVE_HEADER_LEN: usize = 92;

/// Rewrites the domain XML embedded in a libvirt QEMU save image in place.
/// libvirt's own restore-time XML override refuses a different UUID, which a
/// fork running beside its source needs. The data area is padded, so the
/// XML and the cookie that follows it are rewritten within `data_len`.
fn rewrite_save_image_xml(
    path: &str,
    f: impl FnOnce(&str) -> Result<String, LibvirtError>,
) -> Result<(), LibvirtError> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let io = |e: std::io::Error| LibvirtError::Operation(format!("save image {path}: {e}"));
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(io)?;
    let mut header = [0u8; SAVE_HEADER_LEN];
    file.read_exact(&mut header).map_err(io)?;
    let (data_len, cookie_off) = parse_save_header(&header)?;
    let mut data = vec![0u8; data_len];
    file.read_exact(&mut data).map_err(io)?;
    let (buf, new_cookie_off) = rewrite_save_data(&data, cookie_off, f)?;
    file.seek(SeekFrom::Start(32)).map_err(io)?;
    file.write_all(&new_cookie_off.to_le_bytes()).map_err(io)?;
    file.seek(SeekFrom::Start(SAVE_HEADER_LEN as u64))
        .map_err(io)?;
    file.write_all(&buf).map_err(io)?;
    file.sync_all().map_err(io)?;
    Ok(())
}

fn parse_save_header(h: &[u8; SAVE_HEADER_LEN]) -> Result<(usize, usize), LibvirtError> {
    if &h[..16] != SAVE_MAGIC {
        return Err(LibvirtError::Operation(
            "not a complete libvirt QEMU save image".into(),
        ));
    }
    let word = |i: usize| u32::from_le_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]) as usize;
    let (version, data_len, cookie_off) = (word(16), word(20), word(32));
    if !(2..=3).contains(&version) || data_len == 0 || data_len > 16 << 20 {
        return Err(LibvirtError::Operation(format!(
            "unsupported save image (version {version}, data {data_len})"
        )));
    }
    Ok((data_len, cookie_off))
}

/// The rewritten data area (same length) and the new cookie offset.
fn rewrite_save_data(
    data: &[u8],
    cookie_off: usize,
    f: impl FnOnce(&str) -> Result<String, LibvirtError>,
) -> Result<(Vec<u8>, u32), LibvirtError> {
    let bad = || LibvirtError::Operation("malformed save image data".into());
    let xml_end = data.iter().position(|b| *b == 0).ok_or_else(bad)?;
    let xml = std::str::from_utf8(&data[..xml_end]).map_err(|_| bad())?;
    let cookie: &[u8] = if cookie_off > 0 {
        let rest = data.get(cookie_off..).ok_or_else(bad)?;
        &rest[..rest.iter().position(|b| *b == 0).ok_or_else(bad)?]
    } else {
        &[]
    };
    let mut buf = f(xml)?.into_bytes();
    buf.push(0);
    let new_cookie_off = if cookie_off > 0 { buf.len() as u32 } else { 0 };
    if cookie_off > 0 {
        buf.extend_from_slice(cookie);
        buf.push(0);
    }
    if buf.len() > data.len() {
        return Err(LibvirtError::Operation(
            "rewritten domain XML does not fit the save image".into(),
        ));
    }
    buf.resize(data.len(), 0);
    Ok((buf, new_cookie_off))
}

fn memory_fork(
    conn: &Connect,
    source: &str,
    new_name: &str,
    label: &str,
) -> Result<ForkResult, LibvirtError> {
    let dom = lookup_domain(conn, source)?;
    if !dom.is_active().unwrap_or(false) {
        return Err(LibvirtError::Invalid(format!(
            "VM '{source}' is not running; a memory fork needs live RAM"
        )));
    }
    let xml = dom
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("Failed to get XML"))?;
    let first = file_disks(&xml)?;
    let dir = Path::new(&first[0].file)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/var/lib/libvirt/images".into());
    let mem = format!("{}/{source}.{label}.mem", dir.trim_end_matches('/'));
    if Path::new(&mem).exists() {
        return Err(LibvirtError::Invalid(format!(
            "refusing to overwrite {mem}"
        )));
    }
    ensure_isolated_network(conn)?;
    let (snap, frozen, _) = freeze_plan(source, label, &xml, Some(&mem))?;
    DomainSnapshot::create_xml(
        &dom,
        &snap,
        sys::VIR_DOMAIN_SNAPSHOT_CREATE_ATOMIC
            | sys::VIR_DOMAIN_SNAPSHOT_CREATE_NO_METADATA
            | sys::VIR_DOMAIN_SNAPSHOT_CREATE_LIVE,
    )
    .map_err(LibvirtError::map_op("Failed to snapshot memory"))?;
    let result = (|| {
        let disks = create_overlays(&frozen, new_name, label)?;
        let rewritten = rewrite_save_image_xml(&mem, |saved| {
            fork_xml(saved, new_name, &disks, true, true, None, false).map(|(x, _)| x)
        });
        if let Err(e) = rewritten.and_then(|_| {
            Domain::domain_restore_flags(conn, &mem, None, sys::VIR_DOMAIN_SAVE_RUNNING)
                .map_err(|e| LibvirtError::Operation(e.to_string()))
        }) {
            for d in &disks {
                let _ = std::fs::remove_file(&d.file);
            }
            return Err(LibvirtError::Operation(format!("restore memory fork: {e}")));
        }
        let fork = lookup_domain(conn, new_name)?;
        let live = fork
            .get_xml_desc(sys::VIR_DOMAIN_XML_INACTIVE)
            .map_err(LibvirtError::map_op("Failed to get fork XML"))?;
        Domain::define_xml(conn, &strip_backing_store(&live))
            .map_err(LibvirtError::map_op("Failed to persist fork"))?;
        Ok(ForkResult {
            uuid: fork
                .get_uuid_string()
                .map_err(LibvirtError::map_op("Failed to read fork UUID"))?,
            frozen: frozen.clone(),
            disks,
            quiesced: true,
            reseeded: false,
            running: true,
        })
    })();
    let _ = std::fs::remove_file(&mem);
    result
}

/// Throw away everything `vm` wrote after `layers` were frozen: stops it,
/// boots it from fresh overlays on `layers`, and deletes the overlays it was
/// running on plus any managed `discard` files outside the new chain.
pub fn rewind(
    conn: &Connect,
    vm: &str,
    layers: &[Layer],
    label: &str,
    discard: &[String],
) -> Result<(Vec<Layer>, bool), LibvirtError> {
    validate_label(label)?;
    let dom = lookup_domain(conn, vm)?;
    let active = dom.is_active().unwrap_or(false);
    let inactive = dom
        .get_xml_desc(sys::VIR_DOMAIN_XML_INACTIVE)
        .map_err(LibvirtError::map_op("Failed to get XML"))?;
    let current = file_disks(&inactive)?;
    for l in layers {
        if !current.iter().any(|c| c.target == l.target) {
            return Err(LibvirtError::Invalid(format!(
                "VM '{vm}' has no disk '{}' any more",
                l.target
            )));
        }
    }
    let disks = create_overlays(layers, vm, label)?;
    if active {
        if let Err(e) = dom.destroy() {
            for d in &disks {
                let _ = std::fs::remove_file(&d.file);
            }
            return Err(LibvirtError::Operation(format!("stop for rewind: {e}")));
        }
    }
    if super::save_restore::has_managed_save(conn, vm).unwrap_or(false) {
        let _ = super::save_restore::managed_save_remove(conn, vm);
    }
    let xml = strip_backing_store(&repoint_targets(&inactive, &disks));
    if let Err(e) = Domain::define_xml(conn, &xml) {
        for d in &disks {
            let _ = std::fs::remove_file(&d.file);
        }
        if active {
            let _ = dom.create();
        }
        return Err(LibvirtError::Operation(format!("rewind define: {e}")));
    }
    let keep: Vec<String> = layers
        .iter()
        .map(|l| l.file.clone())
        .chain(disks.iter().map(|d| d.file.clone()))
        .collect();
    for old in current
        .iter()
        .filter(|c| layers.iter().any(|l| l.target == c.target))
        .map(|c| c.file.clone())
        .chain(discard.iter().cloned())
    {
        if is_managed_overlay(&old) && !keep.contains(&old) {
            let _ = std::fs::remove_file(&old);
        }
    }
    if active {
        dom.create()
            .map_err(LibvirtError::map_op("Failed to start after rewind"))?;
    }
    Ok((disks, active))
}

/// Merge each `top` layer into its direct backing `base` (dropping the
/// restore point that `base` held) and delete `top`.
pub fn merge(conn: &Connect, vm: &str, pairs: &[(Layer, Layer)]) -> Result<(), LibvirtError> {
    let dom = lookup_domain(conn, vm)?;
    let active = dom.is_active().unwrap_or(false);
    let xml = dom
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("Failed to get XML"))?;
    let current = file_disks(&xml)?;
    for (top, base) in pairs {
        if top.target != base.target {
            return Err(LibvirtError::Invalid("merge pair targets differ".into()));
        }
        let head = current
            .iter()
            .find(|c| c.target == top.target)
            .ok_or_else(|| LibvirtError::Invalid(format!("no disk '{}'", top.target)))?;
        let chain = backing_chain(&head.file)?;
        let ti = chain.iter().position(|f| f == &top.file);
        let bi = chain.iter().position(|f| f == &base.file);
        match (ti, bi) {
            (Some(t), Some(b)) if b == t + 1 && t > 0 => {}
            _ => {
                return Err(LibvirtError::Invalid(format!(
                    "{} is not directly backed by {} below the active layer",
                    top.file, base.file
                )))
            }
        }
        if active {
            super::block_jobs::block_commit(
                conn,
                vm,
                &top.target,
                Some(&base.file),
                Some(&top.file),
                0,
                0,
            )?;
            let deadline = Instant::now() + Duration::from_secs(3600);
            while super::block_jobs::block_job_info(conn, vm, &top.target, 0)?.is_some() {
                if Instant::now() > deadline {
                    return Err(LibvirtError::Operation(format!(
                        "commit of {} timed out",
                        top.file
                    )));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        } else {
            let t = chain.iter().position(|f| f == &top.file).unwrap_or(1);
            let child = chain[t - 1].clone();
            let (fmt, _) = image_info(&base.file)?;
            qemu_img(&["commit", "-q", "-d", "-b", &base.file, &top.file])?;
            qemu_img(&["rebase", "-u", "-F", &fmt, "-b", &base.file, &child])?;
        }
        if backing_chain(&head.file)?.contains(&top.file) {
            return Err(LibvirtError::Operation(format!(
                "{} is still in the chain after commit",
                top.file
            )));
        }
        if is_managed_overlay(&top.file) {
            let _ = std::fs::remove_file(&top.file);
        }
    }
    Ok(())
}

/// Copy every backing layer into the active overlay so the VM no longer
/// depends on another VM's frozen layers.
pub fn detach(conn: &Connect, vm: &str) -> Result<(), LibvirtError> {
    let dom = lookup_domain(conn, vm)?;
    let active = dom.is_active().unwrap_or(false);
    let xml = dom
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("Failed to get XML"))?;
    for d in file_disks(&xml)? {
        if backing_chain(&d.file)?.len() <= 1 {
            continue;
        }
        if active {
            super::block_jobs::block_pull(conn, vm, &d.target, 0, 0)?;
            let deadline = Instant::now() + Duration::from_secs(6 * 3600);
            while super::block_jobs::block_job_info(conn, vm, &d.target, 0)?.is_some() {
                if Instant::now() > deadline {
                    return Err(LibvirtError::Operation(format!(
                        "pull into {} timed out",
                        d.file
                    )));
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        } else {
            qemu_img(&["rebase", "-q", "-f", "qcow2", "-b", "", &d.file])?;
        }
        if backing_chain(&d.file)?.len() > 1 {
            return Err(LibvirtError::Operation(format!(
                "{} still has a backing file",
                d.file
            )));
        }
    }
    Ok(())
}

/// Delete managed overlay files that no domain disk chain still uses.
pub fn discard(conn: &Connect, files: &[String]) -> Result<usize, LibvirtError> {
    let mut used = std::collections::BTreeSet::new();
    let doms = conn
        .list_all_domains(0)
        .map_err(LibvirtError::map_op("Failed to list domains"))?;
    for d in doms {
        let xml = d.get_xml_desc(0).unwrap_or_default();
        for l in domain_disks(&xml) {
            if let DiskKind::File(f) = l.kind {
                for c in backing_chain(&f).unwrap_or_else(|_| vec![f.clone()]) {
                    used.insert(c);
                }
            }
        }
    }
    let mut n = 0;
    for f in files {
        if is_managed_overlay(f) && !used.contains(f) && std::fs::remove_file(f).is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIVE: &str = r#"<domain type='kvm' id='3'>
  <name>web</name>
  <uuid>8c1e4c1a-0000-4000-8000-000000000001</uuid>
  <devices>
    <disk type='file' device='disk'>
      <driver name='qemu' type='qcow2'/>
      <source file='/var/lib/libvirt/images/web.rp-a.qcow2' index='3'/>
      <backingStore type='file' index='2'>
        <format type='qcow2'/>
        <source file='/var/lib/libvirt/images/web.qcow2'/>
        <backingStore type='file' index='1'>
          <format type='qcow2'/>
          <source file='/var/lib/libvirt/images/base.qcow2'/>
          <backingStore/>
        </backingStore>
      </backingStore>
      <target dev='vda' bus='virtio'/>
    </disk>
    <disk type='file' device='cdrom'>
      <driver name='qemu' type='raw'/>
      <source file='/var/lib/libvirt/images/web-seed.iso'/>
      <target dev='sda' bus='sata'/>
      <readonly/>
    </disk>
    <interface type='network'>
      <mac address='52:54:00:aa:bb:cc'/>
      <source network='default' bridge='virbr0'/>
      <target dev='vnet3'/>
      <model type='virtio'/>
    </interface>
  </devices>
</domain>"#;

    #[test]
    fn freeze_plan_covers_file_disks_and_skips_the_seed() {
        let (snap, frozen, overlays) = freeze_plan("web", "rp-1", LIVE, None).unwrap();
        assert_eq!(
            frozen,
            vec![Layer {
                target: "vda".into(),
                file: "/var/lib/libvirt/images/web.rp-a.qcow2".into()
            }]
        );
        assert_eq!(
            overlays[0].file,
            "/var/lib/libvirt/images/web-vda.rp-1.qcow2"
        );
        assert!(snap.contains("<disk name='vda' snapshot='external' type='file'>"));
        assert!(snap.contains("<disk name='sda' snapshot='no'/>"));
        assert!(snap.contains("<memory snapshot='no'/>"));
    }

    #[test]
    fn network_disks_are_refused() {
        let xml = "<domain><devices><disk type='network' device='disk'><source protocol='rbd' name='p/i'/><target dev='vdb'/></disk></devices></domain>";
        assert!(freeze_plan("web", "rp-1", xml, None).is_err());
    }

    #[test]
    fn fork_xml_renames_repoints_and_isolates() {
        let disks = vec![Layer {
            target: "vda".into(),
            file: "/var/lib/libvirt/images/web2-vda.fk-1.qcow2".into(),
        }];
        let (xml, reseeded) = fork_xml(
            LIVE,
            "web2",
            &disks,
            false,
            true,
            Some("/var/lib/libvirt/images/web2-seed.fk.iso"),
            false,
        )
        .unwrap();
        assert!(xml.contains("<name>web2</name>"));
        assert!(!xml.contains("<uuid>"));
        assert!(!xml.contains("backingStore"));
        assert!(xml.contains("file='/var/lib/libvirt/images/web2-vda.fk-1.qcow2'"));
        assert!(!xml.contains("52:54:00:aa:bb:cc"));
        assert!(xml.contains("<source network='machina-fork'/>"));
        assert!(!xml.contains("vnet3"));
        assert!(reseeded);
        assert!(xml.contains("web2-seed.fk.iso"));
        assert!(!xml.contains("web-seed.iso"));
    }

    #[test]
    fn memory_forks_keep_their_macs() {
        let (xml, _) = fork_xml(LIVE, "web2", &[], true, true, None, false).unwrap();
        assert!(xml.contains("52:54:00:aa:bb:cc"));
    }

    #[test]
    fn only_managed_overlays_are_deletable() {
        assert!(is_managed_overlay("/i/web-vda.rp-20261005.qcow2"));
        assert!(is_managed_overlay("/i/web-vda.rw-1.qcow2"));
        assert!(is_managed_overlay("/i/web2-vda.fk-1.qcow2"));
        assert!(!is_managed_overlay("/i/web.qcow2"));
        assert!(!is_managed_overlay("/i/base.qcow2"));
        assert!(!is_managed_overlay("/i/web-vda.rp-1.raw"));
    }

    #[test]
    fn save_image_data_is_rewritten_in_place() {
        let mut data = b"<domain><name>a</name></domain>\0<cookie/>\0".to_vec();
        data.resize(128, 0);
        let (buf, off) = rewrite_save_data(&data, 32, |x| {
            Ok(x.replace("<name>a</name>", "<name>fork-b</name>"))
        })
        .unwrap();
        assert_eq!(buf.len(), 128);
        assert_eq!(off, 37);
        assert!(buf.starts_with(b"<domain><name>fork-b</name></domain>\0<cookie/>\0"));
        assert!(rewrite_save_data(&data, 32, |_| Ok("x".repeat(200))).is_err());
        let mut h = [0u8; SAVE_HEADER_LEN];
        h[..16].copy_from_slice(SAVE_MAGIC);
        h[16] = 2;
        h[20..24].copy_from_slice(&128u32.to_le_bytes());
        h[32] = 32;
        assert_eq!(parse_save_header(&h).unwrap(), (128, 32));
        h[0] = b'X';
        assert!(parse_save_header(&h).is_err());
    }

    #[test]
    fn labels_are_validated() {
        assert!(validate_label("rp-20261005T0400").is_ok());
        assert!(validate_label("").is_err());
        assert!(validate_label("../x").is_err());
    }
}
