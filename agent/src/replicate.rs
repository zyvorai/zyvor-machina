// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Replicates a backup to (or fetches one from) an S3-compatible store for the controller. It runs
//! on the host that holds the backup, with that host's `aws` CLI and credentials, so the data never
//! passes through the controller. See `machina_core::backup_replica` for the validation and the
//! size check. Local paths must be inside `MACHINA_BACKUP_DIR`.

use std::path::Path;
use std::time::Duration;

use machina_core::backup_replica as plan;
use tokio::process::Command;

use crate::pb::{ReplicateBackupRequest, ReplicateBackupResponse};

fn fail(message: impl Into<String>) -> ReplicateBackupResponse {
    ReplicateBackupResponse {
        ok: false,
        message: message.into(),
        ..Default::default()
    }
}

fn timeout() -> Duration {
    Duration::from_secs(
        std::env::var("MACHINA_AGENT_REPLICATE_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4 * 3600),
    )
}

async fn aws(args: &[String]) -> Result<String, String> {
    let mut cmd = Command::new("aws");
    cmd.args(args).kill_on_drop(true);
    match tokio::time::timeout(timeout(), cmd.output()).await {
        Err(_) => Err("aws timed out".into()),
        Ok(Err(e)) => Err(format!("cannot run aws: {e}")),
        Ok(Ok(o)) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).to_string()),
        Ok(Ok(o)) => Err(machina_core::k3s_node::tail(
            String::from_utf8_lossy(&o.stderr).trim(),
            600,
        )),
    }
}

/// Every regular file under `path` (or `path` itself) as `(relative path with '/', size)`.
/// Symlinks are not followed and are not counted.
pub fn local_files(path: &Path) -> Result<Vec<(String, u64)>, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.is_file() {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("backup file name is not valid UTF-8")?;
        return Ok(vec![(name.to_string(), meta.len())]);
    }
    if !meta.is_dir() {
        return Err(format!("{} is not a file or directory", path.display()));
    }
    let mut out = Vec::new();
    let mut stack = vec![(path.to_path_buf(), String::new())];
    while let Some((dir, rel)) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().to_string();
            let rel_name = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let ft = entry.file_type().map_err(|e| e.to_string())?;
            if ft.is_dir() {
                if rel_name.matches('/').count() >= 8 {
                    return Err("backup directory is nested too deeply".into());
                }
                stack.push((entry.path(), rel_name));
            } else if ft.is_file() {
                let len = entry.metadata().map_err(|e| e.to_string())?.len();
                out.push((rel_name, len));
            }
        }
    }
    out.sort();
    Ok(out)
}

async fn walk(path: &Path) -> Result<Vec<(String, u64)>, String> {
    let p = path.to_path_buf();
    tokio::task::spawn_blocking(move || local_files(&p))
        .await
        .map_err(|e| e.to_string())?
}

pub async fn handle(req: ReplicateBackupRequest) -> ReplicateBackupResponse {
    let root =
        std::env::var("MACHINA_BACKUP_DIR").unwrap_or_else(|_| "/var/lib/machina/backups".into());
    if !crate::backup::within_root(&req.local_path, &root) {
        return fail(format!("refused: the path must be inside {root}"));
    }
    if let Err(e) =
        plan::validate_key(&req.key).and_then(|()| plan::validate_endpoint(&req.endpoint_url))
    {
        return fail(e);
    }
    if !req
        .bucket
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
        || req.bucket.len() < 3
        || req.bucket.starts_with(['-', '.'])
    {
        return fail("invalid bucket name");
    }
    match req.action.as_str() {
        "push" => push(&req).await,
        "fetch" => fetch(&req).await,
        other => fail(format!("unknown action '{other}'")),
    }
}

async fn list_remote(req: &ReplicateBackupRequest) -> Result<Vec<(u64, String)>, String> {
    let out = aws(&plan::ls_args(&req.bucket, &req.key, &req.endpoint_url)).await?;
    Ok(plan::parse_ls(&out))
}

async fn push(req: &ReplicateBackupRequest) -> ReplicateBackupResponse {
    let path = Path::new(&req.local_path);
    if !path.exists() {
        return fail(
            "the backup is not on this host (it may have been pruned, or it is an Atlas backup)",
        );
    }
    let is_dir = path.is_dir();
    let local = match walk(path).await {
        Ok(l) => l,
        Err(e) => return fail(e),
    };
    if let Err(e) = aws(&plan::push_args(
        &req.local_path,
        is_dir,
        &req.bucket,
        &req.key,
        &req.endpoint_url,
    ))
    .await
    {
        return fail(format!("upload failed: {e}"));
    }
    let remote = match list_remote(req).await {
        Ok(r) => r,
        Err(e) => return fail(format!("uploaded, but listing the store failed: {e}")),
    };
    match plan::verify(&local, &remote, &req.key, is_dir) {
        Ok(bytes) => ReplicateBackupResponse {
            ok: true,
            message: "uploaded and verified".into(),
            bytes,
            files: local.len() as u32,
            is_dir,
            remote_uri: plan::uri(&req.bucket, &req.key),
        },
        Err(e) => fail(format!("verification failed: {e}")),
    }
}

async fn fetch(req: &ReplicateBackupRequest) -> ReplicateBackupResponse {
    let dest = Path::new(&req.local_path);
    if dest.exists() {
        return fail("refused: the destination already exists");
    }
    let remote = match list_remote(req).await {
        Ok(r) if !r.is_empty() => r,
        Ok(_) => return fail("nothing found in the store at that key"),
        Err(e) => return fail(format!("listing the store failed: {e}")),
    };
    let is_dir = plan::remote_is_dir(&remote, &req.key);
    if let Some(parent) = dest.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return fail(format!("cannot create {}: {e}", parent.display()));
        }
    }
    if let Err(e) = aws(&plan::fetch_args(
        &req.local_path,
        is_dir,
        &req.bucket,
        &req.key,
        &req.endpoint_url,
    ))
    .await
    {
        return fail(format!("download failed: {e}"));
    }
    let expected = plan::expected_files(&remote, &req.key, is_dir);
    let local = match walk(dest).await {
        Ok(l) => l,
        Err(e) => return fail(format!("downloaded, but reading it back failed: {e}")),
    };
    if local != expected {
        return fail("verification failed: the downloaded files do not match the store listing");
    }
    ReplicateBackupResponse {
        ok: true,
        message: "downloaded and verified".into(),
        bytes: local.iter().map(|(_, s)| s).sum(),
        files: local.len() as u32,
        is_dir,
        remote_uri: plan::uri(&req.bucket, &req.key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_files_lists_a_file_and_a_nested_directory() {
        let dir = std::env::temp_dir().join(format!("machina-repl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("vm.d/sub")).unwrap();
        std::fs::write(dir.join("vm.d/manifest.json"), b"{}").unwrap();
        std::fs::write(dir.join("vm.d/sub/disk.qcow2"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.join("single.qcow2"), vec![0u8; 7]).unwrap();
        assert_eq!(
            local_files(&dir.join("single.qcow2")).unwrap(),
            vec![("single.qcow2".to_string(), 7)]
        );
        assert_eq!(
            local_files(&dir.join("vm.d")).unwrap(),
            vec![
                ("manifest.json".to_string(), 2),
                ("sub/disk.qcow2".to_string(), 100)
            ]
        );
        assert!(local_files(&dir.join("missing")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn refuses_paths_outside_the_backup_root_and_bad_input() {
        std::env::remove_var("MACHINA_BACKUP_DIR");
        let base = ReplicateBackupRequest {
            action: "push".into(),
            local_path: "/etc/shadow".into(),
            bucket: "dr-backups".into(),
            key: "machina/x".into(),
            endpoint_url: String::new(),
        };
        let r = handle(base.clone()).await;
        assert!(!r.ok && r.message.contains("inside"));
        let r = handle(ReplicateBackupRequest {
            local_path: "/var/lib/machina/backups/../../etc/shadow".into(),
            ..base.clone()
        })
        .await;
        assert!(!r.ok);
        let r = handle(ReplicateBackupRequest {
            local_path: "/var/lib/machina/backups/a.qcow2".into(),
            bucket: "--no-sign-request".into(),
            ..base.clone()
        })
        .await;
        assert!(!r.ok && r.message.contains("bucket"));
        let r = handle(ReplicateBackupRequest {
            local_path: "/var/lib/machina/backups/a.qcow2".into(),
            action: "delete".into(),
            ..base
        })
        .await;
        assert!(!r.ok && r.message.contains("unknown action"));
    }
}
