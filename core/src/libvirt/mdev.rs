// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Mediated devices (vGPU partitions). A GPU whose driver supports mediated devices lists the
//! partition types it can carve out under `/sys/class/mdev_bus/<pci>/mdev_supported_types/`;
//! writing a UUID to a type's `create` file makes one partition, and it appears under
//! `/sys/bus/mdev/devices/<uuid>`. A VM then uses the partition as a `type='mdev'` hostdev.
//!
//! Every function takes the sysfs root so tests can run on a fake tree. Production callers pass
//! [`SYSFS_ROOT`]. Parent addresses, type ids and UUIDs are validated before they become part of
//! a path, so a request cannot walk out of the mdev directories.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use virt::connect::Connect;

use super::device::{get_domain_flags_pub, DetachOutcome, DETACH_LIVE_WAIT};
use super::domain::{lookup_domain, wait_until_absent_from_live};
use super::hostdev_pci::parse_pci_bdf;
use crate::LibvirtError;

pub const SYSFS_ROOT: &str = "/sys";

/// One partition type a GPU offers (for example an NVIDIA vGPU profile).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MdevType {
    /// PCI address of the GPU, `0000:03:00.0`.
    pub parent: String,
    /// Directory name under `mdev_supported_types`, for example `nvidia-222`.
    pub type_id: String,
    pub name: String,
    pub description: String,
    pub device_api: String,
    /// How many more partitions of this type the GPU can create right now.
    pub available_instances: u64,
}

/// A partition that exists on the host.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MdevInstance {
    pub uuid: String,
    pub parent: String,
    pub type_id: String,
}

fn bad(msg: impl Into<String>) -> LibvirtError {
    LibvirtError::Invalid(msg.into())
}

/// `0000:03:00.0` with the function in 0-7; returned lowercase.
pub fn validate_parent(parent: &str) -> Result<String, LibvirtError> {
    let (d, b, s, f) = parse_pci_bdf(parent)?;
    Ok(format!("{d}:{b}:{s}.{f}").to_ascii_lowercase())
}

/// Type ids are short driver-chosen names such as `nvidia-222` or `i915-GVTg_V5_4`.
pub fn validate_type_id(t: &str) -> Result<&str, LibvirtError> {
    let ok = !t.is_empty()
        && t.len() <= 64
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
        && !t.starts_with('.');
    if ok {
        Ok(t)
    } else {
        Err(bad(format!("invalid mdev type '{t}'")))
    }
}

pub fn validate_uuid(u: &str) -> Result<String, LibvirtError> {
    uuid::Uuid::parse_str(u.trim())
        .map(|u| u.hyphenated().to_string())
        .map_err(|_| bad(format!("invalid mdev uuid '{u}'")))
}

fn read_trim(p: &Path) -> String {
    fs::read_to_string(p)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Every partition type every mdev-capable GPU on the host offers.
pub fn list_types(root: &Path) -> Vec<MdevType> {
    let bus = root.join("class/mdev_bus");
    let mut out = Vec::new();
    let Ok(parents) = fs::read_dir(&bus) else {
        return out;
    };
    for parent in parents.flatten() {
        let parent_name = parent.file_name().to_string_lossy().to_string();
        let Ok(types) = fs::read_dir(parent.path().join("mdev_supported_types")) else {
            continue;
        };
        for t in types.flatten() {
            let dir = t.path();
            let type_id = t.file_name().to_string_lossy().to_string();
            out.push(MdevType {
                parent: parent_name.clone(),
                name: read_trim(&dir.join("name")),
                description: read_trim(&dir.join("description")),
                device_api: read_trim(&dir.join("device_api")),
                available_instances: read_trim(&dir.join("available_instances"))
                    .parse()
                    .unwrap_or(0),
                type_id,
            });
        }
    }
    out.sort_by(|a, b| (&a.parent, &a.type_id).cmp(&(&b.parent, &b.type_id)));
    out
}

/// Partitions that currently exist, with the GPU and type each came from.
pub fn list_instances(root: &Path) -> Vec<MdevInstance> {
    let mut out = Vec::new();
    let Ok(devs) = fs::read_dir(root.join("bus/mdev/devices")) else {
        return out;
    };
    for d in devs.flatten() {
        let uuid = d.file_name().to_string_lossy().to_string();
        // The entry is a symlink to <parent>/<uuid>; mdev_type points at the type directory.
        let parent = fs::canonicalize(d.path())
            .ok()
            .and_then(|p| {
                p.parent()
                    .and_then(|x| x.file_name())
                    .map(|n| n.to_string_lossy().to_string())
            })
            .unwrap_or_default();
        let type_id = fs::read_link(d.path().join("mdev_type"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_default();
        out.push(MdevInstance {
            uuid,
            parent,
            type_id,
        });
    }
    out.sort_by(|a, b| a.uuid.cmp(&b.uuid));
    out
}

fn type_dir(root: &Path, parent: &str, type_id: &str) -> PathBuf {
    root.join("class/mdev_bus")
        .join(parent)
        .join("mdev_supported_types")
        .join(type_id)
}

/// Create one partition of `type_id` on GPU `parent`. A UUID is generated when none is given.
/// Fails before writing when the GPU has no instances of that type left.
pub fn create(
    root: &Path,
    parent: &str,
    type_id: &str,
    uuid: Option<&str>,
) -> Result<String, LibvirtError> {
    let parent = validate_parent(parent)?;
    let type_id = validate_type_id(type_id)?;
    let uuid = match uuid {
        Some(u) => validate_uuid(u)?,
        None => uuid::Uuid::new_v4().hyphenated().to_string(),
    };
    let dir = type_dir(root, &parent, type_id);
    if !dir.is_dir() {
        return Err(LibvirtError::NotFound(format!(
            "GPU {parent} offers no mdev type '{type_id}'"
        )));
    }
    let avail: u64 = read_trim(&dir.join("available_instances"))
        .parse()
        .unwrap_or(0);
    if avail == 0 {
        return Err(LibvirtError::Operation(format!(
            "no {type_id} instances left on {parent}"
        )));
    }
    if root.join("bus/mdev/devices").join(&uuid).exists() {
        return Err(LibvirtError::Operation(format!(
            "mdev {uuid} already exists"
        )));
    }
    fs::write(dir.join("create"), &uuid)
        .map_err(|e| LibvirtError::Operation(format!("create mdev {type_id} on {parent}: {e}")))?;
    Ok(uuid)
}

/// Remove a partition. The kernel refuses while a VM still has it open.
pub fn remove(root: &Path, uuid: &str) -> Result<(), LibvirtError> {
    let uuid = validate_uuid(uuid)?;
    let dev = root.join("bus/mdev/devices").join(&uuid);
    if !dev.exists() {
        return Err(LibvirtError::NotFound(format!("mdev {uuid} not found")));
    }
    fs::write(dev.join("remove"), "1")
        .map_err(|e| LibvirtError::Operation(format!("remove mdev {uuid}: {e}")))
}

/// `managed='no'`: libvirt does not create or destroy mdevs, this module does.
pub fn mdev_hostdev_xml(uuid: &str) -> String {
    format!(
        "<hostdev mode='subsystem' type='mdev' model='vfio-pci' managed='no'>\n  <source>\n    <address uuid='{uuid}'/>\n  </source>\n</hostdev>"
    )
}

pub fn attach_mdev(conn: &Connect, vm_name: &str, uuid: &str) -> Result<(), LibvirtError> {
    let uuid = validate_uuid(uuid)?;
    let dom = lookup_domain(conn, vm_name)?;
    let flags = get_domain_flags_pub(&dom);
    dom.attach_device_flags(&mdev_hostdev_xml(&uuid), flags)
        .map_err(|e| {
            LibvirtError::Operation(format!("Failed to attach mdev {uuid} to '{vm_name}': {e}"))
        })?;
    Ok(())
}

pub fn detach_mdev(
    conn: &Connect,
    vm_name: &str,
    uuid: &str,
) -> Result<DetachOutcome, LibvirtError> {
    let uuid = validate_uuid(uuid)?;
    let dom = lookup_domain(conn, vm_name)?;
    let flags = get_domain_flags_pub(&dom);
    dom.detach_device_flags(&mdev_hostdev_xml(&uuid), flags)
        .map_err(|e| {
            LibvirtError::Operation(format!(
                "Failed to detach mdev {uuid} from '{vm_name}': {e}"
            ))
        })?;
    let needle = format!("uuid='{uuid}'");
    let live_removed = if dom.is_active().unwrap_or(false) {
        wait_until_absent_from_live(&dom, DETACH_LIVE_WAIT, |xml| !xml.contains(&needle))
    } else {
        true
    };
    Ok(DetachOutcome { live_removed })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GPU: &str = "0000:03:00.0";
    const UUID: &str = "11111111-2222-3333-4444-555555555555";

    /// A fake sysfs with one GPU offering two types.
    fn fake_sysfs(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("machina-mdev-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (t, name, avail) in [
            ("nvidia-222", "GRID A16-4C", "3"),
            ("nvidia-223", "GRID A16-8C", "0"),
        ] {
            let d = root
                .join("class/mdev_bus")
                .join(GPU)
                .join("mdev_supported_types")
                .join(t);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("name"), format!("{name}\n")).unwrap();
            fs::write(d.join("description"), "framebuffer 4096M\n").unwrap();
            fs::write(d.join("device_api"), "vfio-pci\n").unwrap();
            fs::write(d.join("available_instances"), format!("{avail}\n")).unwrap();
            fs::write(d.join("create"), "").unwrap();
        }
        fs::create_dir_all(root.join("bus/mdev/devices")).unwrap();
        root
    }

    #[test]
    fn lists_types_sorted_with_availability() {
        let root = fake_sysfs("types");
        let t = list_types(&root);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].type_id, "nvidia-222");
        assert_eq!(t[0].parent, GPU);
        assert_eq!(t[0].name, "GRID A16-4C");
        assert_eq!(t[0].available_instances, 3);
        assert_eq!(t[1].available_instances, 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_sysfs_is_an_empty_list_not_an_error() {
        let nowhere = std::env::temp_dir().join("machina-mdev-does-not-exist");
        assert!(list_types(&nowhere).is_empty());
        assert!(list_instances(&nowhere).is_empty());
    }

    #[test]
    fn create_writes_the_uuid_to_the_type_create_file() {
        let root = fake_sysfs("create");
        let id = create(&root, GPU, "nvidia-222", Some(UUID)).unwrap();
        assert_eq!(id, UUID);
        let written = fs::read_to_string(
            root.join("class/mdev_bus")
                .join(GPU)
                .join("mdev_supported_types/nvidia-222/create"),
        )
        .unwrap();
        assert_eq!(written, UUID);
        // Without a uuid one is generated and is valid.
        let gen = create(&root, GPU, "nvidia-222", None).unwrap();
        assert!(uuid::Uuid::parse_str(&gen).is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn create_refuses_unknown_type_exhausted_type_and_bad_input() {
        let root = fake_sysfs("refuse");
        assert!(matches!(
            create(&root, GPU, "nvidia-999", None),
            Err(LibvirtError::NotFound(_))
        ));
        assert!(matches!(
            create(&root, GPU, "nvidia-223", None),
            Err(LibvirtError::Operation(_))
        ));
        assert!(create(&root, "../../etc", "nvidia-222", None).is_err());
        assert!(create(&root, GPU, "../x", None).is_err());
        assert!(create(&root, GPU, "nvidia-222", Some("not-a-uuid")).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn lists_instances_and_removes_one() {
        use std::os::unix::fs::symlink;
        let root = fake_sysfs("inst");
        // <parent>/<uuid> is the real directory; bus/mdev/devices/<uuid> links to it.
        let real = root.join("devices/pci0000:00").join(GPU).join(UUID);
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("remove"), "").unwrap();
        let tdir = root
            .join("class/mdev_bus")
            .join(GPU)
            .join("mdev_supported_types/nvidia-222");
        symlink(&tdir, real.join("mdev_type")).unwrap();
        symlink(&real, root.join("bus/mdev/devices").join(UUID)).unwrap();

        let inst = list_instances(&root);
        assert_eq!(inst.len(), 1);
        assert_eq!(inst[0].uuid, UUID);
        assert_eq!(inst[0].parent, GPU);
        assert_eq!(inst[0].type_id, "nvidia-222");

        remove(&root, UUID).unwrap();
        assert_eq!(fs::read_to_string(real.join("remove")).unwrap(), "1");
        assert!(matches!(
            remove(&root, "99999999-2222-3333-4444-555555555555"),
            Err(LibvirtError::NotFound(_))
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hostdev_xml_is_unmanaged_mdev() {
        let x = mdev_hostdev_xml(UUID);
        assert!(x.contains("type='mdev'"));
        assert!(x.contains("managed='no'"));
        assert!(x.contains(&format!("uuid='{UUID}'")));
    }

    #[test]
    fn validators_normalise_and_reject() {
        assert_eq!(validate_parent("0000:03:00.0").unwrap(), "0000:03:00.0");
        assert!(validate_parent("03:00.0x").is_err());
        assert_eq!(validate_uuid(&UUID.to_uppercase()).unwrap(), UUID);
        assert!(validate_type_id("a b").is_err());
        assert!(validate_type_id(".hidden").is_err());
        assert!(validate_type_id("i915-GVTg_V5_4").is_ok());
    }
}
