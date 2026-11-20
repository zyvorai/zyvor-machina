// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Point-in-time, multi-disk VM backups.
//!
//! A running VM is backed up with libvirt's `backup-begin` (push mode): QEMU fixes one instant for **all** disks at
//! once and copies them in the background, so the result is never a torn image. Around the instant the guest's
//! filesystems are frozen through the guest agent when it answers, which makes the backup application-consistent;
//! without an agent it is crash-consistent. A stopped VM is simply converted. Every output is checked with
//! `qemu-img check` before the backup is called good.
//!
//! Layout: a VM with one data disk keeps the original single-file format (`<name>.qcow2`). A VM with several disks
//! is a directory (`<name>.d/`) holding one qcow2 per disk plus `manifest.json`, restored disk by disk.

use std::path::{Path, PathBuf};

use machina_core::xml::{extract_attr, split_blocks};
use serde::{Deserialize, Serialize};

pub const MANIFEST: &str = "manifest.json";

/// One data disk of a domain.
#[derive(Debug, Clone, PartialEq)]
pub struct DataDisk {
    /// Target device name in the guest, e.g. `vda`.
    pub target: String,
    /// What `qemu-img` should be given to read or write it (a path, or `rbd:pool/image`).
    pub qemu_arg: String,
    pub rbd: bool,
}

/// The domain's data disks (`device='disk'`, local file or network/RBD), in XML order. CD-ROMs and floppies are skipped.
pub fn data_disks(xml: &str) -> Vec<DataDisk> {
    split_blocks(xml, "disk")
        .into_iter()
        .filter_map(|b| {
            if extract_attr(&b, "disk", "device").as_deref() != Some("disk") {
                return None;
            }
            let target = extract_attr(&b, "target", "dev")?;
            match extract_attr(&b, "disk", "type").as_deref() {
                Some("network") => {
                    let name = extract_attr(&b, "source", "name")?;
                    Some(DataDisk {
                        target,
                        qemu_arg: format!("rbd:{name}"),
                        rbd: true,
                    })
                }
                _ => {
                    let file = extract_attr(&b, "source", "file")?;
                    Some(DataDisk {
                        target,
                        qemu_arg: file,
                        rbd: false,
                    })
                }
            }
        })
        .collect()
}

/// Where the backup goes: one file, or a directory with one file per disk.
#[derive(Debug, Clone, PartialEq)]
pub enum Layout {
    Single(PathBuf),
    Multi(PathBuf),
}

impl Layout {
    /// `dest_path` is the single-file name the controller picked (`….qcow2`); several disks turn it into `….d/`.
    pub fn for_disks(dest_path: &str, disks: usize) -> Layout {
        if disks <= 1 {
            Layout::Single(PathBuf::from(dest_path))
        } else {
            let base = dest_path.strip_suffix(".qcow2").unwrap_or(dest_path);
            Layout::Multi(PathBuf::from(format!("{base}.d")))
        }
    }

    /// The path recorded as the backup's identity.
    pub fn path(&self) -> &Path {
        match self {
            Layout::Single(p) | Layout::Multi(p) => p,
        }
    }

    /// Output file for one disk.
    pub fn file_for(&self, disk: &DataDisk) -> PathBuf {
        match self {
            Layout::Single(p) => p.clone(),
            Layout::Multi(dir) => dir.join(format!("{}.qcow2", disk.target)),
        }
    }
}

/// The `<domainbackup>` document for a push-mode backup of every given disk.
pub fn backup_xml(layout: &Layout, disks: &[DataDisk]) -> String {
    let mut s = String::from("<domainbackup mode='push'><disks>");
    for d in disks {
        s.push_str(&format!(
            "<disk name='{}' backup='yes' type='file'><target file='{}'/><driver type='qcow2'/></disk>",
            d.target,
            xml_escape(&layout.file_for(d).to_string_lossy())
        ));
    }
    s.push_str("</disks></domainbackup>");
    s
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ManifestDisk {
    pub target: String,
    pub file: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub vm: String,
    pub created_at: String,
    /// "application-consistent" or "crash-consistent" (or "offline").
    pub consistency: String,
    pub disks: Vec<ManifestDisk>,
}

/// Pair each backed-up disk with the domain's current disk of the same target. Refuses any mismatch, so a restore
/// can never write one disk's data over a different disk or leave a disk stale.
pub fn pair_for_restore(
    manifest: &Manifest,
    domain_disks: &[DataDisk],
) -> Result<Vec<(String, DataDisk)>, String> {
    let mut want: Vec<&str> = manifest.disks.iter().map(|d| d.target.as_str()).collect();
    let mut have: Vec<&str> = domain_disks.iter().map(|d| d.target.as_str()).collect();
    want.sort_unstable();
    have.sort_unstable();
    if want != have {
        return Err(format!(
            "the backup has disks [{}] but the VM now has [{}]; refusing to restore a partial or mismatched set",
            want.join(", "),
            have.join(", ")
        ));
    }
    Ok(manifest
        .disks
        .iter()
        .map(|m| {
            let d = domain_disks
                .iter()
                .find(|d| d.target == m.target)
                .expect("targets verified equal")
                .clone();
            (m.file.clone(), d)
        })
        .collect())
}

/// Reject manifest file names that could escape the backup directory.
pub fn safe_member(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains("..") && !name.starts_with('.')
}

/// `Job type:` line of `virsh domjobinfo`, lower-cased ("none", "bounded", "unbounded", "completed", "failed"…).
pub fn job_type(domjobinfo: &str) -> String {
    domjobinfo
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("job type")
                .then(|| v.trim().to_lowercase())
        })
        .unwrap_or_default()
}

/// `qemu-img check` exit codes that mean the image is usable: 0 (clean) and 3 (only leaked clusters).
pub fn check_accepts(code: Option<i32>) -> bool {
    matches!(code, Some(0) | Some(3))
}

/// The image files that make up a stored backup: the file itself, or every member named by a directory's manifest.
pub fn images_in(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_dir() {
        let raw = std::fs::read_to_string(path.join(MANIFEST))
            .map_err(|e| format!("cannot read the backup manifest: {e}"))?;
        let m: Manifest = serde_json::from_str(&raw)
            .map_err(|e| format!("the backup manifest is damaged: {e}"))?;
        if m.disks.is_empty() {
            return Err("the backup manifest lists no disks".into());
        }
        m.disks
            .iter()
            .map(|d| {
                if safe_member(&d.file) {
                    Ok(path.join(&d.file))
                } else {
                    Err(format!("manifest names an unsafe file '{}'", d.file))
                }
            })
            .collect()
    } else if path.is_file() {
        Ok(vec![path.to_path_buf()])
    } else {
        Err("the backup is missing".into())
    }
}

/// True when `path` is absolute, free of `..`, and inside `root` (but not the root itself).
pub fn within_root(path: &str, root: &str) -> bool {
    let p = Path::new(path);
    p.is_absolute() && !path.contains("..") && p.starts_with(root) && p != Path::new(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = "<domain><devices>\
<disk type='file' device='disk'><driver name='qemu' type='qcow2'/><source file='/var/lib/libvirt/images/a.qcow2'/><target dev='vda' bus='virtio'/></disk>\
<disk type='network' device='disk'><source protocol='rbd' name='pool/vol'/><target dev='vdb' bus='virtio'/></disk>\
<disk type='file' device='cdrom'><source file='/iso/x.iso'/><target dev='sda' bus='sata'/></disk>\
</devices></domain>";

    #[test]
    fn finds_file_and_rbd_data_disks_and_skips_cdroms() {
        let d = data_disks(XML);
        assert_eq!(d.len(), 2);
        assert_eq!(
            (d[0].target.as_str(), d[0].qemu_arg.as_str(), d[0].rbd),
            ("vda", "/var/lib/libvirt/images/a.qcow2", false)
        );
        assert_eq!(
            (d[1].target.as_str(), d[1].qemu_arg.as_str(), d[1].rbd),
            ("vdb", "rbd:pool/vol", true)
        );
    }

    #[test]
    fn one_disk_keeps_the_single_file_format_several_make_a_directory() {
        assert_eq!(
            Layout::for_disks("/b/x-1.qcow2", 1),
            Layout::Single("/b/x-1.qcow2".into())
        );
        assert_eq!(
            Layout::for_disks("/b/x-1.qcow2", 2),
            Layout::Multi("/b/x-1.d".into())
        );
    }

    #[test]
    fn backup_xml_names_every_disk_and_escapes_paths() {
        let disks = data_disks(XML);
        let l = Layout::for_disks("/b/o&p.qcow2", 2);
        let x = backup_xml(&l, &disks);
        assert!(x.contains("name='vda'") && x.contains("name='vdb'"));
        assert!(x.contains("/b/o&amp;p.d/vda.qcow2"));
        assert!(x.starts_with("<domainbackup mode='push'>"));
    }

    #[test]
    fn restore_pairing_refuses_a_mismatched_disk_set() {
        let disks = data_disks(XML);
        let m = |t: &[&str]| Manifest {
            vm: "v".into(),
            created_at: String::new(),
            consistency: "crash-consistent".into(),
            disks: t
                .iter()
                .map(|t| ManifestDisk {
                    target: (*t).into(),
                    file: format!("{t}.qcow2"),
                })
                .collect(),
        };
        let ok = pair_for_restore(&m(&["vdb", "vda"]), &disks).unwrap();
        assert_eq!(ok.len(), 2);
        assert!(pair_for_restore(&m(&["vda"]), &disks)
            .unwrap_err()
            .contains("mismatched"));
        assert!(pair_for_restore(&m(&["vda", "vdc"]), &disks).is_err());
    }

    #[test]
    fn manifest_round_trips_and_member_names_are_confined() {
        let m = Manifest {
            vm: "v".into(),
            created_at: "t".into(),
            consistency: "offline".into(),
            disks: vec![ManifestDisk {
                target: "vda".into(),
                file: "vda.qcow2".into(),
            }],
        };
        let back: Manifest = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back, m);
        assert!(safe_member("vda.qcow2"));
        for bad in ["../x", "a/b", "", ".hidden", "..", "/etc/passwd"] {
            assert!(!safe_member(bad), "{bad}");
        }
    }

    #[test]
    fn reads_the_job_type_from_domjobinfo() {
        assert_eq!(job_type("Job type:         None   \n"), "none");
        assert_eq!(
            job_type("Job type:         Unbounded\nTime elapsed: 5 ms"),
            "unbounded"
        );
        assert_eq!(job_type("nothing here"), "");
    }

    #[test]
    fn check_exit_codes_and_root_confinement() {
        assert!(check_accepts(Some(0)) && check_accepts(Some(3)));
        assert!(!check_accepts(Some(2)) && !check_accepts(Some(1)) && !check_accepts(None));
        assert!(within_root(
            "/var/lib/machina/backups/a.qcow2",
            "/var/lib/machina/backups"
        ));
        assert!(!within_root(
            "/var/lib/machina/backups",
            "/var/lib/machina/backups"
        ));
        assert!(!within_root(
            "/var/lib/machina/backups/../x",
            "/var/lib/machina/backups"
        ));
        assert!(!within_root("/etc/passwd", "/var/lib/machina/backups"));
        assert!(!within_root("rel/a", "/var/lib/machina/backups"));
    }

    #[test]
    fn images_in_reads_a_directory_manifest_and_rejects_the_missing() {
        let dir = std::env::temp_dir().join(format!("machina-bk-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let m = Manifest {
            vm: "v".into(),
            created_at: "t".into(),
            consistency: "offline".into(),
            disks: vec![
                ManifestDisk {
                    target: "vda".into(),
                    file: "vda.qcow2".into(),
                },
                ManifestDisk {
                    target: "vdb".into(),
                    file: "vdb.qcow2".into(),
                },
            ],
        };
        std::fs::write(dir.join(MANIFEST), serde_json::to_string(&m).unwrap()).unwrap();
        let imgs = images_in(&dir).unwrap();
        assert_eq!(imgs, vec![dir.join("vda.qcow2"), dir.join("vdb.qcow2")]);
        assert!(images_in(&dir.join("nope")).is_err());
        let bad = Manifest {
            disks: vec![ManifestDisk {
                target: "x".into(),
                file: "../evil".into(),
            }],
            ..m
        };
        std::fs::write(dir.join(MANIFEST), serde_json::to_string(&bad).unwrap()).unwrap();
        assert!(images_in(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
