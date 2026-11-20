// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Optional [mkosi](https://github.com/systemd/mkosi) integration: build a disk image from a workspace
//! containing `mkosi.conf`, then attach it as the VM root volume (alternative to virt-builder / golden images).
//!
//! Ephemeral `--workspace-directory` under `/var/tmp` avoids mkosi errors when the default
//! `~/.cache/mkosi` would sit inside `BuildSources=` (e.g. sources under `$HOME`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use rand::Rng;
use virt::connect::Connect;

use crate::config::LibvirtConfig;
use crate::state::CreateVmRequest;
use crate::LibvirtError;

use super::subprocess::{self, VmCreateLogSink};

/// Auto-detect a mkosi workspace when the user provided no boot source.
///
/// Tries these matches in order:
///  1. Workspace name is an exact prefix of the VM name (e.g. "fedora43" for "fedora43-web")
///  2. VM name contains the workspace's distro token (e.g. "fedora" → matches "fedora43")
///
/// Returns the workspace path string if a match is found.
pub fn auto_detect_workspace(vm_name: &str) -> Option<String> {
    let name_lower = vm_name.to_lowercase();
    let workspaces = crate::libvirt::extras::list_mkosi_workspaces();
    if workspaces.is_empty() {
        return None;
    }
    // Pass 1: workspace name is a prefix of the VM name.
    for ws in &workspaces {
        if name_lower.starts_with(&ws.name.to_lowercase()) {
            return Some(ws.path.clone());
        }
    }
    // Pass 2: VM name contains the distro token extracted from workspace name
    // e.g. "fedora43" → distro token "fedora"; "ubuntu2404" → "ubuntu".
    for ws in &workspaces {
        let token: String = ws.name.chars().take_while(|c| c.is_alphabetic()).collect();
        if !token.is_empty() && name_lower.contains(&token.to_lowercase()) {
            return Some(ws.path.clone());
        }
    }
    None
}

/// If `req.mkosi_workspace` is set, run `mkosi build` in that directory and materialize the output disk.
pub fn materialize_mkosi_if_requested(
    conn: &Connect,
    req: &mut CreateVmRequest,
    cfg: &LibvirtConfig,
    log: Option<&VmCreateLogSink>,
) -> Result<(), LibvirtError> {
    let ws = req.mkosi_workspace.trim();
    if ws.is_empty() {
        return Ok(());
    }

    if !cfg.mkosi_allowed {
        return Err(LibvirtError::Invalid(
            "mkosi builds are disabled on this server ([libvirt] mkosi_allowed = false)".into(),
        ));
    }

    crate::validate::validate_mkosi_workspace(ws)?;

    if !req.existing_disk.trim().is_empty() {
        return Err(LibvirtError::Invalid(
            "mkosi_workspace cannot be used when a root disk is already set (existing_disk or golden template)"
                .into(),
        ));
    }

    if !req.virt_builder_os.trim().is_empty() {
        return Err(LibvirtError::Invalid(
            "mkosi_workspace cannot be combined with virt_builder_os".into(),
        ));
    }

    if !req.iso.trim().is_empty() {
        return Err(LibvirtError::Invalid(
            "mkosi produces a full root disk; omit install ISO (`iso`)".into(),
        ));
    }

    let workspace = Path::new(ws)
        .canonicalize()
        .map_err(|e| LibvirtError::Invalid(format!("mkosi_workspace: {e}")))?;

    let dest = super::create::find_disk_path(conn, &req.name)?;
    if Path::new(&dest).exists() {
        return Err(LibvirtError::Operation(format!(
            "Refusing to overwrite existing disk: {dest}"
        )));
    }

    let ws_str = workspace
        .to_str()
        .ok_or_else(|| LibvirtError::Invalid("mkosi_workspace path is not valid UTF-8".into()))?;

    let staging = alloc_mkosi_ephemeral_workspace(&req.name)?;
    let staging_str = staging
        .to_str()
        .ok_or_else(|| LibvirtError::Internal("mkosi staging path is not valid UTF-8".into()))?;

    // Direct all output into the staging tree so we know exactly where to find the
    // artifact and it is cleaned up with staging rather than accumulating in the workspace.
    let output_dir = staging.join("output");
    fs::create_dir(&output_dir)
        .map_err(|e| LibvirtError::Operation(format!("mkosi: cannot create output dir: {e}")))?;
    let output_dir_str = output_dir
        .to_str()
        .ok_or_else(|| LibvirtError::Internal("mkosi output dir path is not valid UTF-8".into()))?;

    let mkosi_bin = resolve_mkosi_executable();
    let image_name = req.mkosi_image.trim().to_string();
    if !image_name.is_empty() {
        // `mkosi` is a Python argparse CLI; `image_name` is passed as a bare argv
        // element right after its own `--image` flag with no `--` separator, so an
        // unvalidated value starting with '-' can be parsed as a different mkosi
        // flag instead of the image name (same flag-injection class as
        // validate_login_username/validate_service_name).
        if image_name.starts_with('-')
            || !image_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(LibvirtError::Invalid(
                "mkosi_image must start with a letter/digit and contain only letters, digits, dot, underscore, hyphen".into(),
            ));
        }
    }

    tracing::info!(
        "mkosi build --directory {} --workspace-directory {} --output-dir {}{}",
        ws_str,
        staging_str,
        output_dir_str,
        if image_name.is_empty() {
            String::new()
        } else {
            format!(" --image {image_name}")
        },
    );

    let mut cmd = Command::new(&mkosi_bin);
    cmd.args([
        "--directory",
        ws_str,
        "--workspace-directory",
        staging_str,
        "--output-dir",
        output_dir_str,
    ]);
    if !image_name.is_empty() {
        cmd.args(["--image", &image_name]);
    }
    cmd.arg("build");

    let summary = format!(
        "$ mkosi build --directory {ws_str} --workspace-directory {staging_str} --output-dir {output_dir_str}{}",
        if image_name.is_empty() {
            String::new()
        } else {
            format!(" --image {image_name}")
        }
    );
    let out = subprocess::run_command_streaming(cmd, &summary, "mkosi", log)?;

    if !out.status.success() {
        tracing::warn!(
            path = %staging.display(),
            "mkosi build failed; ephemeral workspace left for inspection (delete manually or set MACHINA_MKOSI_KEEP_WORKSPACE)"
        );
        return Err(LibvirtError::Operation(format!(
            "mkosi build failed (exit {}); see streamed log above",
            out.status
        )));
    }

    let artifact = find_mkosi_disk_artifact(&output_dir)?;
    materialize_artifact_to_dest(&artifact, Path::new(&dest), log)?;

    let keep_staging = std::env::var_os("MACHINA_MKOSI_KEEP_WORKSPACE").is_some();
    if !keep_staging {
        if let Err(e) = fs::remove_dir_all(&staging) {
            tracing::debug!(path = %staging.display(), "mkosi staging cleanup: {e}");
        }
    }

    super::guest_agent_provision::inject_guestkit_into_disk(&dest, Some(cfg), log)?;
    req.existing_disk = dest;
    Ok(())
}

/// Prefer the install.sh symlink so systemd units without `/usr/local/bin` on `PATH` still work.
fn resolve_mkosi_executable() -> PathBuf {
    for p in ["/usr/local/bin/mkosi", "/usr/bin/mkosi"] {
        let pb = Path::new(p);
        if pb.is_file() {
            return pb.to_path_buf();
        }
    }
    PathBuf::from("mkosi")
}

/// Scratch directory for mkosi's workspace (must not live under typical `BuildSources=` trees).
fn alloc_mkosi_ephemeral_workspace(vm_name: &str) -> Result<PathBuf, LibvirtError> {
    let safe: String = vm_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(48)
        .collect();
    let safe = if safe.is_empty() {
        "vm".to_string()
    } else {
        safe
    };
    let base: PathBuf = std::env::var_os("MACHINA_MKOSI_WORKSPACE_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from("/var/tmp/machina-mkosi-ws"));
    fs::create_dir_all(&base).map_err(|e| {
        LibvirtError::Operation(format!(
            "mkosi: cannot create workspace base {}: {e}",
            base.display()
        ))
    })?;
    let id: u32 = rand::thread_rng().gen();
    let dir = base.join(format!("{safe}-{id:08x}"));
    fs::create_dir(&dir).map_err(|e| {
        LibvirtError::Operation(format!(
            "mkosi: cannot create ephemeral workspace {}: {e}",
            dir.display()
        ))
    })?;
    Ok(dir)
}

/// Find the newest `.raw` or `.qcow2` artifact in the given directory.
/// We always pass `--output-dir` pointing here so the search is flat and deterministic.
fn find_mkosi_disk_artifact(output_dir: &Path) -> Result<PathBuf, LibvirtError> {
    let mut candidates: Vec<(SystemTime, PathBuf)> = Vec::new();
    collect_disk_images(output_dir, &mut candidates);

    candidates.sort_by_key(|a| a.0);
    candidates
        .into_iter()
        .next_back()
        .map(|(_, p)| p)
        .ok_or_else(|| {
            LibvirtError::Operation(format!(
                "mkosi build produced no .raw/.qcow2 in {}",
                output_dir.display()
            ))
        })
}

fn collect_disk_images(dir: &Path, out: &mut Vec<(SystemTime, PathBuf)>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let path = ent.path();
        let Ok(meta) = ent.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext != "raw" && ext != "qcow2" {
            continue;
        }
        let mt = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        out.push((mt, path));
    }
}

fn materialize_artifact_to_dest(
    artifact: &Path,
    dest: &Path,
    log: Option<&VmCreateLogSink>,
) -> Result<(), LibvirtError> {
    let ext = artifact.extension().and_then(|s| s.to_str()).unwrap_or("");

    if ext == "qcow2" {
        fs::copy(artifact, dest).map_err(|e| {
            LibvirtError::Operation(format!(
                "Failed to copy mkosi qcow2 to {}: {e}",
                dest.display()
            ))
        })?;
        return Ok(());
    }

    // .raw → qcow2 for libvirt path convention
    let a = artifact
        .to_str()
        .ok_or_else(|| LibvirtError::Invalid("mkosi artifact path is not valid UTF-8".into()))?;
    let d = dest
        .to_str()
        .ok_or_else(|| LibvirtError::Invalid("destination disk path is not valid UTF-8".into()))?;
    let summary = format!("$ qemu-img convert -O qcow2 {a} {d}");
    let mut qcmd = Command::new("qemu-img");
    qcmd.args(["convert", "-O", "qcow2", a, d]);
    let out = subprocess::run_command_streaming(qcmd, &summary, "qemu-img", log)?;

    if !out.status.success() {
        let _ = fs::remove_file(dest);
        return Err(LibvirtError::Operation(format!(
            "qemu-img convert (mkosi raw → qcow2) failed (exit {}); see streamed log",
            out.status
        )));
    }

    Ok(())
}
