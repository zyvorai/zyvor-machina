// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Extra features: ISO/disk browser, USB passthrough, cloud-init, VM import, live resize, tags, PCI listing.

mod pci;
mod usb;

pub use pci::*;
pub use usb::*;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use virt::connect::Connect;

use super::automation::with_json_lock;
use super::domain::{lookup_domain, poll_until};
use super::resize::{memory_near_target, MemoryApplyOutcome, BALLOON_WAIT};
use super::storage;
use crate::LibvirtError;

/// Escape a string for safe inclusion in YAML single-quoted scalars.
/// Wraps the value in single quotes and escapes internal single quotes by doubling them.
fn yaml_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

// ── ISO / Disk Image Browser ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageFile {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    pub format: String, // iso, qcow2, raw, vmdk, img
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseFilesResponse {
    pub files: Vec<ImageFile>,
    /// Absolute directories scanned (libvirt pool targets plus defaults).
    pub scan_directories: Vec<String>,
}

/// One row in a hypervisor directory listing (`browse_directory`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseDirEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseDirResponse {
    /// Canonical absolute path of the directory being listed.
    pub path: String,
    /// Nearest ancestor directory that is still inside an allowed root (for “up”).
    pub parent: Option<String>,
    pub entries: Vec<BrowseDirEntry>,
    /// Allowed root directories (shortcuts in the UI).
    pub roots: Vec<String>,
}

fn browse_scan_dirs_to_strings(dirs: &[PathBuf]) -> Vec<String> {
    dirs.iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect()
}

/// Scan ISO files under libvirt pool directories (dynamic) plus `/home`, `/root`, `/tmp`.
pub fn list_iso_files(conn: &Connect) -> Result<BrowseFilesResponse, LibvirtError> {
    let mut dirs = storage::collect_image_scan_directories(conn)?;
    for extra in ["/home", "/root", "/tmp"] {
        let pb = PathBuf::from(extra);
        if !dirs.iter().any(|p| p == &pb) {
            dirs.push(pb);
        }
    }
    let scan_directories = browse_scan_dirs_to_strings(&dirs);
    let mut files = Vec::new();
    for dir in &dirs {
        scan_dir_for_extension(dir, &["iso"], &mut files, 2);
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(BrowseFilesResponse {
        files,
        scan_directories,
    })
}

/// Scan disk images under all libvirt dir-pool targets plus `/var/lib/machina/images` and `/var/lib/libvirt/images` if missing.
pub fn list_disk_images(conn: &Connect) -> Result<BrowseFilesResponse, LibvirtError> {
    let dirs = storage::collect_image_scan_directories(conn)?;
    let scan_directories = browse_scan_dirs_to_strings(&dirs);
    let mut files = Vec::new();
    for dir in &dirs {
        scan_dir_for_extension(dir, &["qcow2", "raw", "img", "vmdk"], &mut files, 1);
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(BrowseFilesResponse {
        files,
        scan_directories,
    })
}

/// Shortcut directories shown in the browse UI (chips). Always includes filesystem root `/` so any
/// absolute path on the hypervisor is reachable like a remote-server file picker (subject to OS
/// permissions for the daemon user). Pool targets and common paths are extra shortcuts.
fn collect_browse_roots(conn: &Connect) -> Result<Vec<PathBuf>, LibvirtError> {
    let mut dirs = storage::collect_image_scan_directories(conn)?;
    for extra in [
        "/", "/data", "/home", "/root", "/tmp", "/srv", "/media", "/mnt", "/opt",
    ] {
        let pb = PathBuf::from(extra);
        if pb.is_dir() {
            dirs.push(pb);
        }
    }
    let mut canonical: Vec<PathBuf> = Vec::new();
    for p in dirs {
        let Ok(c) = p.canonicalize() else { continue };
        if !canonical.iter().any(|x| x == &c) {
            canonical.push(c);
        }
    }
    canonical.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    // Default empty-path browse and left-to-right shortcuts: `/` first so the whole FS is obvious.
    if let Some(i) = canonical
        .iter()
        .position(|p| p.as_os_str() == std::ffi::OsStr::new("/"))
    {
        let root = canonical.remove(i);
        canonical.insert(0, root);
    }
    Ok(canonical)
}

fn path_under_any_root(canonical: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| canonical.starts_with(r))
}

fn browse_parent(canonical_dir: &Path, roots: &[PathBuf]) -> Option<String> {
    let mut p = canonical_dir.to_path_buf();
    while let Some(parent) = p.parent() {
        let parent_canon = parent.canonicalize().ok()?;
        if parent_canon == p {
            return None;
        }
        if path_under_any_root(&parent_canon, roots) {
            return Some(parent_canon.to_string_lossy().to_string());
        }
        p = parent_canon;
    }
    None
}

/// List a single directory on the hypervisor (ISO/disk picker). With `/` among the roots, any
/// absolute path is allowed after canonicalization; entries outside those trees are skipped.
pub fn browse_directory(conn: &Connect, raw_path: &str) -> Result<BrowseDirResponse, LibvirtError> {
    if raw_path.contains("..") {
        return Err(LibvirtError::Invalid("Path traversal not allowed".into()));
    }
    let roots = collect_browse_roots(conn)?;
    if roots.is_empty() {
        return Err(LibvirtError::Invalid(
            "No browse roots (filesystem root / could not be resolved).".into(),
        ));
    }
    let roots_str: Vec<String> = roots
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    let trimmed = raw_path.trim();
    let canonical_dir = if trimmed.is_empty() {
        roots[0].clone()
    } else {
        let p = PathBuf::from(trimmed);
        if !p.is_absolute() {
            return Err(LibvirtError::Invalid(
                "path must be an absolute path on the hypervisor".into(),
            ));
        }
        let c = p
            .canonicalize()
            .map_err(|e| LibvirtError::Invalid(format!("Cannot resolve path '{trimmed}': {e}")))?;
        if !path_under_any_root(&c, &roots) {
            return Err(LibvirtError::Forbidden(format!(
                "Path is outside allowed directories: {}",
                c.display()
            )));
        }
        if !c.is_dir() {
            return Err(LibvirtError::Invalid(format!(
                "Not a directory: {}",
                c.display()
            )));
        }
        c
    };

    if !canonical_dir.is_dir() {
        return Err(LibvirtError::Invalid(format!(
            "Not a directory: {}",
            canonical_dir.display()
        )));
    }

    let read = std::fs::read_dir(&canonical_dir).map_err(|e| {
        LibvirtError::Operation(format!(
            "Failed to read directory {}: {e}",
            canonical_dir.display()
        ))
    })?;

    let mut entries: Vec<BrowseDirEntry> = Vec::new();
    for e in read.flatten() {
        let child = e.path();
        let Ok(child_canon) = child.canonicalize() else {
            continue;
        };
        if !path_under_any_root(&child_canon, &roots) {
            continue;
        }
        let is_directory = child_canon.is_dir();
        let name = e.file_name().to_string_lossy().to_string();
        let size_bytes = if is_directory {
            0
        } else {
            std::fs::metadata(&child_canon)
                .map(|m| m.len())
                .unwrap_or(0)
        };
        entries.push(BrowseDirEntry {
            name,
            path: child_canon.to_string_lossy().to_string(),
            is_directory,
            size_bytes,
        });
    }

    entries.sort_by(|a, b| match (a.is_directory, b.is_directory) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    let path_str = canonical_dir.to_string_lossy().to_string();
    let parent = browse_parent(&canonical_dir, &roots);

    Ok(BrowseDirResponse {
        path: path_str,
        parent,
        entries,
        roots: roots_str,
    })
}

fn scan_dir_for_extension(
    dir: &Path,
    extensions: &[&str],
    files: &mut Vec<ImageFile>,
    max_depth: u32,
) {
    scan_dir_recursive(dir, extensions, files, 0, max_depth);
}

fn scan_dir_recursive(
    dir: &Path,
    extensions: &[&str],
    files: &mut Vec<ImageFile>,
    depth: u32,
    max_depth: u32,
) {
    if depth > max_depth {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && depth < max_depth {
            scan_dir_recursive(&path, extensions, files, depth + 1, max_depth);
        } else if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if extensions.iter().any(|e| *e == ext_lower) {
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    files.push(ImageFile {
                        path: path.to_string_lossy().to_string(),
                        name: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                        size_bytes: size,
                        format: ext_lower,
                    });
                }
            }
        }
    }
}

// ── mkosi Workspace Browser ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MkosiWorkspace {
    pub path: String,
    pub name: String,
    /// Images declared in this workspace (subdirectories each containing `mkosi.conf`, for image trees).
    pub images: Vec<String>,
}

/// Scan standard base directories for mkosi workspaces (subdirs containing `mkosi.conf`).
pub fn list_mkosi_workspaces() -> Vec<MkosiWorkspace> {
    let bases = [
        "/var/lib/machina/mkosi-defs",
        "/var/lib/machina/mkosi",
        "/etc/machina/mkosi-defs",
    ];
    let mut out = Vec::new();
    for base in &bases {
        let Ok(entries) = std::fs::read_dir(base) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if !path.join("mkosi.conf").is_file() {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            // Collect sub-images: subdirs that also contain mkosi.conf (image trees).
            let mut images: Vec<String> = Vec::new();
            if let Ok(sub) = std::fs::read_dir(&path) {
                for se in sub.flatten() {
                    let sp = se.path();
                    if sp.is_dir() && sp.join("mkosi.conf").is_file() {
                        if let Some(n) = sp.file_name() {
                            images.push(n.to_string_lossy().to_string());
                        }
                    }
                }
            }
            images.sort();
            out.push(MkosiWorkspace {
                path: path.to_string_lossy().to_string(),
                name,
                images,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

// ── Cloud-init ─────────────────────────────────────────────────────

/// Generate a cloud-init ISO with user-data and meta-data.
/// When `output_path` is empty, writes under `default_images_dir` (typically the primary libvirt images pool).
pub fn generate_cloud_init_iso(
    output_path: &str,
    default_images_dir: &str,
    hostname: &str,
    username: &str,
    password: &str,
    ssh_key: &str,
    libvirt_cfg: Option<&crate::config::LibvirtConfig>,
) -> Result<String, LibvirtError> {
    generate_cloud_init_iso_with(
        output_path,
        default_images_dir,
        hostname,
        username,
        password,
        ssh_key,
        None,
        libvirt_cfg,
    )
}

/// A per-VM scratch directory for seed files that is always removed, including on early error returns. (It used to be one
/// fixed shared path, so two VMs created at once could overwrite each other's seed, and a failure left credentials behind.)
struct SeedScratch(PathBuf);

impl SeedScratch {
    fn create(hostname: &str) -> Result<Self, LibvirtError> {
        Self::create_in(Path::new("/tmp/machina-cloud-init"), hostname)
    }

    /// `create` under a chosen root (the tests use their own: the real one is owned by whichever user ran the agent).
    fn create_in(root: &Path, hostname: &str) -> Result<Self, LibvirtError> {
        let root = root.to_path_buf();
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&root)
            .map_err(|e| LibvirtError::Operation(format!("Failed to create cloud-init temp dir: {e}")))?;
        let safe: String = hostname
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
            .take(64)
            .collect();
        let dir = root.join(format!("{safe}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|e| LibvirtError::Operation(format!("Failed to create cloud-init scratch dir: {e}")))?;
        Ok(Self(dir))
    }
}

impl Drop for SeedScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Like [`generate_cloud_init_iso`], with optional free-form `user_data`. When it is given it becomes the NoCloud `user-data`
/// verbatim and the settings Machina generates (user, key, password, guest agent) move to `vendor-data`, which cloud-init
/// applies alongside it (user-data wins on conflicts). Without it the generated settings are the user-data, as before.
#[allow(clippy::too_many_arguments)]
pub fn generate_cloud_init_iso_with(
    output_path: &str,
    default_images_dir: &str,
    hostname: &str,
    username: &str,
    password: &str,
    ssh_key: &str,
    user_data_override: Option<&str>,
    libvirt_cfg: Option<&crate::config::LibvirtConfig>,
) -> Result<String, LibvirtError> {
    if let Some(d) = user_data_override {
        if d.trim().is_empty() || d.len() > 16 * 1024 || d.contains('\0') {
            return Err(LibvirtError::Invalid(
                "user_data must be 1 byte to 16 KiB of text without NUL bytes".into(),
            ));
        }
    }
    let scratch = SeedScratch::create(hostname)?;
    let tmp_dir = scratch.0.clone();

    // meta-data (escape user-provided hostname to prevent YAML injection)
    let meta_data = super::cloud_init::nocloud_meta_data(hostname, hostname);
    std::fs::write(tmp_dir.join("meta-data"), &meta_data)
        .map_err(|e| LibvirtError::Operation(format!("Failed to write meta-data: {e}")))?;

    let guest_default = super::guest_agent_provision::guest_agent_enabled(libvirt_cfg);
    let mut effective_user = username.to_string();
    if effective_user.is_empty() && guest_default {
        effective_user = super::guest_agent_provision::DEFAULT_CLOUD_INIT_USER.to_string();
    }

    // user-data (escape all user-provided values to prevent YAML injection)
    let mut user_data = String::from("#cloud-config\n");
    if !effective_user.is_empty() {
        user_data.push_str(&format!(
            "users:\n  - name: {}\n    sudo: ALL=(ALL) NOPASSWD:ALL\n    shell: /bin/bash\n",
            yaml_escape(&effective_user),
        ));
        if !password.is_empty() {
            user_data.push_str(&format!(
                "    lock_passwd: false\n    plain_text_passwd: {}\n",
                yaml_escape(password),
            ));
        }
        if !ssh_key.is_empty() {
            user_data.push_str(&format!(
                "    ssh_authorized_keys:\n      - {}\n",
                yaml_escape(ssh_key),
            ));
        }
    }
    if !password.is_empty() {
        user_data.push_str("ssh_pwauth: true\n");
    }
    if guest_default {
        super::guest_agent_provision::append_guestkit_cloud_config(&mut user_data, true);
    }
    if super::guest_agent_provision::vm_wants_graphical_desktop(hostname) {
        super::guest_agent_provision::append_desktop_graphical_cloud_config(
            &mut user_data,
            &effective_user,
        );
    }

    match user_data_override {
        Some(custom) => {
            std::fs::write(tmp_dir.join("user-data"), custom)
                .map_err(|e| LibvirtError::Operation(format!("Failed to write user-data: {e}")))?;
            std::fs::write(tmp_dir.join("vendor-data"), &user_data)
                .map_err(|e| LibvirtError::Operation(format!("Failed to write vendor-data: {e}")))?;
        }
        None => {
            std::fs::write(tmp_dir.join("user-data"), &user_data)
                .map_err(|e| LibvirtError::Operation(format!("Failed to write user-data: {e}")))?;
        }
    }
    super::guest_agent_provision::stage_guestkit_seed_files(&tmp_dir, libvirt_cfg, None)?;

    // Generate ISO (try genisoimage, then mkisofs, then xorriso)
    let iso_path = if output_path.is_empty() {
        format!(
            "{}/{}-cloud-init.iso",
            default_images_dir.trim_end_matches('/'),
            hostname
        )
    } else {
        output_path.to_string()
    };

    let cmds = [
        (
            "genisoimage",
            vec![
                "-output",
                &iso_path,
                "-V",
                "cidata",
                "-r",
                "-J",
                tmp_dir.to_str().unwrap_or("/tmp/machina-cloud-init"),
            ],
        ),
        (
            "mkisofs",
            vec![
                "-output",
                &iso_path,
                "-V",
                "cidata",
                "-r",
                "-J",
                tmp_dir.to_str().unwrap_or("/tmp/machina-cloud-init"),
            ],
        ),
    ];

    let mut success = false;
    for (cmd, args) in &cmds {
        if let Ok(output) = Command::new(cmd).args(args).output() {
            if output.status.success() {
                success = true;
                break;
            }
        }
    }

    // `scratch` removes the directory when it drops, on every path out of this function.
    drop(scratch);

    if !success {
        return Err(LibvirtError::Operation(
            "Failed to create cloud-init ISO. Install genisoimage or mkisofs.".to_string(),
        ));
    }

    Ok(iso_path)
}

// ── VM Import ──────────────────────────────────────────────────────

/// Import a disk image by converting it to qcow2 if needed. Destination directory follows the primary libvirt pool.
/// qemu-img's name for a disk file extension: VHD is `vpc` and a bare `.img` is raw, neither is accepted as `-f vhd` / `-f img`
/// (the import used to fail with "Unknown driver").
fn qemu_input_format(ext: &str) -> &str {
    match ext {
        "vhd" | "vpc" => "vpc",
        "img" => "raw",
        other => other,
    }
}

pub fn import_disk_image(
    conn: &Connect,
    source: &str,
    dest_name: &str,
) -> Result<String, LibvirtError> {
    let source_path = Path::new(source);
    if !source_path.is_absolute() {
        return Err(LibvirtError::Invalid(
            "Source path must be absolute".to_string(),
        ));
    }
    let source_path = source_path
        .canonicalize()
        .map_err(|e| LibvirtError::Invalid(format!("Cannot resolve source path: {e}")))?;
    if !source_path.is_file() {
        return Err(LibvirtError::Operation(format!(
            "Source file not found: {}",
            source_path.display()
        )));
    }

    crate::validate::validate_name(dest_name)?;

    let ext = source_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let base = storage::primary_vm_disk_base_dir(conn)
        .unwrap_or_else(|| "/var/lib/libvirt/images".to_string());
    let dest_path = format!("{}/{}.qcow2", base.trim_end_matches('/'), dest_name);

    if Path::new(&dest_path).exists() {
        return Err(LibvirtError::Operation(format!(
            "Destination already exists: {dest_path}"
        )));
    }

    match ext.as_str() {
        "qcow2" => {
            // Already qcow2, just copy
            std::fs::copy(&source_path, &dest_path)
                .map_err(|e| LibvirtError::Operation(format!("Copy failed: {e}")))?;
        }
        "vmdk" | "vdi" | "raw" | "img" | "vpc" | "vhd" => {
            // Convert with qemu-img
            let source_str = source_path.to_string_lossy();
            let output = Command::new("qemu-img")
                .args([
                    "convert",
                    "-f",
                    qemu_input_format(&ext),
                    "-O",
                    "qcow2",
                    &*source_str,
                    &dest_path,
                ])
                .output()
                .map_err(LibvirtError::map_op("qemu-img convert"))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(LibvirtError::Operation(format!(
                    "qemu-img convert failed: {stderr}"
                )));
            }
        }
        _ => {
            return Err(LibvirtError::Invalid(format!("Unsupported format: {ext}")));
        }
    }

    Ok(dest_path)
}

// ── Live Resize ────────────────────────────────────────────────────

/// Hot-add vCPUs to a running VM.
pub fn live_set_vcpus(conn: &Connect, name: &str, vcpus: u32) -> Result<(), LibvirtError> {
    crate::validate::validate_vcpus(vcpus)?;
    let domain = lookup_domain(conn, name)?;

    // Set both live and config
    domain
        .set_vcpus_flags(
            vcpus,
            virt::sys::VIR_DOMAIN_AFFECT_LIVE | virt::sys::VIR_DOMAIN_AFFECT_CONFIG,
        )
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("greater than max allowable") {
                LibvirtError::Operation(format!(
                    "Failed to live-set vCPUs for '{name}': {e}. \
                     The running domain max is too low — raise max (Edit CPU / topology) and reboot, \
                     or shut down and set vCPUs offline."
                ))
            } else {
                LibvirtError::Operation(format!("Failed to live-set vCPUs for '{name}': {e}"))
            }
        })?;
    Ok(())
}

/// Hot-set memory on a running VM (requires balloon driver).
pub fn live_set_memory(
    conn: &Connect,
    name: &str,
    memory_mb: u64,
) -> Result<MemoryApplyOutcome, LibvirtError> {
    crate::validate::validate_memory_mb(memory_mb)?;
    let domain = lookup_domain(conn, name)?;
    let kb = memory_mb * 1024;

    domain
        .set_memory_flags(kb, virt::sys::VIR_DOMAIN_AFFECT_LIVE)
        .map_err(|e| {
            LibvirtError::Operation(format!("Failed to live-set memory for '{name}': {e}"))
        })?;
    // Accepting the target isn't the same as the guest's balloon driver reaching it — see
    // resize::set_memory's doc comment for the full explanation.
    let live_applied = poll_until(BALLOON_WAIT, || memory_near_target(&domain, kb));
    Ok(MemoryApplyOutcome { live_applied })
}

// ── VM Tags ───────────────────────────────────────────────────────

const TAGS_FILE: &str = "/var/lib/machina/tags.json";

/// Tag map: vm_name -> list of tags.
pub type TagMap = HashMap<String, Vec<String>>;

/// Load tags from the JSON file. Returns empty map if file doesn't exist.
pub fn load_tags() -> TagMap {
    match std::fs::read_to_string(TAGS_FILE) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

/// Save tags to the JSON file.
pub fn save_tags(tags: &TagMap) -> Result<(), LibvirtError> {
    let dir = Path::new(TAGS_FILE)
        .parent()
        .unwrap_or(Path::new("/var/lib/machina"));
    let _ = std::fs::create_dir_all(dir);
    let data = serde_json::to_string_pretty(tags)
        .map_err(|e| LibvirtError::Operation(format!("Failed to serialize tags: {e}")))?;
    std::fs::write(TAGS_FILE, data)
        .map_err(|e| LibvirtError::Operation(format!("Failed to write tags file: {e}")))?;
    Ok(())
}

/// Set tags for a specific VM (replaces existing tags).
pub fn set_vm_tags(vm_name: &str, tags: Vec<String>) -> Result<(), LibvirtError> {
    with_json_lock(|| {
        let mut map = load_tags();
        if tags.is_empty() {
            map.remove(vm_name);
        } else {
            map.insert(vm_name.to_string(), tags);
        }
        save_tags(&map)
    })
}

/// Get tags for a specific VM.
pub fn get_vm_tags(vm_name: &str) -> Vec<String> {
    let map = load_tags();
    map.get(vm_name).cloned().unwrap_or_default()
}

// ── VM Labels (key/value, selected by VM network policies) ────────

const LABELS_FILE: &str = "/var/lib/machina/vm-labels.json";

pub type LabelMap = HashMap<String, std::collections::BTreeMap<String, String>>;

/// Labels per VM. VMs without labels fall back to their `key=value` tags.
pub fn load_labels() -> LabelMap {
    let mut map: LabelMap = std::fs::read_to_string(LABELS_FILE)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or_default();
    for (vm, tags) in load_tags() {
        map.entry(vm).or_insert_with(|| tags_as_labels(&tags));
    }
    map.retain(|_, l| !l.is_empty());
    map
}

/// `key=value` tags as labels (other tags are ignored).
pub fn tags_as_labels(tags: &[String]) -> std::collections::BTreeMap<String, String> {
    tags.iter()
        .filter_map(|t| t.split_once('='))
        .filter(|(k, _)| !k.trim().is_empty())
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

/// Label keys: optional `prefix/` + name (alphanumerics, `-`, `_`, `.`), values
/// up to 63 of the same characters (Kubernetes label syntax).
pub fn validate_labels(
    labels: &std::collections::BTreeMap<String, String>,
) -> Result<(), LibvirtError> {
    let part = |s: &str, max: usize| {
        !s.is_empty()
            && s.len() <= max
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
    };
    for (k, v) in labels {
        let name = match k.split_once('/') {
            Some((prefix, name)) if part(prefix, 253) => name,
            Some(_) => {
                return Err(LibvirtError::Invalid(format!(
                    "label key `{k}`: bad prefix"
                )))
            }
            None => k.as_str(),
        };
        if !part(name, 63) {
            return Err(LibvirtError::Invalid(format!(
                "label key `{k}` is not a valid label name"
            )));
        }
        if !v.is_empty() && !part(v, 63) {
            return Err(LibvirtError::Invalid(format!(
                "label `{k}`: value `{v}` is not a valid label value"
            )));
        }
    }
    Ok(())
}

pub fn get_vm_labels(vm_name: &str) -> std::collections::BTreeMap<String, String> {
    load_labels().remove(vm_name).unwrap_or_default()
}

/// Replace a VM's labels (empty = remove).
pub fn set_vm_labels(
    vm_name: &str,
    labels: std::collections::BTreeMap<String, String>,
) -> Result<(), LibvirtError> {
    validate_labels(&labels)?;
    with_json_lock(|| {
        let mut map: LabelMap = std::fs::read_to_string(LABELS_FILE)
            .ok()
            .and_then(|d| serde_json::from_str(&d).ok())
            .unwrap_or_default();
        if labels.is_empty() {
            map.remove(vm_name);
        } else {
            map.insert(vm_name.to_string(), labels);
        }
        let dir = Path::new(LABELS_FILE)
            .parent()
            .unwrap_or(Path::new("/var/lib/machina"));
        let _ = std::fs::create_dir_all(dir);
        let data = serde_json::to_string_pretty(&map)
            .map_err(|e| LibvirtError::Operation(format!("Failed to serialize labels: {e}")))?;
        std::fs::write(LABELS_FILE, data)
            .map_err(|e| LibvirtError::Operation(format!("Failed to write labels file: {e}")))
    })
}

// ── Host System Stats ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostStats {
    pub cpu_percent: f64,
    pub memory_total_mb: u64,
    pub memory_used_mb: u64,
    pub memory_percent: f64,
    pub swap_total_mb: u64,
    pub swap_used_mb: u64,
    pub disk_total_gb: f64,
    pub disk_used_gb: f64,
    pub disk_percent: f64,
    pub load_1: f64,
    pub load_5: f64,
    pub load_15: f64,
    pub uptime_secs: u64,
    pub processes: u32,
}

pub fn get_host_stats() -> HostStats {
    let cpu_percent = parse_cpu_percent();
    let (mem_total, mem_used, swap_total, swap_used) = parse_meminfo();
    let (disk_total, disk_used) = parse_disk_usage("/");
    let (l1, l5, l15) = parse_loadavg();
    let uptime = parse_uptime();
    let procs = std::fs::read_dir("/proc")
        .map(|d| {
            d.filter(|e| {
                e.as_ref()
                    .ok()
                    .and_then(|e| {
                        e.file_name()
                            .to_str()
                            .map(|s| s.chars().all(|c| c.is_ascii_digit()))
                    })
                    .unwrap_or(false)
            })
            .count() as u32
        })
        .unwrap_or(0);

    let mem_pct = if mem_total > 0 {
        (mem_used as f64 / mem_total as f64 * 100.0).min(100.0)
    } else {
        0.0
    };
    let disk_pct = if disk_total > 0.0 {
        (disk_used / disk_total * 100.0).min(100.0)
    } else {
        0.0
    };

    HostStats {
        cpu_percent,
        memory_total_mb: mem_total,
        memory_used_mb: mem_used,
        memory_percent: mem_pct,
        swap_total_mb: swap_total,
        swap_used_mb: swap_used,
        disk_total_gb: disk_total,
        disk_used_gb: disk_used,
        disk_percent: disk_pct,
        load_1: l1,
        load_5: l5,
        load_15: l15,
        uptime_secs: uptime,
        processes: procs,
    }
}

// ── Host filesystems & processes (Cockpit-style overview) ─────────

/// One mounted filesystem row from `df` (block-backed mounts only; virtual fs types skipped).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostFilesystem {
    pub source: String,
    pub fstype: String,
    pub mount_point: String,
    pub size_bytes: u64,
    pub used_bytes: u64,
    pub avail_bytes: u64,
    pub use_percent: f64,
}

/// One process row sorted by RSS (resident memory).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostProcess {
    pub pid: u32,
    pub user: String,
    pub cpu_percent: f64,
    pub rss_kb: u64,
    /// Short kernel thread / process name from `ps` (`comm`).
    pub command: String,
    /// Full command line from `/proc/pid/cmdline` when readable (Linux).
    #[serde(default)]
    pub args: String,
}

#[cfg(target_os = "linux")]
const SKIP_HOST_FS_TYPES: &[&str] = &[
    "proc",
    "sysfs",
    "devtmpfs",
    "tmpfs",
    "cgroup",
    "cgroup2",
    "configfs",
    "tracefs",
    "securityfs",
    "bpf",
    "pstore",
    "mqueue",
    "hugetlbfs",
    "fusectl",
    "autofs",
    "binfmt_misc",
    "debugfs",
];

/// Per-mount disk usage (GNU `df -B1 -T`). Non-Linux returns an empty list.
pub fn list_host_filesystems() -> Result<Vec<HostFilesystem>, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        Ok(Vec::new())
    }
    #[cfg(target_os = "linux")]
    {
        list_host_filesystems_linux()
    }
}

#[cfg(target_os = "linux")]
fn list_host_filesystems_linux() -> Result<Vec<HostFilesystem>, LibvirtError> {
    let output = Command::new("df")
        .args(["-B1", "-T"])
        .output()
        .map_err(|e| LibvirtError::Operation(format!("df failed: {e}")))?;
    // df exits non-zero when *any* single mount is inaccessible (e.g. a stale NFS
    // or a broken guestfs FUSE mount: "Transport endpoint is not connected") yet
    // still prints valid rows for every other filesystem. Parse whatever stdout we
    // got instead of failing the whole listing; only error when there is nothing.
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Err(LibvirtError::Operation(format!(
            "df failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    parse_df_bt_output(&stdout)
}

#[cfg(target_os = "linux")]
fn parse_df_bt_output(stdout: &str) -> Result<Vec<HostFilesystem>, LibvirtError> {
    let lines: Vec<&str> = stdout.lines().collect();
    if lines.len() < 2 {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for line in lines.iter().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 7 {
            continue;
        }
        let fstype = parts[1];
        if SKIP_HOST_FS_TYPES.contains(&fstype) {
            continue;
        }
        let size_b: u64 = parts[2].parse().unwrap_or(0);
        let used_b: u64 = parts[3].parse().unwrap_or(0);
        let avail_b: u64 = parts[4].parse().unwrap_or(0);
        let pcent = parts[5].trim_end_matches('%').parse::<f64>().unwrap_or(0.0);
        let mount_point = parts[6..].join(" ");
        if mount_point.is_empty() {
            continue;
        }
        out.push(HostFilesystem {
            source: parts[0].to_string(),
            fstype: fstype.to_string(),
            mount_point,
            size_bytes: size_b,
            used_bytes: used_b,
            avail_bytes: avail_b,
            use_percent: pcent.min(100.0),
        });
    }
    Ok(out)
}

/// How to order rows for [`list_host_top_processes`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HostTopProcessOrder {
    /// Resident set size (highest memory first).
    #[default]
    Rss,
    /// `ps` %CPU (highest CPU first).
    Cpu,
}

/// Top processes from `ps` (procps): by RSS or by CPU%. Non-Linux returns an empty list.
pub fn list_host_top_processes(
    limit: u32,
    order: HostTopProcessOrder,
) -> Result<Vec<HostProcess>, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (limit, order);
        Ok(Vec::new())
    }
    #[cfg(target_os = "linux")]
    {
        list_host_top_processes_linux(limit, order)
    }
}

#[cfg(target_os = "linux")]
fn read_proc_cmdline(pid: u32) -> String {
    let path = format!("/proc/{pid}/cmdline");
    std::fs::read(&path)
        .map(|b| {
            let s = String::from_utf8_lossy(&b)
                .replace('\0', " ")
                .trim()
                .to_string();
            if s.len() > 280 {
                // Truncate on a UTF-8 char boundary — from_utf8_lossy inserts 3-byte
                // replacement chars, so a fixed byte-offset slice would panic.
                let mut end = 277;
                while end > 0 && !s.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...", &s[..end])
            } else {
                s
            }
        })
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn list_host_top_processes_linux(
    limit: u32,
    order: HostTopProcessOrder,
) -> Result<Vec<HostProcess>, LibvirtError> {
    let lim = limit.clamp(1, 100) as usize;
    let sort_key: &str = match order {
        HostTopProcessOrder::Rss => "--sort=-rss",
        HostTopProcessOrder::Cpu => "--sort=-pcpu",
    };
    let output = Command::new("ps")
        .args([
            "-eo",
            "pid=,user=,pcpu=,rss=,comm=",
            sort_key,
            "--no-headers",
        ])
        .output()
        .map_err(|e| LibvirtError::Operation(format!("ps failed: {e}")))?;
    if !output.status.success() {
        return Err(LibvirtError::Operation(format!(
            "ps failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut rows = Vec::new();
    for line in text.lines() {
        if rows.len() >= lim {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(pid_s) = parts.next() else { continue };
        let Some(user) = parts.next() else { continue };
        let Some(pcpu_s) = parts.next() else { continue };
        let Some(rss_s) = parts.next() else { continue };
        let comm = parts.collect::<Vec<_>>().join(" ");
        let pid: u32 = pid_s.parse().unwrap_or(0);
        if pid == 0 {
            continue;
        }
        let cpu_percent: f64 = pcpu_s.parse().unwrap_or(0.0);
        let rss_kb: u64 = rss_s.parse().unwrap_or(0);
        let mut command = if comm.is_empty() {
            "?".to_string()
        } else {
            comm
        };
        if command.len() > 64 {
            command.truncate(61);
            command.push_str("...");
        }
        let args = read_proc_cmdline(pid);
        rows.push(HostProcess {
            pid,
            user: user.to_string(),
            cpu_percent,
            rss_kb,
            command,
            args,
        });
    }
    Ok(rows)
}

/// Send `SIGTERM` or `SIGKILL` to a host process (`signal`: `TERM`, `KILL`, optional `SIG*` prefix). Linux only.
///
/// Refuses PID ≤ 1, the current process (daemon), and unreasonably large PIDs. Runs as the daemon user (root), so
/// this can terminate processes owned by other users.
#[cfg(target_os = "linux")]
pub fn kill_host_process(pid: u32, signal: &str) -> Result<(), LibvirtError> {
    let sig = parse_kill_signal(signal)?;
    if pid <= 1 {
        return Err(LibvirtError::Operation(
            "Refusing to signal init or kernel threads (PID ≤ 1)".into(),
        ));
    }
    let own = std::process::id();
    if pid == own {
        return Err(LibvirtError::Operation(
            "Refusing to signal the machina daemon process".into(),
        ));
    }
    if pid > 4_194_304 {
        return Err(LibvirtError::Operation("PID out of range".into()));
    }
    let rc = unsafe { libc::kill(pid as libc::pid_t, sig) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        return Err(LibvirtError::Operation(format!("kill: {err}")));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn parse_kill_signal(signal: &str) -> Result<i32, LibvirtError> {
    let s = signal.trim();
    let s = s.strip_prefix("SIG").unwrap_or(s);
    match s.to_ascii_uppercase().as_str() {
        "TERM" => Ok(libc::SIGTERM),
        "KILL" => Ok(libc::SIGKILL),
        _ => Err(LibvirtError::Operation(
            "signal must be TERM or KILL (SIGTERM / SIGKILL accepted)".into(),
        )),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn kill_host_process(_pid: u32, _signal: &str) -> Result<(), LibvirtError> {
    Err(LibvirtError::Operation(
        "Killing host processes is only supported on Linux".into(),
    ))
}

#[cfg(all(test, target_os = "linux"))]
mod host_overview_tests {
    use super::*;

    #[test]
    fn parse_df_bt_skips_devtmpfs() {
        let s = "Filesystem     Type     1B-blocks         Used    Available Use% Mounted on\n\
/dev/vda2      ext4  499499491328 123456789012  354000000000  26% /\n\
devtmpfs       devtmpfs   4047020032            0   4047020032   0% /dev\n\
/dev/vda1      ext4      993624064    198123008    745000960  22% /boot\n";
        let v = parse_df_bt_output(s).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].mount_point, "/");
        assert_eq!(v[0].fstype, "ext4");
        assert!((v[0].use_percent - 26.0).abs() < 0.01);
        assert_eq!(v[1].mount_point, "/boot");
    }
}

fn read_cpu_jiffies() -> (u64, u64) {
    let stat = std::fs::read_to_string("/proc/stat").unwrap_or_default();
    let line = stat.lines().next().unwrap_or("");
    let vals: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();
    if vals.len() >= 4 {
        let total: u64 = vals.iter().sum();
        let idle = vals[3];
        (total, idle)
    } else {
        (0, 0)
    }
}

static LAST_CPU_SAMPLE: std::sync::Mutex<Option<(std::time::Instant, u64, u64)>> =
    std::sync::Mutex::new(None);

fn parse_cpu_percent() -> f64 {
    let (total, idle) = read_cpu_jiffies();
    if total == 0 {
        return 0.0;
    }
    let now = std::time::Instant::now();
    let mut guard = LAST_CPU_SAMPLE.lock().unwrap_or_else(|e| e.into_inner());
    let pct = if let Some((t0, tot0, idle0)) = guard.as_ref() {
        let dt = now.duration_since(*t0).as_secs_f64();
        if dt >= 0.05 {
            let dtotal = total.saturating_sub(*tot0) as f64;
            let didle = idle.saturating_sub(*idle0) as f64;
            if dtotal > 0.0 {
                ((dtotal - didle) / dtotal * 100.0).min(100.0)
            } else {
                0.0
            }
        } else {
            0.0
        }
    } else {
        0.0
    };
    *guard = Some((now, total, idle));
    pct
}

fn parse_meminfo() -> (u64, u64, u64, u64) {
    let content = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mut total: u64 = 0;
    let mut available: u64 = 0;
    let mut swap_total: u64 = 0;
    let mut swap_free: u64 = 0;
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let val: u64 = parts[1].parse().unwrap_or(0);
            match parts[0] {
                "MemTotal:" => total = val / 1024,
                "MemAvailable:" => available = val / 1024,
                "SwapTotal:" => swap_total = val / 1024,
                "SwapFree:" => swap_free = val / 1024,
                _ => {}
            }
        }
    }
    (
        total,
        total.saturating_sub(available),
        swap_total,
        swap_total.saturating_sub(swap_free),
    )
}

fn parse_disk_usage(path: &str) -> (f64, f64) {
    let output = Command::new("df").args(["-BG", path]).output().ok();
    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if let Some(line) = stdout.lines().nth(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let total: f64 = parts[1].trim_end_matches('G').parse().unwrap_or(0.0);
                let used: f64 = parts[2].trim_end_matches('G').parse().unwrap_or(0.0);
                return (total, used);
            }
        }
    }
    (0.0, 0.0)
}

fn parse_loadavg() -> (f64, f64, f64) {
    let content = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let parts: Vec<f64> = content
        .split_whitespace()
        .take(3)
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() >= 3 {
        (parts[0], parts[1], parts[2])
    } else {
        (0.0, 0.0, 0.0)
    }
}

fn parse_uptime() -> u64 {
    let content = std::fs::read_to_string("/proc/uptime").unwrap_or_default();
    content
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<f64>().ok())
        .map(|f| f as u64)
        .unwrap_or(0)
}

// ── Save VM as Template ──────────────────────────────────────────

/// Save a VM's configuration as a reusable template.
pub fn save_vm_as_template(
    conn: &Connect,
    vm_name: &str,
    template_name: &str,
) -> Result<(), LibvirtError> {
    crate::validate::validate_name(template_name)?;
    let domain = lookup_domain(conn, vm_name)?;
    let info = domain
        .get_info()
        .map_err(LibvirtError::map_op("Failed to get VM info"))?;
    let xml = domain
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("Failed to get domain XML"))?;
    let base_image = super::template_apply::primary_disk_path_from_xml(&xml)
        .and_then(|p| p.to_str().map(|s| s.to_string()));

    let template = serde_json::json!({
        "name": template_name,
        "description": format!("Saved from VM '{}'", vm_name),
        "vcpus": info.nr_virt_cpu,
        "memory_mb": info.memory / 1024,
        "disk_gb": 20,
        "os_variant": "generic",
        "base_image": base_image,
        "template_disk_mode": "backing",
    });

    let templates_dir = "/var/lib/machina/templates";
    let _ = std::fs::create_dir_all(templates_dir);
    let path = format!("{}/{}.json", templates_dir, template_name);
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&template).unwrap_or_default(),
    )
    .map_err(|e| LibvirtError::Operation(format!("Failed to save template: {e}")))?;

    Ok(())
}

/// Save a golden-image JSON template under `/var/lib/machina/templates/{name}.json`.
pub fn write_saved_template(template: &crate::VmTemplate) -> Result<(), LibvirtError> {
    crate::validate::validate_name(&template.name)?;
    let templates_dir = "/var/lib/machina/templates";
    let _ = std::fs::create_dir_all(templates_dir);
    let path = format!("{}/{}.json", templates_dir, template.name);
    let body = serde_json::to_string_pretty(template)
        .map_err(|e| LibvirtError::Operation(format!("serialize template: {e}")))?;
    std::fs::write(&path, body)
        .map_err(|e| LibvirtError::Operation(format!("Failed to save template: {e}")))?;
    Ok(())
}

/// After a dockur Golden Forge build: copy qcow2 to the marketplace path and register a saved template.
/// Stable disk: `/var/lib/libvirt/images/{win10|win11}.qcow2` (matches controller catalog `source_disk`).
pub fn register_dockur_windows_golden(
    guest: &str,
    artifact_qcow2: &std::path::Path,
) -> Result<std::path::PathBuf, LibvirtError> {
    if !matches!(
        guest,
        "win10" | "win11" | "windows-server-2022" | "windows-server-2025"
    ) {
        return Err(LibvirtError::Invalid(format!(
            "dockur golden guest must be win10, win11, windows-server-2022, or windows-server-2025, got {guest}"
        )));
    }
    if !artifact_qcow2.is_file() {
        return Err(LibvirtError::Invalid(format!(
            "golden artifact missing: {}",
            artifact_qcow2.display()
        )));
    }
    let images_dir = std::path::Path::new("/var/lib/libvirt/images");
    let _ = std::fs::create_dir_all(images_dir);
    let stable = images_dir.join(format!("{guest}.qcow2"));
    std::fs::copy(artifact_qcow2, &stable).map_err(|e| {
        LibvirtError::Operation(format!("copy golden to {}: {e}", stable.display()))
    })?;

    let (label, os_variant, disk_gb) = match guest {
        "win10" => ("Windows 10 (dockur Golden Forge)", "win10", 64u64),
        "windows-server-2022" => (
            "Windows Server 2022 (dockur Golden Forge)",
            "win2k22",
            80u64,
        ),
        "windows-server-2025" => (
            "Windows Server 2025 (dockur Golden Forge)",
            "win2k25",
            80u64,
        ),
        _ => ("Windows 11 (dockur Golden Forge)", "win11", 64u64),
    };
    let tmpl = crate::VmTemplate {
        name: guest.to_string(),
        description: format!(
            "{label} — UEFI + VirtIO; clone via Create VM or export KubeVirt YAML from Disk Images"
        ),
        vcpus: 4,
        memory_mb: 8192,
        disk_gb,
        os_variant: os_variant.to_string(),
        base_image: Some(stable.to_string_lossy().to_string()),
        template_disk_mode: "copy".to_string(),
    };
    write_saved_template(&tmpl)?;
    Ok(stable)
}

/// List all saved templates from /var/lib/machina/templates/.
pub fn list_saved_templates() -> Vec<crate::VmTemplate> {
    let templates_dir = "/var/lib/machina/templates";
    let mut templates = Vec::new();
    let dir = match std::fs::read_dir(templates_dir) {
        Ok(d) => d,
        Err(_) => return templates,
    };
    for entry in dir.flatten() {
        if entry.path().extension().is_some_and(|e| e == "json") {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(tmpl) = serde_json::from_str::<crate::VmTemplate>(&content) {
                    templates.push(tmpl);
                }
            }
        }
    }
    templates
}

// ── DHCP Leases ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhcpLease {
    pub network: String,
    pub mac: String,
    pub ip: String,
    pub hostname: String,
    pub expiry: String,
}

/// Get DHCP leases from all active libvirt networks via virsh.
pub fn list_dhcp_leases(conn: &Connect) -> Result<Vec<DhcpLease>, LibvirtError> {
    let networks = conn
        .list_all_networks(0)
        .map_err(LibvirtError::map_op("Failed to list networks"))?;

    let mut leases = Vec::new();
    for net in networks {
        let net_name = net.get_name().unwrap_or_default();
        if !net.is_active().unwrap_or(false) {
            continue;
        }

        // Use virsh net-dhcp-leases to get lease info
        if let Ok(output) = Command::new("virsh")
            .args(["net-dhcp-leases", &net_name])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().skip(2) {
                // Format: Expiry  MAC  Protocol  IP  Hostname  ClientID
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 && parts[0] != "-" {
                    // Expiry is "YYYY-MM-DD HH:MM:SS" (2 columns) or "-"
                    let (expiry, rest) = if parts[0].contains('-') && parts.len() >= 6 {
                        (format!("{} {}", parts[0], parts[1]), &parts[2..])
                    } else {
                        (parts[0].to_string(), &parts[1..])
                    };
                    if rest.len() >= 4 {
                        leases.push(DhcpLease {
                            network: net_name.clone(),
                            mac: rest[0].to_string(),
                            ip: rest[2].to_string(),
                            hostname: if rest.len() > 3 && rest[3] != "-" {
                                rest[3].to_string()
                            } else {
                                String::new()
                            },
                            expiry,
                        });
                    }
                }
            }
        }
    }
    Ok(leases)
}

// ── IOMMU Groups ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IommuDevice {
    pub bdf: String, // e.g. "0000:01:00.0"
    pub vendor: String,
    pub device_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IommuGroup {
    pub group_id: u32,
    pub devices: Vec<IommuDevice>,
}

/// Read /sys/kernel/iommu_groups/*/devices/* and return IOMMU groups with PCI device info.
pub fn list_iommu_groups() -> Result<Vec<IommuGroup>, LibvirtError> {
    let iommu_base = Path::new("/sys/kernel/iommu_groups");
    if !iommu_base.is_dir() {
        return Ok(Vec::new());
    }

    let mut groups = Vec::new();
    let mut group_dirs: Vec<_> = std::fs::read_dir(iommu_base)
        .map_err(|e| LibvirtError::Operation(format!("Failed to read IOMMU groups: {e}")))?
        .flatten()
        .collect();

    // Sort by group number
    group_dirs.sort_by_key(|e| {
        e.file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(u32::MAX)
    });

    for entry in &group_dirs {
        let group_id = match entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        {
            Some(id) => id,
            None => continue,
        };

        let devices_dir = entry.path().join("devices");
        if !devices_dir.is_dir() {
            continue;
        }

        let mut devices = Vec::new();
        if let Ok(dev_entries) = std::fs::read_dir(&devices_dir) {
            for dev_entry in dev_entries.flatten() {
                let bdf = dev_entry.file_name().to_string_lossy().to_string();

                // Try to read vendor/device description via lspci -s BDF -mm
                let (vendor, device_name) =
                    match Command::new("lspci").args(["-s", &bdf, "-mm"]).output() {
                        Ok(output) => {
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            parse_lspci_mm_line(&stdout)
                        }
                        Err(_) => (String::new(), String::new()),
                    };

                devices.push(IommuDevice {
                    bdf,
                    vendor,
                    device_name,
                });
            }
        }

        devices.sort_by(|a, b| a.bdf.cmp(&b.bdf));

        groups.push(IommuGroup { group_id, devices });
    }

    Ok(groups)
}

/// Parse a single line of `lspci -s BDF -mm` output to extract vendor and device name.
fn parse_lspci_mm_line(line: &str) -> (String, String) {
    // Format: Slot\tClass\tVendor\tDevice\t...
    // Fields are quoted with double-quotes
    let fields: Vec<&str> = line.trim().split('\t').collect();
    // lspci -mm outputs: Slot  Class  Vendor  Device  SVendor  SDevice  PhySlot  Rev  ProgIf
    if fields.len() >= 4 {
        let vendor = fields[2].trim_matches('"').to_string();
        let device = fields[3].trim_matches('"').to_string();
        (vendor, device)
    } else {
        (String::new(), String::new())
    }
}

// ── Systemd Service Manager ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemdService {
    pub name: String,
    pub description: String,
    pub active_state: String,
    pub sub_state: String,
    pub enabled: String,
}

/// List all systemd services via `systemctl list-units --type=service`.
pub fn list_services() -> Result<Vec<SystemdService>, LibvirtError> {
    let output = Command::new("systemctl")
        .args([
            "list-units",
            "--type=service",
            "--all",
            "--no-pager",
            "--plain",
            "--output=json",
        ])
        .output()
        .map_err(LibvirtError::map_op("Failed to run systemctl list-units"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Try JSON parse first (systemd 252+)
    if let Ok(units) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
        let mut services: Vec<SystemdService> = units
            .iter()
            .filter_map(|u| {
                let name = u.get("unit")?.as_str()?.to_string();
                Some(SystemdService {
                    name: name.clone(),
                    description: u
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    active_state: u
                        .get("active")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    sub_state: u
                        .get("sub")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    enabled: String::new(), // filled below
                })
            })
            .collect();

        // Batch-fetch enabled states
        fill_enabled_states(&mut services);
        return Ok(services);
    }

    // Fallback: parse plain text output
    let plain_output = Command::new("systemctl")
        .args([
            "list-units",
            "--type=service",
            "--all",
            "--no-pager",
            "--plain",
        ])
        .output()
        .map_err(LibvirtError::map_op(
            "Failed to run systemctl list-units (plain)",
        ))?;

    let plain_stdout = String::from_utf8_lossy(&plain_output.stdout);
    let mut services = Vec::new();
    for line in plain_stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 && parts[0].ends_with(".service") {
            services.push(SystemdService {
                name: parts[0].to_string(),
                description: parts[4..].join(" "),
                active_state: parts[2].to_string(),
                sub_state: parts[3].to_string(),
                enabled: String::new(),
            });
        }
    }
    fill_enabled_states(&mut services);
    Ok(services)
}

fn fill_enabled_states(services: &mut [SystemdService]) {
    // Collect all service names and query enabled state in batch
    let names: Vec<String> = services.iter().map(|s| s.name.clone()).collect();
    for chunk in names.chunks(50) {
        let mut args = vec!["is-enabled", "--no-pager"];
        let chunk_owned: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
        args.extend(chunk_owned.iter());
        if let Ok(output) = Command::new("systemctl").args(&args).output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for (i, line) in stdout.lines().enumerate() {
                let global_idx = names.iter().position(|n| chunk.get(i) == Some(n));
                if let Some(idx) = global_idx {
                    services[idx].enabled = line.trim().to_string();
                }
            }
        }
    }
}

/// Validate a systemd service name (alphanumeric, dash, underscore, dot, @).
fn validate_service_name(name: &str) -> Result<(), LibvirtError> {
    if name.is_empty() || name.len() > 256 {
        return Err(LibvirtError::Invalid(
            "Service name too short or too long".to_string(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '@')
    {
        return Err(LibvirtError::Invalid(
            "Service name contains invalid characters".to_string(),
        ));
    }
    // service_action() passes this as the last bare argv element to `systemctl
    // <action> <name>` with no `--` separator — same flag-injection class as
    // validate_login_username/validate_package_token (e.g. a name of "-H" would
    // be parsed by systemctl as an option, not a unit name).
    if name.starts_with('-') {
        return Err(LibvirtError::Invalid(
            "Service name must not start with '-'".to_string(),
        ));
    }
    Ok(())
}

/// Perform a systemctl action (start/stop/restart/enable/disable/enable_now) on a service.
/// `enable_now` runs `systemctl enable --now` (enable at boot and start now).
pub fn service_action(name: &str, action: &str) -> Result<(), LibvirtError> {
    validate_service_name(name)?;

    let valid_actions = [
        "start",
        "stop",
        "restart",
        "enable",
        "disable",
        "enable_now",
    ];
    if !valid_actions.contains(&action) {
        return Err(LibvirtError::Invalid(format!(
            "Invalid action: {action}. Must be one of: start, stop, restart, enable, disable, enable_now"
        )));
    }

    let output = if action == "enable_now" {
        Command::new("systemctl")
            .args(["enable", "--now", name])
            .output()
    } else {
        Command::new("systemctl").args([action, name]).output()
    }
    .map_err(LibvirtError::map_op("Failed to run systemctl"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let label = if action == "enable_now" {
            "enable --now"
        } else {
            action
        };
        return Err(LibvirtError::Operation(format!(
            "systemctl {label} {name} failed: {stderr}"
        )));
    }

    Ok(())
}

// ── System Logs (journald) ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub timestamp: String,
    pub unit: String,
    pub priority: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalBootEntry {
    pub index: i32,
    pub boot_id: String,
    pub first_entry: String,
    pub last_entry: String,
}

pub fn get_journal_boots() -> Result<Vec<JournalBootEntry>, LibvirtError> {
    let output = Command::new("journalctl")
        .args(["--no-pager", "--list-boots"])
        .output()
        .map_err(LibvirtError::map_op(
            "Failed to run journalctl --list-boots",
        ))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(LibvirtError::Operation(format!(
            "journalctl --list-boots failed: {stderr}"
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut out = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let idx = match parts[0].parse::<i32>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let boot_id = parts[1].to_string();
        let first_entry = format!("{} {}", parts[2], parts[3]);
        let last_entry = if parts.len() >= 6 {
            format!("{} {}", parts[4], parts[5])
        } else {
            String::new()
        };
        out.push(JournalBootEntry {
            index: idx,
            boot_id,
            first_entry,
            last_entry,
        });
    }
    Ok(out)
}

/// Get journal logs from journalctl.
#[allow(clippy::too_many_arguments)]
pub fn get_journal_logs(
    lines: u32,
    priority: Option<&str>,
    unit: Option<&str>,
    boot: Option<i32>,
    since: Option<&str>,
    until: Option<&str>,
    grep: Option<&str>,
    uid: Option<u32>,
    pid: Option<u32>,
    kernel_only: bool,
) -> Result<Vec<JournalEntry>, LibvirtError> {
    let lines_str = lines.min(5000).to_string();
    let mut args = vec!["--no-pager", "-n", &lines_str, "-o", "json"];

    let priority_owned;
    if let Some(p) = priority {
        // Validate priority
        let valid = [
            "emerg", "alert", "crit", "err", "warning", "notice", "info", "debug", "0", "1", "2",
            "3", "4", "5", "6", "7",
        ];
        if !valid.contains(&p) {
            return Err(LibvirtError::Invalid(format!("Invalid priority: {p}")));
        }
        priority_owned = "-p".to_string();
        args.push(&priority_owned);
        args.push(p);
    }

    let unit_owned;
    if let Some(u) = unit {
        if !u.is_empty() {
            // Validate unit name
            if !u.chars().all(|c| {
                c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '@' || c == '*'
            }) {
                return Err(LibvirtError::Invalid("Invalid unit name".to_string()));
            }
            unit_owned = "-u".to_string();
            args.push(&unit_owned);
            args.push(u);
        }
    }

    let boot_owned;
    if let Some(b) = boot {
        if b == 0 {
            args.push("-b");
        } else {
            boot_owned = b.to_string();
            args.push("-b");
            args.push(&boot_owned);
        }
    }

    let since_owned;
    if let Some(s) = since {
        if !s.trim().is_empty() {
            // Restrict to simple date/time tokens to avoid odd control chars.
            if !s.chars().all(|c| {
                c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || "-:+./,_".contains(c)
            }) {
                return Err(LibvirtError::Invalid("Invalid --since value".to_string()));
            }
            since_owned = s.trim().to_string();
            args.push("--since");
            args.push(&since_owned);
        }
    }

    let until_owned;
    if let Some(u) = until {
        if !u.trim().is_empty() {
            if !u.chars().all(|c| {
                c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || "-:+./,_".contains(c)
            }) {
                return Err(LibvirtError::Invalid("Invalid --until value".to_string()));
            }
            until_owned = u.trim().to_string();
            args.push("--until");
            args.push(&until_owned);
        }
    }

    let grep_owned;
    if let Some(g) = grep {
        if !g.trim().is_empty() {
            if g.len() > 200 {
                return Err(LibvirtError::Invalid("Search text too long".to_string()));
            }
            if !g
                .chars()
                .all(|c| c.is_ascii_graphic() || c.is_ascii_whitespace())
            {
                return Err(LibvirtError::Invalid("Invalid search text".to_string()));
            }
            grep_owned = g.trim().to_string();
            args.push("--grep");
            args.push(&grep_owned);
        }
    }

    let uid_owned;
    if let Some(u) = uid {
        uid_owned = format!("_UID={u}");
        args.push(&uid_owned);
    }

    let pid_owned;
    if let Some(p) = pid {
        pid_owned = format!("_PID={p}");
        args.push(&pid_owned);
    }

    if kernel_only {
        args.push("-k");
    }

    let output = Command::new("journalctl")
        .args(&args)
        .output()
        .map_err(LibvirtError::map_op("Failed to run journalctl"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
            let timestamp = obj
                .get("__REALTIME_TIMESTAMP")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())
                .map(|us| {
                    let secs = us / 1_000_000;
                    format_epoch_timestamp(secs)
                })
                .unwrap_or_default();

            let unit_field = obj
                .get("_SYSTEMD_UNIT")
                .or_else(|| obj.get("SYSLOG_IDENTIFIER"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let prio_val = obj
                .get("PRIORITY")
                .and_then(|v| {
                    v.as_str()
                        .and_then(|s| s.parse::<u8>().ok())
                        .or_else(|| v.as_u64().map(|n| n as u8))
                })
                .unwrap_or(6);
            let priority_str = match prio_val {
                0 => "emerg",
                1 => "alert",
                2 => "crit",
                3 => "err",
                4 => "warning",
                5 => "notice",
                6 => "info",
                7 => "debug",
                _ => "info",
            }
            .to_string();

            let message = obj
                .get("MESSAGE")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            entries.push(JournalEntry {
                timestamp,
                unit: unit_field,
                priority: priority_str,
                message,
            });
        }
    }

    Ok(entries)
}

fn format_epoch_timestamp(secs: u64) -> String {
    // Calculate date/time from epoch seconds
    // Days calculation
    let mut days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Calculate year/month/day from days since epoch (1970-01-01)
    let mut year: u64 = 1970;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let month_days = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month: u64 = 1;
    for md in &month_days {
        if days < *md as u64 {
            break;
        }
        days -= *md as u64;
        month += 1;
    }
    let day = days + 1;

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}")
}

fn is_leap_year(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

// ── Hostname / Timezone / System Info ────────────────────────────

fn try_cmd_stdout(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

/// Like `try_cmd_stdout`, but returns stdout even when exit status is non-zero (e.g. failed units list).
fn try_cmd_stdout_any_status(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

fn truncate_text(mut s: String, max_lines: usize, max_bytes: usize) -> String {
    let lines: Vec<&str> = s.lines().take(max_lines).collect();
    s = lines.join("\n");
    if s.len() > max_bytes {
        s.truncate(max_bytes);
        s.push_str("\n… [truncated]");
    }
    s
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub hostname: String,
    pub timezone: String,
    pub kernel_version: String,
    pub architecture: String,
    pub os_name: String,
    pub os_version: String,
    pub os_pretty_name: String,
    pub boot_time: String,
    pub rtc_time: String,
    pub ntp_service: String,
    pub system_clock_synchronized: bool,
    pub systemd_version: String,
    pub boot_duration: String,
    pub critical_chain_top: Vec<String>,
    pub logged_in_users: usize,
    /// Extra `hostnamectl` fields (when present).
    pub pretty_hostname: String,
    pub transient_hostname: String,
    pub icon_name: String,
    pub chassis: String,
    pub deployment: String,
    pub location: String,
    pub machine_id: String,
    pub boot_id: String,
    pub hardware_model: String,
    pub firmware_version: String,
    /// Extra `timedatectl` fields.
    pub local_time: String,
    pub universal_time: String,
    pub rtc_in_local_tz: String,
    /// Full `systemctl --version` output (truncated server-side).
    pub systemd_version_full: String,
    /// Top lines of `systemd-analyze blame`.
    pub systemd_analyze_blame_top: Vec<String>,
    /// `loginctl list-users` text (truncated).
    pub loginctl_users_text: String,
    /// `loginctl list-sessions` text (truncated).
    pub loginctl_sessions_text: String,
    /// `systemctl show` manager properties (truncated).
    pub systemctl_show_manager: String,
    /// Active mount units (truncated).
    pub mount_units_text: String,
    /// Failed units (truncated).
    pub failed_units_text: String,
    /// `systemctl list-dependencies systemd-networkd` (truncated; empty if unavailable).
    pub networkd_dependencies_text: String,
    /// Raw `hostnamectl` human output (truncated).
    pub hostnamectl_status_text: String,
    /// Raw `timedatectl` human output (truncated).
    pub timedatectl_status_text: String,
    /// `timedatectl show` (key=value, truncated).
    pub timedatectl_show_text: String,
    /// `systemctl is-system-running`.
    pub systemctl_is_system_running: String,
    /// `systemctl show-environment` (truncated).
    pub systemctl_show_environment_text: String,
    /// `systemctl list-sockets` (truncated).
    pub systemctl_list_sockets_text: String,
    /// `systemctl list-timers --all` (truncated).
    pub systemctl_list_timers_text: String,
    /// `systemctl list-jobs` (truncated).
    pub systemctl_list_jobs_text: String,
    /// `systemctl status systemd-networkd` (truncated).
    pub systemctl_status_networkd_text: String,
    /// `systemctl status systemd-resolved` (truncated).
    pub systemctl_status_resolved_text: String,
    /// `systemctl list-dependencies systemd-resolved` (truncated).
    pub resolved_dependencies_text: String,
    /// `resolvectl status` (truncated).
    pub resolvectl_status_text: String,
    /// `resolvectl statistics` (truncated).
    pub resolvectl_statistics_text: String,
    /// `bootctl status` (truncated; empty if not using systemd-boot).
    pub bootctl_status_text: String,
    /// Enabled `.service` unit files (truncated).
    pub enabled_service_unit_files_text: String,
    /// Running `.service` units (truncated).
    pub running_service_units_text: String,
    /// `loginctl list-seats` (truncated).
    pub loginctl_list_seats_text: String,
    /// `journalctl --list-boots` (truncated).
    pub journalctl_list_boots_text: String,
    /// `systemd-analyze verify` (truncated; may be non-zero exit).
    pub systemd_analyze_verify_text: String,
    /// `systemctl list-dependencies default.target` (truncated).
    pub default_target_dependencies_text: String,
    // Hardware info from DMI
    pub product_name: String,
    pub sys_vendor: String,
    pub bios_version: String,
    pub bios_date: String,
    pub board_name: String,
    pub serial_number: String,
    pub cpu_model: String,
    pub virtualization: String,
}

/// Get system info: hostname, timezone, kernel, OS.
pub fn get_system_info() -> Result<SystemInfo, LibvirtError> {
    let hostnamectl_raw = Command::new("hostnamectl")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let timedatectl_raw = Command::new("timedatectl")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let systemd_analyze_raw = Command::new("systemd-analyze")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let systemctl_version_raw = Command::new("systemctl")
        .args(["--version"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let critical_chain_raw = Command::new("systemd-analyze")
        .args(["critical-chain"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let userspace_ts = Command::new("systemctl")
        .args(["show", "--property=UserspaceTimestamp", "--value"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let loginctl_users_raw = Command::new("loginctl")
        .args(["list-users", "--no-legend"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let loginctl_sessions_raw =
        try_cmd_stdout_any_status("loginctl", &["list-sessions", "--no-legend"]);

    let systemd_version_full =
        truncate_text(try_cmd_stdout("systemctl", &["--version"]), 80, 24_576);
    let blame_raw = try_cmd_stdout_any_status("systemd-analyze", &["blame"]);
    let systemd_analyze_blame_top: Vec<String> = blame_raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(50)
        .map(|s| s.to_string())
        .collect();
    let systemctl_show_manager = truncate_text(
        try_cmd_stdout_any_status("systemctl", &["show", "--no-pager"]),
        200,
        65_536,
    );
    let mount_units_text = truncate_text(
        try_cmd_stdout_any_status(
            "systemctl",
            &[
                "list-units",
                "--type=mount",
                "--state=active",
                "--no-pager",
                "--no-legend",
            ],
        ),
        120,
        32_768,
    );
    let failed_units_text = truncate_text(
        try_cmd_stdout_any_status(
            "systemctl",
            &["list-units", "--state=failed", "--no-pager", "--no-legend"],
        ),
        80,
        16_384,
    );
    let networkd_dependencies_text = truncate_text(
        try_cmd_stdout_any_status(
            "systemctl",
            &["list-dependencies", "systemd-networkd", "--no-pager"],
        ),
        120,
        24_576,
    );
    let hostnamectl_status_text = truncate_text(hostnamectl_raw.clone(), 120, 32_768);
    let timedatectl_status_text = truncate_text(timedatectl_raw.clone(), 120, 24_576);
    let timedatectl_show_text = truncate_text(
        try_cmd_stdout_any_status("timedatectl", &["show"]),
        200,
        16_384,
    );
    let systemctl_is_system_running =
        try_cmd_stdout_any_status("systemctl", &["is-system-running"])
            .trim()
            .to_string();
    let systemctl_show_environment_text = truncate_text(
        try_cmd_stdout_any_status("systemctl", &["show-environment", "--no-pager"]),
        120,
        24_576,
    );
    let systemctl_list_sockets_text = truncate_text(
        try_cmd_stdout_any_status("systemctl", &["list-sockets", "--no-pager"]),
        100,
        32_768,
    );
    let systemctl_list_timers_text = truncate_text(
        try_cmd_stdout_any_status(
            "systemctl",
            &["list-timers", "--all", "--no-pager", "--no-legend"],
        ),
        120,
        32_768,
    );
    let systemctl_list_jobs_text = truncate_text(
        try_cmd_stdout_any_status("systemctl", &["list-jobs", "--no-pager"]),
        40,
        8_192,
    );
    let systemctl_status_networkd_text = truncate_text(
        try_cmd_stdout_any_status(
            "systemctl",
            &["status", "systemd-networkd", "--no-pager", "-l"],
        ),
        80,
        24_576,
    );
    let systemctl_status_resolved_text = truncate_text(
        try_cmd_stdout_any_status(
            "systemctl",
            &["status", "systemd-resolved", "--no-pager", "-l"],
        ),
        80,
        24_576,
    );
    let resolved_dependencies_text = truncate_text(
        try_cmd_stdout_any_status(
            "systemctl",
            &["list-dependencies", "systemd-resolved", "--no-pager"],
        ),
        120,
        24_576,
    );
    let resolvectl_status_text = truncate_text(
        try_cmd_stdout_any_status("resolvectl", &["status"]),
        150,
        48_640,
    );
    let resolvectl_statistics_text = truncate_text(
        try_cmd_stdout_any_status("resolvectl", &["statistics"]),
        80,
        16_384,
    );
    let bootctl_status_text = truncate_text(try_cmd_stdout("bootctl", &["status"]), 80, 16_384);
    let enabled_service_unit_files_text = truncate_text(
        try_cmd_stdout_any_status(
            "systemctl",
            &[
                "list-unit-files",
                "--type=service",
                "--state=enabled",
                "--no-pager",
                "--no-legend",
            ],
        ),
        150,
        48_640,
    );
    let running_service_units_text = truncate_text(
        try_cmd_stdout_any_status(
            "systemctl",
            &[
                "list-units",
                "--type=service",
                "--state=running",
                "--no-pager",
                "--no-legend",
            ],
        ),
        120,
        32_768,
    );
    let loginctl_list_seats_text = truncate_text(
        try_cmd_stdout_any_status("loginctl", &["list-seats", "--no-legend"]),
        40,
        8_192,
    );
    let journalctl_list_boots_text = truncate_text(
        try_cmd_stdout_any_status("journalctl", &["--list-boots", "--no-pager"]),
        50,
        12_288,
    );
    let systemd_analyze_verify_text = truncate_text(
        try_cmd_stdout_any_status("systemd-analyze", &["verify", "--no-pager"]),
        100,
        32_768,
    );
    let default_target_dependencies_text = truncate_text(
        try_cmd_stdout_any_status(
            "systemctl",
            &["list-dependencies", "default.target", "--no-pager"],
        ),
        120,
        24_576,
    );

    let field = |blob: &str, key: &str| -> String {
        blob.lines()
            .find_map(|line| {
                let (k, v) = line.split_once(':')?;
                if k.trim() == key {
                    Some(v.trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_default()
    };

    let hostname = field(&hostnamectl_raw, "Static hostname");
    let pretty_hostname = field(&hostnamectl_raw, "Pretty hostname");
    let transient_hostname = field(&hostnamectl_raw, "Transient hostname");
    let icon_name = field(&hostnamectl_raw, "Icon name");
    let chassis = field(&hostnamectl_raw, "Chassis");
    let deployment = field(&hostnamectl_raw, "Deployment");
    let location = field(&hostnamectl_raw, "Location");
    let machine_id = field(&hostnamectl_raw, "Machine ID");
    let boot_id = field(&hostnamectl_raw, "Boot ID");
    let hardware_model = field(&hostnamectl_raw, "Hardware Model");
    let mut firmware_version = field(&hostnamectl_raw, "Firmware Version");
    if firmware_version.is_empty() {
        firmware_version = field(&hostnamectl_raw, "BIOS Version");
    }

    let os_pretty_name = field(&hostnamectl_raw, "Operating System");
    let kernel_version = field(&hostnamectl_raw, "Kernel")
        .strip_prefix("Linux ")
        .unwrap_or(&field(&hostnamectl_raw, "Kernel"))
        .to_string();
    let architecture = field(&hostnamectl_raw, "Architecture");
    let virtualization = {
        let v = field(&hostnamectl_raw, "Virtualization");
        if v.is_empty() {
            "none".to_string()
        } else {
            v
        }
    };
    let os_name = os_pretty_name
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    let os_version = os_pretty_name
        .split_once(' ')
        .map(|(_, v)| v.to_string())
        .unwrap_or_default();

    let timezone = field(&timedatectl_raw, "Time zone")
        .split_whitespace()
        .next()
        .unwrap_or("unknown")
        .to_string();
    let boot_time = if userspace_ts.is_empty() {
        "unknown".to_string()
    } else {
        userspace_ts
    };
    let rtc_time = field(&timedatectl_raw, "RTC time");
    let ntp_service = field(&timedatectl_raw, "NTP service");
    let system_clock_synchronized = field(&timedatectl_raw, "System clock synchronized") == "yes";
    let local_time = field(&timedatectl_raw, "Local time");
    let universal_time = field(&timedatectl_raw, "Universal time");
    let rtc_in_local_tz = field(&timedatectl_raw, "RTC in local TZ");

    let boot_duration = systemd_analyze_raw
        .lines()
        .next()
        .unwrap_or("unknown")
        .trim()
        .to_string();
    let systemd_version = systemctl_version_raw
        .lines()
        .next()
        .unwrap_or("unknown")
        .trim()
        .to_string();
    let logged_in_users = loginctl_users_raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count();
    let loginctl_users_text = truncate_text(loginctl_users_raw.clone(), 40, 8_192);
    let loginctl_sessions_text = truncate_text(loginctl_sessions_raw, 80, 16_384);
    let critical_chain_top = critical_chain_raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("The time when unit became active"))
        .take(8)
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    // DMI hardware info
    let read_dmi = |name: &str| -> String {
        std::fs::read_to_string(format!("/sys/class/dmi/id/{name}"))
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let product_name = read_dmi("product_name");
    let sys_vendor = read_dmi("sys_vendor");
    let bios_version = read_dmi("bios_version");
    let bios_date = read_dmi("bios_date");
    let board_name = read_dmi("board_name");
    let serial_number = read_dmi("product_serial");

    // CPU model from /proc/cpuinfo
    let cpu_model = std::fs::read_to_string("/proc/cpuinfo")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    Ok(SystemInfo {
        hostname,
        timezone,
        kernel_version,
        architecture,
        os_name,
        os_version,
        os_pretty_name,
        boot_time,
        rtc_time,
        ntp_service,
        system_clock_synchronized,
        systemd_version,
        boot_duration,
        critical_chain_top,
        logged_in_users,
        pretty_hostname,
        transient_hostname,
        icon_name,
        chassis,
        deployment,
        location,
        machine_id,
        boot_id,
        hardware_model,
        firmware_version,
        local_time,
        universal_time,
        rtc_in_local_tz,
        systemd_version_full,
        systemd_analyze_blame_top,
        loginctl_users_text,
        loginctl_sessions_text,
        systemctl_show_manager,
        mount_units_text,
        failed_units_text,
        networkd_dependencies_text,
        hostnamectl_status_text,
        timedatectl_status_text,
        timedatectl_show_text,
        systemctl_is_system_running,
        systemctl_show_environment_text,
        systemctl_list_sockets_text,
        systemctl_list_timers_text,
        systemctl_list_jobs_text,
        systemctl_status_networkd_text,
        systemctl_status_resolved_text,
        resolved_dependencies_text,
        resolvectl_status_text,
        resolvectl_statistics_text,
        bootctl_status_text,
        enabled_service_unit_files_text,
        running_service_units_text,
        loginctl_list_seats_text,
        journalctl_list_boots_text,
        systemd_analyze_verify_text,
        default_target_dependencies_text,
        product_name,
        sys_vendor,
        bios_version,
        bios_date,
        board_name,
        serial_number,
        cpu_model,
        virtualization,
    })
}

/// Set hostname via hostnamectl.
pub fn set_hostname(name: &str) -> Result<(), LibvirtError> {
    // Validate hostname: RFC 1123
    if name.is_empty() || name.len() > 253 {
        return Err(LibvirtError::Invalid(
            "Hostname must be 1-253 characters".to_string(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '.')
    {
        return Err(LibvirtError::Invalid(
            "Hostname contains invalid characters".to_string(),
        ));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(LibvirtError::Invalid(
            "Hostname must not start or end with a dash".to_string(),
        ));
    }

    let output = Command::new("hostnamectl")
        .args(["set-hostname", name])
        .output()
        .map_err(LibvirtError::map_op("Failed to run hostnamectl"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(LibvirtError::Operation(format!(
            "hostnamectl set-hostname failed: {stderr}"
        )));
    }
    Ok(())
}

/// Set timezone via timedatectl.
pub fn set_timezone(tz: &str) -> Result<(), LibvirtError> {
    // Validate timezone: alphanumeric, slash, dash, underscore, plus
    if tz.is_empty() || tz.len() > 64 {
        return Err(LibvirtError::Invalid(
            "Timezone must be 1-64 characters".to_string(),
        ));
    }
    if !tz
        .chars()
        .all(|c| c.is_alphanumeric() || c == '/' || c == '-' || c == '_' || c == '+')
    {
        return Err(LibvirtError::Invalid(
            "Timezone contains invalid characters".to_string(),
        ));
    }
    // This is passed as a bare trailing argv element to `timedatectl set-timezone
    // <tz>` with no `--` separator, same as `set_hostname` above — reject a leading
    // '-' so it can't be parsed as a timedatectl option instead of the zone name.
    if tz.starts_with('-') {
        return Err(LibvirtError::Invalid(
            "Timezone must not start with '-'".to_string(),
        ));
    }

    let output = Command::new("timedatectl")
        .args(["set-timezone", tz])
        .output()
        .map_err(LibvirtError::map_op("Failed to run timedatectl"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(LibvirtError::Operation(format!(
            "timedatectl set-timezone failed: {stderr}"
        )));
    }
    Ok(())
}

// ── Host Shutdown/Reboot ────────────────────────────────────────

/// Shut down the host machine.
pub fn host_shutdown() -> Result<(), LibvirtError> {
    let output = Command::new("shutdown")
        .args(["-h", "now"])
        .output()
        .map_err(LibvirtError::map_op("Failed to run shutdown"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(LibvirtError::Operation(format!(
            "shutdown failed: {stderr}"
        )));
    }
    Ok(())
}

/// Reboot the host machine.
pub fn host_reboot() -> Result<(), LibvirtError> {
    let output = Command::new("shutdown")
        .args(["-r", "now"])
        .output()
        .map_err(LibvirtError::map_op("Failed to run reboot"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(LibvirtError::Operation(format!("reboot failed: {stderr}")));
    }
    Ok(())
}

#[cfg(test)]
mod seed_scratch_tests {
    use super::*;

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("machina-seed-test-{}", uuid::Uuid::new_v4().simple()))
    }

    #[test]
    fn each_seed_gets_its_own_directory_and_it_disappears_when_dropped() {
        let root = test_root();
        let a = SeedScratch::create_in(&root, "web-1").unwrap();
        let b = SeedScratch::create_in(&root, "web-1").unwrap();
        assert_ne!(a.0, b.0, "two seeds for the same name must not share a directory");
        assert!(a.0.is_dir() && b.0.is_dir());
        let (pa, pb) = (a.0.clone(), b.0.clone());
        std::fs::write(pa.join("user-data"), "secret").unwrap();
        drop(a);
        assert!(!pa.exists(), "the scratch directory (and its secret) must be removed");
        assert!(pb.is_dir(), "dropping one must not touch the other");
        drop(b);
        assert!(!pb.exists());
    }

    #[test]
    fn hostile_names_cannot_escape_the_scratch_root() {
        let root = test_root();
        let s = SeedScratch::create_in(&root, "../../etc/passwd").unwrap();
        assert!(s.0.starts_with(&root));
        assert!(!s.0.to_string_lossy().contains(".."));
    }

    #[test]
    fn unusable_user_data_is_refused_before_anything_is_written() {
        for bad in ["", "   \n", "a\0b"] {
            let err = generate_cloud_init_iso_with("", "/tmp", "vm", "u", "", "", Some(bad), None).unwrap_err();
            assert!(err.to_string().contains("user_data"), "{err}");
        }
        let big = "x".repeat(16 * 1024 + 1);
        assert!(generate_cloud_init_iso_with("", "/tmp", "vm", "u", "", "", Some(&big), None).is_err());
    }
}

#[cfg(test)]
mod import_format_tests {
    use super::qemu_input_format;

    #[test]
    fn extensions_map_to_the_names_qemu_img_accepts() {
        assert_eq!(qemu_input_format("vhd"), "vpc");
        assert_eq!(qemu_input_format("vpc"), "vpc");
        assert_eq!(qemu_input_format("img"), "raw");
        for same in ["vmdk", "vdi", "raw"] {
            assert_eq!(qemu_input_format(same), same);
        }
    }
}
