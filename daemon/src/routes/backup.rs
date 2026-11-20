// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::body::Body;
use axum::extract::{Extension, Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use machina_core::LibvirtManager;
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;
use tokio_util::io::ReaderStream;

use crate::auth::{require_write, RequestActor};
use crate::error::AppError;

// ── Request types ───────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct BackupRequest {
    #[serde(default)]
    vm_name: Option<String>,
    #[serde(default)]
    with_disks: bool,
    #[serde(default)]
    incremental: bool,
    #[serde(default)]
    nfs_target: Option<String>,
    #[serde(default = "default_retain")]
    retain: u32,
    /// `fluxvm` backs up a FluxVM VM (`vm_name` required) through `fluxvm-api`.
    #[serde(default)]
    backend: Option<String>,
    #[serde(default)]
    compress: bool,
}

fn default_retain() -> u32 {
    7
}

#[derive(Deserialize)]
struct RestoreRequest {
    backup_id: String,
    #[serde(default)]
    backend: Option<String>,
    /// FluxVM: VM to restore into (stopped); defaults to the backup's own VM.
    #[serde(default)]
    vm_name: Option<String>,
}

#[derive(Deserialize, Default)]
struct BackupQuery {
    #[serde(default)]
    backend: Option<String>,
    /// FluxVM listing: only this VM's backups.
    #[serde(default)]
    vm: Option<String>,
}

impl BackupQuery {
    fn is_fluxvm(&self) -> bool {
        machina_core::fluxvm::is_fluxvm(self.backend.as_deref())
    }
}

fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n}B")
    } else {
        format!("{v:.1}{}", UNITS[i])
    }
}

/// A `fluxvm-api` backup entry in the shape of this module's backup list.
fn fluxvm_backup_item(b: &serde_json::Value) -> serde_json::Value {
    let size = b["size_bytes"].as_u64().unwrap_or(0);
    json!({
        "id": b["name"],
        "timestamp": b.get("created_at").cloned().unwrap_or_else(|| b["name"].clone()),
        "vm_filter": b.get("vm_name").cloned().unwrap_or_default(),
        "vm_count": 1,
        "net_count": 0,
        "with_disks": true,
        "nfs_target": "",
        "size": human_bytes(size),
        "size_bytes": size,
        "status": "completed",
        "status_message": "",
        "progress": "",
        "has_checksums": false,
        "backend": machina_core::fluxvm::BACKEND_NAME,
        "live": b.get("live").cloned().unwrap_or_default(),
    })
}

async fn fluxvm_backup_items(vm: Option<&str>) -> Result<Vec<serde_json::Value>, AppError> {
    let items = crate::routes::fluxvm::client()?.backups(vm).await?;
    Ok(items.iter().map(fluxvm_backup_item).collect())
}

// ── Helpers ─────────────────────────────────────────────────────────

fn backup_dir() -> PathBuf {
    let config = machina_core::MachinaConfig::load();
    if !config.backup.backup_dir.is_empty() {
        return PathBuf::from(&config.backup.backup_dir);
    }

    if let Ok(content) = std::fs::read_to_string("/etc/machina/backup.conf") {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("backup_dir") {
                let rest = rest.trim_start();
                if let Some(value) = rest.strip_prefix('=') {
                    let dir = value.trim().trim_matches('"').trim_matches('\'');
                    if !dir.is_empty() {
                        return PathBuf::from(dir);
                    }
                }
            }
        }
    }

    PathBuf::from("/var/lib/machina/backups")
}

fn backup_script() -> PathBuf {
    let installed = PathBuf::from("/usr/local/share/machina/scripts/backup.sh");
    if installed.exists() {
        return installed;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let relative = parent.join("../scripts/backup.sh");
            if let Ok(canonical) = relative.canonicalize() {
                return canonical;
            }
        }
    }
    PathBuf::from("scripts/backup.sh")
}

fn validate_backup_id(id: &str) -> Result<(), AppError> {
    if id.is_empty() || id.len() > 64 {
        return Err(
            machina_core::LibvirtError::Invalid("Invalid backup id length".to_string()).into(),
        );
    }
    if !id.chars().all(|c| c.is_ascii_digit() || c == '-') {
        return Err(machina_core::LibvirtError::Invalid(
            "Backup id must contain only digits and dashes".to_string(),
        )
        .into());
    }
    // Explicitly reject path traversal patterns
    if id.contains("..") || id.contains('/') || id.contains('\\') {
        return Err(machina_core::LibvirtError::Invalid(
            "Backup id contains invalid path characters".to_string(),
        )
        .into());
    }
    // download_backup() passes `id` as the final bare argv element to `tar` (`tar czf -
    // -C <dir> <id>`) with no `--` separator ahead of it; the digit/dash-only charset
    // above still allows a leading '-', which tar would parse as an option instead of
    // the archive member name (same flag-injection class as
    // validate_login_username/validate_service_name/validate_package_token).
    if id.starts_with('-') {
        return Err(machina_core::LibvirtError::Invalid(
            "Backup id must not start with '-'".to_string(),
        )
        .into());
    }
    Ok(())
}

/// Validate NFS target format: must look like host:/path or ip:/path.
fn validate_nfs_target(target: &str) -> Result<(), AppError> {
    if target.is_empty() {
        return Ok(());
    }
    // Must contain exactly one colon separating host and path
    let parts: Vec<&str> = target.splitn(2, ':').collect();
    if parts.len() != 2 || parts[0].is_empty() || !parts[1].starts_with('/') {
        return Err(machina_core::LibvirtError::Invalid(
            "Invalid NFS target format. Expected: host:/path".to_string(),
        )
        .into());
    }
    let host = parts[0];
    // Host must be alphanumeric, dots, dashes only
    if !host
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '-')
    {
        return Err(machina_core::LibvirtError::Invalid(
            "NFS host contains invalid characters".to_string(),
        )
        .into());
    }
    // Reject suspicious hostname patterns
    if host.contains("..")
        || host.starts_with('-')
        || host.ends_with('-')
        || host.starts_with('.')
        || host.ends_with('.')
    {
        return Err(
            machina_core::LibvirtError::Invalid("NFS host has invalid format".to_string()).into(),
        );
    }
    // Path must not contain traversal
    if parts[1].contains("..") {
        return Err(machina_core::LibvirtError::Invalid(
            "NFS path must not contain '..'".to_string(),
        )
        .into());
    }
    Ok(())
}

fn dir_size_human(path: &std::path::Path) -> String {
    fn dir_size(path: &std::path::Path) -> u64 {
        let mut total: u64 = 0;
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let ft = match entry.file_type() {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };
                // Skip symlinks to avoid infinite recursion
                if ft.is_symlink() {
                    continue;
                }
                if ft.is_file() {
                    total += entry.metadata().map(|m| m.len()).unwrap_or(0);
                } else if ft.is_dir() {
                    total += dir_size(&entry.path());
                }
            }
        }
        total
    }

    let bytes = dir_size(path);
    if bytes >= 1_073_741_824 {
        format!("{:.1}G", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1}M", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}K", bytes as f64 / 1024.0)
    } else {
        format!("{}B", bytes)
    }
}

/// Generate a timestamp string in YYYYMMDD-HHMMSS format (pure Rust, no shell).
fn generate_timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Convert epoch seconds to YYYYMMDD-HHMMSS (UTC) without chrono
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Civil date from days since 1970-01-01 (algorithm from Howard Hinnant)
    let z = days_since_epoch as i64 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}{m:02}{d:02}-{hours:02}{minutes:02}{seconds:02}")
}

fn parse_backup_meta(dir: &std::path::Path, dir_name: &str) -> serde_json::Value {
    let meta_path = dir.join("backup.meta");
    let mut vm_filter = "all".to_string();
    let mut vm_count: u64 = 0;
    let mut net_count: u64 = 0;
    let mut with_disks = false;
    let mut nfs_target = "local".to_string();

    // Read status
    let status_path = dir.join("backup.status");
    let mut status = "completed".to_string();
    let mut status_message = String::new();
    let mut progress = String::new();
    if let Ok(content) = std::fs::read_to_string(&status_path) {
        for line in content.lines() {
            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "status" => status = value.trim().to_string(),
                    "message" => status_message = value.trim().to_string(),
                    "progress" => progress = value.trim().to_string(),
                    _ => {}
                }
            }
        }
    } else if !meta_path.exists() {
        status = "unknown".to_string();
    }

    if let Ok(content) = std::fs::read_to_string(&meta_path) {
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some((key, value)) = trimmed.split_once('=') {
                let key = key.trim();
                let value = value.trim().trim_matches('"').trim_matches('\'');
                match key {
                    "vm_filter" => vm_filter = value.to_string(),
                    "vm_count" => vm_count = value.parse().unwrap_or(0),
                    "net_count" => net_count = value.parse().unwrap_or(0),
                    "with_disks" => with_disks = value == "true" || value == "1",
                    "nfs_target" if !value.is_empty() => {
                        nfs_target = value.to_string();
                    }
                    _ => {}
                }
            }
        }
    } else if status != "running" {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.ends_with(".xml") {
                    if name.starts_with("net-") {
                        net_count += 1;
                    } else {
                        vm_count += 1;
                    }
                }
            }
        }
    }

    let has_checksums = dir.join("checksums.sha256").exists();
    let size = dir_size_human(dir);

    json!({
        "id": dir_name,
        "timestamp": dir_name,
        "vm_filter": vm_filter,
        "vm_count": vm_count,
        "net_count": net_count,
        "with_disks": with_disks,
        "nfs_target": nfs_target,
        "size": size,
        "status": status,
        "status_message": status_message,
        "progress": progress,
        "has_checksums": has_checksums,
    })
}

// ── Handlers ────────────────────────────────────────────────────────

/// POST /backups — trigger a new backup.
async fn trigger_backup(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    body: Option<Json<BackupRequest>>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "backups:write")?;
    let req = body.map(|Json(r)| r).unwrap_or_default();
    if machina_core::fluxvm::is_fluxvm(req.backend.as_deref()) {
        let vm = req.vm_name.as_deref().unwrap_or_default();
        if vm.is_empty() {
            return Err(machina_core::LibvirtError::Invalid(
                "vm_name is required for a FluxVM backup".into(),
            )
            .into());
        }
        let out = crate::routes::fluxvm::client()?
            .backup(vm, None, req.compress)
            .await?;
        return Ok(Json(json!({
            "status": "completed",
            "backup_id": out["name"],
            "backend": machina_core::fluxvm::BACKEND_NAME,
            "backup": fluxvm_backup_item(&out),
        })));
    }
    let script = backup_script();

    if !script.exists() {
        return Err(machina_core::LibvirtError::Operation(format!(
            "Backup script not found at {}",
            script.display()
        ))
        .into());
    }

    // Validate vm_name if provided
    if let Some(ref vm) = req.vm_name {
        if !vm.is_empty() {
            machina_core::validate::validate_name(vm)?;
        }
    }

    // Validate nfs_target if provided
    if let Some(ref nfs) = req.nfs_target {
        validate_nfs_target(nfs)?;
    }

    // Append sub-second precision. generate_timestamp() has 1-second resolution, so
    // two POSTs in the same wall-clock second (double-click / automation) produced
    // the SAME id → both backup.sh runs wrote into the same directory and corrupted
    // each other. Microseconds are digit-only, so the id still passes
    // validate_backup_id.
    let backup_id = format!(
        "{}-{:06}",
        generate_timestamp(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_micros()
    );
    let mut cmd = tokio::process::Command::new("bash");
    cmd.arg(&script);
    cmd.env(
        "MACHINA_BACKUP_DIR",
        backup_dir().to_string_lossy().as_ref(),
    );
    cmd.env("MACHINA_BACKUP_ID", &backup_id);

    if req.with_disks {
        cmd.arg("--with-disks");
    }
    if req.incremental {
        cmd.arg("--incremental");
    }
    if let Some(ref vm) = req.vm_name {
        if !vm.is_empty() {
            cmd.arg("--vm").arg(vm);
        }
    }
    if let Some(ref nfs) = req.nfs_target {
        if !nfs.is_empty() {
            cmd.arg("--nfs").arg(nfs);
        }
    }
    if req.retain > 0 {
        cmd.arg("--retain").arg(req.retain.to_string());
    }

    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    cmd.spawn().map_err(|e| {
        machina_core::LibvirtError::Operation(format!("Failed to start backup: {e}"))
    })?;

    Ok(Json(json!({
        "status": "started",
        "backup_id": backup_id,
    })))
}

/// GET /backups — list existing backups.
async fn list_backups(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<BackupQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "backups:write")?;
    if q.is_fluxvm() {
        return Ok(Json(json!(fluxvm_backup_items(q.vm.as_deref()).await?)));
    }
    let dir = backup_dir();
    let mut backups = Vec::new();

    if dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut dirs: Vec<_> = entries
                .flatten()
                .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                .collect();
            dirs.sort_by_key(|e| e.file_name());

            for entry in dirs {
                let name = entry.file_name();
                let name_str = name.to_string_lossy().to_string();
                backups.push(parse_backup_meta(&entry.path(), &name_str));
            }
        }
    }

    backups.reverse();
    if q.backend.is_none() && machina_core::MachinaConfig::load().fluxvm.enabled {
        match fluxvm_backup_items(None).await {
            Ok(items) => backups.extend(items),
            Err(_) => tracing::warn!("fluxvm: backup list unavailable"),
        }
    }
    Ok(Json(json!(backups)))
}

/// GET /backups/:id/status — get backup status and progress.
async fn get_backup_status(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "backups:write")?;
    validate_backup_id(&id)?;

    let dir = backup_dir().join(&id);
    // Canonicalize and verify the path is within the backup directory
    let dir = dir
        .canonicalize()
        .map_err(|_| machina_core::LibvirtError::NotFound(format!("Backup '{}' not found", id)))?;
    // Canonicalize the base too before comparing: if the configured backup dir has
    // a symlinked component (e.g. /var → /private/var), a canonicalized child would
    // not start with the raw base and every valid backup would 404.
    let base = backup_dir();
    let base = base.canonicalize().unwrap_or(base);
    if !dir.starts_with(&base) || !dir.is_dir() {
        return Err(
            machina_core::LibvirtError::NotFound(format!("Backup '{}' not found", id)).into(),
        );
    }

    let status_path = dir.join("backup.status");
    let mut status = "completed".to_string();
    let mut message = String::new();
    let mut progress = String::new();
    let mut updated = String::new();

    if let Ok(content) = std::fs::read_to_string(&status_path) {
        for line in content.lines() {
            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "status" => status = value.trim().to_string(),
                    "message" => message = value.trim().to_string(),
                    "progress" => progress = value.trim().to_string(),
                    "updated" => updated = value.trim().to_string(),
                    _ => {}
                }
            }
        }
    }

    Ok(Json(json!({
        "backup_id": id,
        "status": status,
        "message": message,
        "progress": progress,
        "updated": updated,
    })))
}

/// POST /backups/:id/verify — verify backup checksums.
async fn verify_backup(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "backups:write")?;
    validate_backup_id(&id)?;

    let dir = backup_dir().join(&id);
    let dir = dir
        .canonicalize()
        .map_err(|_| machina_core::LibvirtError::NotFound(format!("Backup '{}' not found", id)))?;
    // Canonicalize the base too before comparing: if the configured backup dir has
    // a symlinked component (e.g. /var → /private/var), a canonicalized child would
    // not start with the raw base and every valid backup would 404.
    let base = backup_dir();
    let base = base.canonicalize().unwrap_or(base);
    if !dir.starts_with(&base) || !dir.is_dir() {
        return Err(
            machina_core::LibvirtError::NotFound(format!("Backup '{}' not found", id)).into(),
        );
    }

    let checksum_file = dir.join("checksums.sha256");
    if !checksum_file.exists() {
        return Ok(Json(json!({
            "backup_id": id,
            "verified": false,
            "error": "No checksums.sha256 file found",
            "files_checked": 0,
            "files_failed": 0,
        })));
    }

    // Use tokio::process to avoid blocking the async runtime
    let output = tokio::process::Command::new("sha256sum")
        .arg("-c")
        .arg("checksums.sha256")
        .current_dir(&dir)
        .output()
        .await
        .map_err(|e| {
            machina_core::LibvirtError::Operation(format!("Failed to run sha256sum: {e}"))
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let total = stdout.lines().count();
    let failed: Vec<&str> = stdout.lines().filter(|l| l.contains("FAILED")).collect();
    let ok_count = total - failed.len();

    Ok(Json(json!({
        "backup_id": id,
        "verified": output.status.success(),
        "files_checked": total,
        "files_ok": ok_count,
        "files_failed": failed.len(),
        "failed_files": failed,
        "errors": stderr.trim(),
    })))
}

/// GET /backups/:id/download — stream backup as tar.gz.
async fn download_backup(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    require_write(&actor, "backups:write")?;
    validate_backup_id(&id)?;

    let dir = backup_dir().join(&id);
    if !dir.exists() || !dir.is_dir() {
        return Err(
            machina_core::LibvirtError::NotFound(format!("Backup '{}' not found", id)).into(),
        );
    }

    // Stream tar output instead of buffering entire archive in memory
    let mut child = tokio::process::Command::new("tar")
        .arg("czf")
        .arg("-")
        .arg("-C")
        .arg(backup_dir().to_string_lossy().as_ref())
        .arg(&id)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| machina_core::LibvirtError::Operation(format!("Failed to start tar: {e}")))?;

    let stdout = child.stdout.take().ok_or_else(|| {
        machina_core::LibvirtError::Operation("Failed to capture tar stdout".to_string())
    })?;

    let stream = ReaderStream::new(stdout);
    let body = Body::from_stream(stream);

    let filename = format!("machina-backup-{id}.tar.gz");
    Ok((
        [
            (header::CONTENT_TYPE, "application/gzip"),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{filename}\""),
            ),
        ],
        body,
    )
        .into_response())
}

/// POST /backups/restore — restore from a specific backup.
async fn restore_backup(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<RestoreRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "backups:write")?;
    if machina_core::fluxvm::is_fluxvm(req.backend.as_deref()) {
        let c = crate::routes::fluxvm::client()?;
        let vm = match req.vm_name.filter(|v| !v.is_empty()) {
            Some(v) => v,
            None => c
                .backups(None)
                .await?
                .into_iter()
                .find(|b| b["name"] == req.backup_id.as_str())
                .and_then(|b| b["vm_name"].as_str().map(str::to_string))
                .ok_or_else(|| {
                    machina_core::LibvirtError::NotFound(format!(
                        "FluxVM backup '{}' not found (or it has no vm_name; pass vm_name)",
                        req.backup_id
                    ))
                })?,
        };
        let out = c.restore_backup(&vm, &req.backup_id).await?;
        return Ok(Json(json!({
            "status": "restored",
            "backup_id": req.backup_id,
            "vm_name": vm,
            "backend": machina_core::fluxvm::BACKEND_NAME,
            "result": out,
        })));
    }
    validate_backup_id(&req.backup_id)?;

    let dir = backup_dir().join(&req.backup_id);
    if !dir.exists() || !dir.is_dir() {
        return Err(machina_core::LibvirtError::NotFound(format!(
            "Backup '{}' not found",
            req.backup_id
        ))
        .into());
    }

    let script = backup_script();
    if !script.exists() {
        return Err(machina_core::LibvirtError::Operation(format!(
            "Backup script not found at {}",
            script.display()
        ))
        .into());
    }

    let mut cmd = tokio::process::Command::new("bash");
    cmd.arg(&script)
        .arg("--restore")
        .arg(dir.to_string_lossy().as_ref());
    cmd.env(
        "MACHINA_BACKUP_DIR",
        backup_dir().to_string_lossy().as_ref(),
    );

    // backup.sh writes no status file for --restore (only for a fresh backup),
    // so unlike trigger_backup this run has no `/backups/:id/status` to poll.
    // Discarding stdout/stderr to /dev/null left operators with a bare
    // "restore_started" and zero way to ever learn whether it actually
    // succeeded, partially failed (the script `warn`s and continues on
    // per-VM/per-disk errors), or crashed. Capture combined output to a log
    // file in the backup dir instead so it can be inspected after the fact.
    let log_path = dir.join("restore.log");
    let log_file = std::fs::File::create(&log_path).map_err(|e| {
        machina_core::LibvirtError::Operation(format!("Failed to create restore log: {e}"))
    })?;
    let log_file_err = log_file.try_clone().map_err(|e| {
        machina_core::LibvirtError::Operation(format!("Failed to create restore log: {e}"))
    })?;

    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log_file))
        .stderr(std::process::Stdio::from(log_file_err));

    cmd.spawn().map_err(|e| {
        machina_core::LibvirtError::Operation(format!("Failed to start restore: {e}"))
    })?;

    Ok(Json(json!({
        "status": "restore_started",
        "backup_id": req.backup_id,
        "log": log_path.to_string_lossy(),
    })))
}

/// DELETE /backups/:id — remove a backup directory.
async fn delete_backup(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(id): Path<String>,
    Query(q): Query<BackupQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "backups:write")?;
    if q.is_fluxvm() {
        crate::routes::fluxvm::client()?.delete_backup(&id).await?;
        return Ok(Json(json!({
            "status": "deleted",
            "backup_id": id,
            "backend": machina_core::fluxvm::BACKEND_NAME,
        })));
    }
    validate_backup_id(&id)?;

    let dir = backup_dir().join(&id);
    if !dir.exists() || !dir.is_dir() {
        return Err(
            machina_core::LibvirtError::NotFound(format!("Backup '{}' not found", id)).into(),
        );
    }

    tokio::fs::remove_dir_all(&dir).await.map_err(|e| {
        machina_core::LibvirtError::Operation(format!("Failed to delete backup '{}': {e}", id))
    })?;

    Ok(Json(json!({
        "status": "deleted",
        "backup_id": id,
    })))
}

/// GET /backups/schedule — get backup timer status.
async fn get_schedule(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "backups:write")?;
    let active = tokio::process::Command::new("systemctl")
        .args(["is-active", "machina-backup.timer"])
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
        .unwrap_or(false);

    let enabled = tokio::process::Command::new("systemctl")
        .args(["is-enabled", "machina-backup.timer"])
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "enabled")
        .unwrap_or(false);

    let next_run = tokio::process::Command::new("systemctl")
        .args([
            "show",
            "machina-backup.timer",
            "--property=NextElapseUSecRealtime",
            "--value",
        ])
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let last_run = tokio::process::Command::new("systemctl")
        .args([
            "show",
            "machina-backup.timer",
            "--property=LastTriggerUSec",
            "--value",
        ])
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let installed = std::path::Path::new("/usr/lib/systemd/system/machina-backup.timer").exists()
        || std::path::Path::new("/etc/systemd/system/machina-backup.timer").exists();

    Ok(Json(json!({
        "installed": installed,
        "enabled": enabled,
        "active": active,
        "next_run": next_run,
        "last_run": last_run,
    })))
}

/// POST /backups/schedule — enable or disable backup timer.
#[derive(Deserialize)]
struct ScheduleRequest {
    enabled: bool,
}

async fn set_schedule(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<ScheduleRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "backups:write")?;
    let action = if req.enabled { "enable" } else { "disable" };

    let output = tokio::process::Command::new("systemctl")
        .args([action, "--now", "machina-backup.timer"])
        .output()
        .await
        .map_err(|e| {
            machina_core::LibvirtError::Operation(format!("Failed to {action} timer: {e}"))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(machina_core::LibvirtError::Operation(format!(
            "systemctl {action} failed: {stderr}"
        ))
        .into());
    }

    Ok(Json(json!({
        "status": action,
        "enabled": req.enabled,
    })))
}

// ── Router ──────────────────────────────────────────────────────────

pub fn backup_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/backups", get(list_backups).post(trigger_backup))
        .route("/backups/restore", post(restore_backup))
        .route("/backups/schedule", get(get_schedule).post(set_schedule))
        .route("/backups/{id}", delete(delete_backup))
        .route("/backups/{id}/status", get(get_backup_status))
        .route("/backups/{id}/verify", post(verify_backup))
        .route("/backups/{id}/download", get(download_backup))
}
