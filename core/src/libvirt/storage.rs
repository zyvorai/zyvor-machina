// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::path::Path;

use virt::connect::Connect;
use virt::storage_pool::StoragePool;
use virt::storage_vol::StorageVol;

use crate::state::{StoragePoolInfo, StorageVolumeInfo};
use crate::LibvirtError;

fn lookup_pool(conn: &Connect, name: &str) -> Result<StoragePool, LibvirtError> {
    StoragePool::lookup_by_name(conn, name)
        .map_err(|e| LibvirtError::NotFound(format!("Pool '{name}' not found: {e}")))
}

/// Map libvirt pool XML to platform backend label (`dir`, `nfs`, `logical`, `iscsi`, `zfs`, …).
pub fn storage_pool_backend_from_xml(xml: &str) -> String {
    let lower = xml.to_ascii_lowercase();
    if lower.contains("<pool type='netfs'") || lower.contains("type=\"netfs\"") {
        return "nfs".into();
    }
    if lower.contains("<pool type='logical'") || lower.contains("type=\"logical\"") {
        return "lvm".into();
    }
    if lower.contains("<pool type='iscsi'") || lower.contains("type=\"iscsi\"") {
        return "iscsi".into();
    }
    if lower.contains("<pool type='rbd'") || lower.contains("type=\"rbd\"") {
        return "ceph".into();
    }
    if lower.contains("<pool type='zfs'") || lower.contains("type=\"zfs\"") {
        return "zfs".into();
    }
    "directory".into()
}

/// Extract `<target>…</path>…` from libvirt storage pool XML (`<path` may include attributes).
pub fn target_path_from_pool_xml(xml: &str) -> Option<String> {
    let lower = xml.to_ascii_lowercase();
    let start = lower.find("<target")?;
    let after = &xml[start..];
    let gt = after.find('>')?;
    let inner_start = start + gt + 1;
    let inner_lower = &lower[inner_start..];
    let close_rel = inner_lower.find("</target>")?;
    let inner = &xml[inner_start..inner_start + close_rel];
    crate::xml::extract_text(inner, "path")
        .or_else(|| crate::xml::extract_simple_text(inner, "path"))
}

/// Sorted unique `<target><path>` values from all defined storage pools.
pub fn list_pool_target_paths(conn: &Connect) -> Result<Vec<String>, LibvirtError> {
    let pools = conn
        .list_all_storage_pools(0)
        .map_err(LibvirtError::map_op("Failed to list storage pools"))?;
    let mut set = std::collections::BTreeSet::new();
    for pool in pools {
        let Ok(xml) = pool.get_xml_desc(0) else {
            continue;
        };
        if let Some(p) = target_path_from_pool_xml(&xml) {
            let t = p.trim().to_string();
            if Path::new(&t).is_absolute() {
                set.insert(t);
            }
        }
    }
    Ok(set.into_iter().collect())
}

/// Directories scanned for disk images / delete allow-list: all pool targets plus machina defaults.
pub fn collect_image_scan_directories(
    conn: &Connect,
) -> Result<Vec<std::path::PathBuf>, LibvirtError> {
    let mut out: Vec<std::path::PathBuf> = list_pool_target_paths(conn)?
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect();
    for extra in [
        "/var/lib/machina/images",
        "/var/lib/libvirt/images",
        "/var/lib/machina/packer-builds",
    ] {
        let pb = std::path::PathBuf::from(extra);
        if !out.iter().any(|p| p == &pb) {
            out.push(pb);
        }
    }
    // The configured ISO-upload directory usually sits under one of the paths
    // above already, but an operator can point it elsewhere — include it
    // explicitly so a custom `iso_upload_dir` doesn't fall outside the allow-list
    // used to validate CD-ROM media / disk-image sources.
    let iso_dir = crate::MachinaConfig::load().libvirt.iso_upload_dir;
    let iso_pb = std::path::PathBuf::from(iso_dir.trim());
    if iso_pb.is_absolute() && !out.iter().any(|p| p == &iso_pb) {
        out.push(iso_pb);
    }
    // The agent's backup dir (MACHINA_BACKUP_DIR, default /var/lib/machina/backups)
    // is a legitimate machina-managed location that backup WRITES to and restore
    // READS from. Include it so both paths work out of the box — without this,
    // local backups fail "output directory must be under storage pool targets"
    // and local restores fail the mirror check. It stays a machina-owned dir, so
    // it doesn't widen the surface to arbitrary host files.
    let backup_dir =
        std::env::var("MACHINA_BACKUP_DIR").unwrap_or_else(|_| "/var/lib/machina/backups".into());
    let bpb = std::path::PathBuf::from(&backup_dir);
    if !out.iter().any(|p| p == &bpb) {
        out.push(bpb);
    }
    Ok(out)
}

/// Prefixes (each ending with `/`) under which disk image delete is allowed.
pub fn disk_image_delete_allowed_prefixes(conn: &Connect) -> Result<Vec<String>, LibvirtError> {
    let dirs = collect_image_scan_directories(conn)?;
    Ok(dirs
        .into_iter()
        .map(|p| {
            let s = p.to_string_lossy().to_string();
            if s.ends_with('/') {
                s
            } else {
                format!("{s}/")
            }
        })
        .collect())
}

/// Ensures `output`'s parent directory exists and lies under [`disk_image_delete_allowed_prefixes`]
/// (same policy as disk image browse / delete).
pub fn assert_new_disk_output_parent_allowed(
    conn: &Connect,
    output: &str,
) -> Result<(), LibvirtError> {
    let out = output.trim();
    if out.is_empty() {
        return Err(LibvirtError::Invalid("output path is empty".into()));
    }
    let pb = Path::new(out);
    if !pb.is_absolute() {
        return Err(LibvirtError::Invalid(
            "output must be an absolute path".into(),
        ));
    }
    if out.contains("/../") || out.ends_with("/..") || out.starts_with("../") {
        return Err(LibvirtError::Invalid(
            "output path must not contain '..'".into(),
        ));
    }
    let parent = pb
        .parent()
        .filter(|x| !x.as_os_str().is_empty())
        .ok_or_else(|| LibvirtError::Invalid("output has no parent directory".into()))?;
    let parent_canon = parent.canonicalize().map_err(|e| {
        LibvirtError::Invalid(format!(
            "output parent directory does not exist or is inaccessible: {e}"
        ))
    })?;
    let mut parent_s = parent_canon.to_string_lossy().to_string();
    if !parent_s.ends_with('/') {
        parent_s.push('/');
    }
    let prefixes = disk_image_delete_allowed_prefixes(conn)?;
    if !prefixes.iter().any(|pref| parent_s.starts_with(pref)) {
        return Err(LibvirtError::Invalid(format!(
            "output directory must be under libvirt storage pool targets or default image dirs (parent {})",
            parent_s.trim_end_matches('/')
        )));
    }
    Ok(())
}

/// Ensures a request-supplied *source* file (e.g. a backup image to restore/read)
/// is an absolute path under the allowed libvirt storage prefixes, with no `..`
/// traversal. Confines unauthenticated agent file reads so a caller cannot slurp
/// arbitrary host files (e.g. `/etc/shadow`) into a VM disk.
pub fn assert_backup_source_within_pools(conn: &Connect, source: &str) -> Result<(), LibvirtError> {
    let src = source.trim();
    if src.is_empty() {
        return Err(LibvirtError::Invalid("source path is empty".into()));
    }
    let pb = Path::new(src);
    if !pb.is_absolute() {
        return Err(LibvirtError::Invalid(
            "source must be an absolute path".into(),
        ));
    }
    if src.contains("/../") || src.ends_with("/..") || src.starts_with("../") {
        return Err(LibvirtError::Invalid(
            "source path must not contain '..'".into(),
        ));
    }
    let canon = pb.canonicalize().map_err(|e| {
        LibvirtError::Invalid(format!(
            "source path does not exist or is inaccessible: {e}"
        ))
    })?;
    let canon_s = canon.to_string_lossy().to_string();
    // Allowed prefixes now include the agent backup dir (see
    // collect_image_scan_directories), so restore-from-backup is accepted.
    let prefixes = disk_image_delete_allowed_prefixes(conn)?;
    if !prefixes.iter().any(|pref| canon_s.starts_with(pref)) {
        return Err(LibvirtError::Invalid(format!(
            "source file must be under libvirt storage pool targets, default image dirs, or the backup dir (path {canon_s})"
        )));
    }
    Ok(())
}

/// Directory for new VM root disks: prefers pool `default`, then any path containing `images`, else first pool path.
pub fn primary_vm_disk_base_dir(conn: &Connect) -> Option<String> {
    let pools = conn.list_all_storage_pools(0).ok()?;
    let mut rows: Vec<(String, String)> = Vec::new();
    for pool in pools {
        let Ok(name) = pool.get_name() else {
            continue;
        };
        let Ok(xml) = pool.get_xml_desc(0) else {
            continue;
        };
        let Some(path) = target_path_from_pool_xml(&xml) else {
            continue;
        };
        if !Path::new(&path).is_absolute() {
            continue;
        }
        rows.push((name, path.trim_end_matches('/').to_string()));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows.iter()
        .find(|(n, _)| n == "default")
        .or_else(|| rows.iter().find(|(_, p)| p.contains("images")))
        .or_else(|| rows.first())
        .map(|(_, p)| p.clone())
}

pub fn list_pools(conn: &Connect) -> Result<Vec<StoragePoolInfo>, LibvirtError> {
    let pools = conn
        .list_all_storage_pools(0)
        .map_err(LibvirtError::map_op("Failed to list storage pools"))?;

    let mut result = Vec::new();
    for pool in pools {
        let name = pool
            .get_name()
            .map_err(LibvirtError::map_op("Failed to get pool name"))?;

        let (state, capacity_gb, allocation_gb, available_gb) = match pool.get_info().ok() {
            Some(i) => (
                pool_state_to_string(i.state),
                bytes_to_gb(i.capacity),
                bytes_to_gb(i.allocation),
                bytes_to_gb(i.available),
            ),
            None => (crate::unknown_string(), 0.0, 0.0, 0.0),
        };

        result.push(StoragePoolInfo {
            name,
            uuid: pool.get_uuid_string().unwrap_or_default(),
            state,
            capacity_gb,
            allocation_gb,
            available_gb,
            autostart: pool.get_autostart().unwrap_or(false),
        });
    }

    Ok(result)
}

pub fn list_volumes(
    conn: &Connect,
    pool_name: &str,
) -> Result<Vec<StorageVolumeInfo>, LibvirtError> {
    let pool = lookup_pool(conn, pool_name)?;
    if let Err(e) = pool.refresh(0) {
        tracing::debug!("Pool refresh for '{}' failed (non-fatal): {}", pool_name, e);
    }

    let vol_list = pool
        .list_all_volumes(0)
        .map_err(LibvirtError::map_op("Failed to list volumes"))?;

    let mut result = Vec::new();
    for vol in vol_list {
        let name = vol
            .get_name()
            .map_err(LibvirtError::map_op("Failed to get volume name"))?;

        let (vol_type, capacity_gb, allocation_gb) = match vol.get_info().ok() {
            Some(i) => (
                vol_type_to_string(i.kind),
                bytes_to_gb(i.capacity),
                bytes_to_gb(i.allocation),
            ),
            None => (crate::unknown_string(), 0.0, 0.0),
        };

        result.push(StorageVolumeInfo {
            name,
            pool: pool_name.to_string(),
            capacity_gb,
            allocation_gb,
            path: vol.get_path().unwrap_or_default(),
            vol_type,
        });
    }

    Ok(result)
}

/// Names of VMs that currently have `disk_path` as a `device='disk'` source,
/// paired with whether each is running. Checks both the live and the
/// persistent (offline) definition — see `domain_xml_live_and_config` — so a
/// disk that only *config*-detached (`requires_restart: true`, guest never
/// acknowledged the live hot-unplug) still counts as in use.
fn vms_with_disk_source(conn: &Connect, disk_path: &str) -> Vec<(String, bool)> {
    let Ok(domains) = conn.list_all_domains(0) else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    for domain in domains {
        let Ok(name) = domain.get_name() else {
            continue;
        };
        let xml = super::domain::domain_xml_live_and_config(&domain);
        let attached = crate::xml::split_blocks(&xml, "disk").iter().any(|block| {
            crate::xml::extract_attr(block, "disk", "device").as_deref() == Some("disk")
                && crate::xml::extract_attr(block, "source", "file").as_deref() == Some(disk_path)
        });
        if attached {
            let running = domain.get_info().map(|i| i.state == 1).unwrap_or(false);
            hits.push((name, running));
        }
    }
    hits
}

pub fn delete_volume(conn: &Connect, pool_name: &str, vol_name: &str) -> Result<(), LibvirtError> {
    // Match create_volume's validation — reject names outside [alnum._-].
    crate::validate::validate_name(vol_name)?;
    let pool = lookup_pool(conn, pool_name)?;
    let vol = StorageVol::lookup_by_name(&pool, vol_name)
        .map_err(|e| LibvirtError::NotFound(format!("Volume '{vol_name}' not found: {e}")))?;
    // Deleting the backing file out from under a domain that still references it (even
    // only in the persistent/offline definition) leaves that domain unable to boot —
    // unlike overwriting an ISO in place (see vms_with_iso_mounted), there is no stale
    // file descriptor to fall back on once the file is gone. Refuse instead of silently
    // corrupting a VM's disk reference.
    if let Ok(path) = vol.get_path() {
        let hits = vms_with_disk_source(conn, &path);
        if !hits.is_empty() {
            let names: Vec<&str> = hits.iter().map(|(n, _)| n.as_str()).collect();
            return Err(LibvirtError::Operation(format!(
                "Volume '{vol_name}' is still attached to: {} — fully detach it (and restart the VM if required) before deleting",
                names.join(", ")
            )));
        }
    }
    vol.delete(0)
        .map_err(LibvirtError::map_op("Failed to delete volume"))?;
    Ok(())
}

pub fn start_pool(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    let pool = lookup_pool(conn, name)?;
    pool.create(0)
        .map_err(|e| LibvirtError::Operation(format!("Failed to start pool '{name}': {e}")))?;
    Ok(())
}

pub fn stop_pool(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    let pool = lookup_pool(conn, name)?;
    pool.destroy()
        .map_err(|e| LibvirtError::Operation(format!("Failed to stop pool '{name}': {e}")))?;
    Ok(())
}

pub fn create_volume(
    conn: &Connect,
    pool_name: &str,
    vol_name: &str,
    capacity_gb: u64,
    format: &str,
) -> Result<String, LibvirtError> {
    crate::validate::validate_name(vol_name)?;
    let pool = lookup_pool(conn, pool_name)?;

    let capacity_bytes = capacity_gb
        .checked_mul(1024 * 1024 * 1024)
        .ok_or_else(|| LibvirtError::Operation("Capacity overflow".to_string()))?;
    let xml = format!(
        r#"<volume>
  <name>{}</name>
  <capacity unit='bytes'>{capacity_bytes}</capacity>
  <target>
    <format type='{}'/>
  </target>
</volume>"#,
        crate::xml::escape(vol_name),
        crate::xml::escape(format),
    );

    let vol = StorageVol::create_xml(&pool, &xml, 0).map_err(|e| {
        LibvirtError::Operation(format!("Failed to create volume '{vol_name}': {e}"))
    })?;
    // The path is what a disk attach needs; callers store it with the volume.
    vol.get_path()
        .map_err(|e| LibvirtError::Operation(format!("Created volume '{vol_name}' but could not read its path: {e}")))
}

pub fn set_pool_autostart(conn: &Connect, name: &str, autostart: bool) -> Result<(), LibvirtError> {
    let pool = lookup_pool(conn, name)?;
    pool.set_autostart(autostart)
        .map_err(LibvirtError::map_op("Failed to set pool autostart"))?;
    Ok(())
}

pub fn refresh_pool(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    let pool = lookup_pool(conn, name)?;
    pool.refresh(0)
        .map_err(|e| LibvirtError::Operation(format!("Failed to refresh pool '{name}': {e}")))?;
    Ok(())
}

pub fn create_pool(
    conn: &Connect,
    name: &str,
    pool_type: &str,
    target_path: &str,
) -> Result<(), LibvirtError> {
    crate::validate::validate_name(name)?;
    if pool_type == "iscsi" {
        return Err(LibvirtError::Invalid(
            "an iSCSI pool needs a portal and target: use POST /api/v1/storage/iscsi-pools".into(),
        ));
    }

    let xml = match pool_type {
        "dir" => format!(
            r#"<pool type='dir'>
  <name>{}</name>
  <target><path>{}</path></target>
</pool>"#,
            crate::xml::escape(name),
            crate::xml::escape(target_path),
        ),
        "logical" => format!(
            r#"<pool type='logical'>
  <name>{}</name>
  <source><name>{}</name></source>
  <target><path>{}</path></target>
</pool>"#,
            crate::xml::escape(name),
            crate::xml::escape(name),
            crate::xml::escape(target_path),
        ),
        _ => format!(
            r#"<pool type='{}'>
  <name>{}</name>
  <target><path>{}</path></target>
</pool>"#,
            crate::xml::escape(pool_type),
            crate::xml::escape(name),
            crate::xml::escape(target_path),
        ),
    };

    let pool = StoragePool::define_xml(conn, &xml, 0)
        .map_err(|e| LibvirtError::Operation(format!("Failed to create pool '{name}': {e}")))?;
    pool.build(0).ok(); // build may fail for some pool types, that's fine
    pool.create(0)
        .map_err(|e| LibvirtError::Operation(format!("Failed to start pool '{name}': {e}")))?;
    pool.set_autostart(true).ok();
    Ok(())
}

pub fn delete_pool(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    let pool = lookup_pool(conn, name)?;
    if pool.is_active().unwrap_or(false) {
        let _ = pool.destroy();
    }
    pool.undefine()
        .map_err(|e| LibvirtError::Operation(format!("Failed to delete pool '{name}': {e}")))?;
    Ok(())
}

pub fn get_pool_xml(conn: &Connect, name: &str) -> Result<String, LibvirtError> {
    let pool = lookup_pool(conn, name)?;
    pool.get_xml_desc(0)
        .map_err(LibvirtError::map_op("Failed to get pool XML"))
}

pub fn resize_volume(
    conn: &Connect,
    pool_name: &str,
    vol_name: &str,
    capacity_gb: u64,
) -> Result<(), LibvirtError> {
    let pool = lookup_pool(conn, pool_name)?;
    let vol = StorageVol::lookup_by_name(&pool, vol_name)
        .map_err(|e| LibvirtError::NotFound(format!("Volume '{}' not found: {}", vol_name, e)))?;
    let capacity_bytes = capacity_gb
        .checked_mul(1024 * 1024 * 1024)
        .ok_or_else(|| LibvirtError::Operation("Capacity overflow".to_string()))?;
    vol.resize(capacity_bytes, 0).map_err(|e| {
        LibvirtError::Operation(format!("Failed to resize volume '{vol_name}': {e}"))
    })?;
    Ok(())
}

pub fn clone_volume(
    conn: &Connect,
    pool_name: &str,
    src_vol: &str,
    new_name: &str,
) -> Result<(), LibvirtError> {
    crate::validate::validate_name(new_name)?;
    let pool = lookup_pool(conn, pool_name)?;
    let vol = StorageVol::lookup_by_name(&pool, src_vol)
        .map_err(|e| LibvirtError::NotFound(format!("Volume '{}' not found: {}", src_vol, e)))?;

    let vol_info = vol
        .get_info()
        .map_err(LibvirtError::map_op("Failed to get volume info"))?;
    let xml = format!(
        r#"<volume>
  <name>{}</name>
  <capacity unit='bytes'>{}</capacity>
</volume>"#,
        crate::xml::escape(new_name),
        vol_info.capacity,
    );

    StorageVol::create_xml_from(&pool, &xml, &vol, 0)
        .map_err(|e| LibvirtError::Operation(format!("Failed to clone volume: {e}")))?;
    Ok(())
}

fn bytes_to_gb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

fn pool_state_to_string(state: u32) -> String {
    match state {
        0 => "inactive".to_string(),
        1 => "building".to_string(),
        2 => "running".to_string(),
        3 => "degraded".to_string(),
        4 => "inaccessible".to_string(),
        _ => format!("unknown ({state})"),
    }
}

fn vol_type_to_string(kind: u32) -> String {
    match kind {
        0 => "file".to_string(),
        1 => "block".to_string(),
        2 => "dir".to_string(),
        3 => "network".to_string(),
        4 => "netdir".to_string(),
        5 => "ploop".to_string(),
        _ => format!("unknown ({kind})"),
    }
}

#[cfg(test)]
mod tests {
    use super::target_path_from_pool_xml;

    #[test]
    fn target_path_dir_pool() {
        let xml = r#"<pool type='dir'>
  <name>default</name>
  <target>
    <path>/data/libvirt/images</path>
  </target>
</pool>"#;
        assert_eq!(
            target_path_from_pool_xml(xml).as_deref(),
            Some("/data/libvirt/images")
        );
    }

    #[test]
    fn target_path_with_permissions_attr() {
        let xml = r#"<pool type='dir'>
  <target>
    <path permissions='0711'>/var/lib/libvirt/images</path>
  </target>
</pool>"#;
        assert_eq!(
            target_path_from_pool_xml(xml).as_deref(),
            Some("/var/lib/libvirt/images")
        );
    }
}
