// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{DefaultBodyLimit, Extension, Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use machina_core::build_precheck;
use machina_core::host_inventory::{
    gather_hardware_inventory_report, inventory_history_jsonl_path, load_inventory_history_entries,
    HardwareInventoryReport,
};
use machina_core::host_platform;
use machina_core::libvirt::node;
use machina_core::libvirt::{extras, storage, virt_builder};
use machina_core::{audit, AuditEvent, LibvirtError, LibvirtManager, MachinaConfig};
use serde::Deserialize;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

use crate::auth::{
    require_api_scope, require_browse_host_paths, require_browser_session_for_host_insight,
    require_destroy_vm, require_usb_pci, require_write, RequestActor,
};
use crate::conn_query::{spawn_libvirt_actor, ConnQuery};
use crate::error::AppError;

/// Caps concurrent blocking host probes (`package-updates`, `net-rates`) that can stall the default pool.
static HOST_HEAVY_PROBE_SEM: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(2));

/// Only one mutating package action at a time (can run for a long time and locks package managers).
static HOST_PACKAGE_ACTION_SEM: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(1));

/// Short-lived cache for `virt-builder --list --list-format json` (avoid hammering the tool on every UI poll).
const VIRT_BUILDER_LIST_CACHE_TTL: Duration = Duration::from_secs(300);

struct VirtBuilderIndexCache {
    fetched_at: Instant,
    index: virt_builder::VirtBuilderIndex,
}

static VIRT_BUILDER_INDEX_CACHE: Mutex<Option<VirtBuilderIndexCache>> = Mutex::new(None);

fn log_audit(action: &str, target: &str, result: &str) {
    let event = AuditEvent {
        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        action: action.to_string(),
        target: target.to_string(),
        result: result.to_string(),
        actor: String::new(),
    };
    audit::write_audit_event(&event);
}

fn log_audit_with_actor(actor: &RequestActor, action: &str, target: &str, result: &str) {
    let event = AuditEvent {
        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        action: action.to_string(),
        target: target.to_string(),
        result: result.to_string(),
        actor: actor.username.clone(),
    };
    audit::write_audit_event(&event);
}

// ── ISO / Disk Browser ─────────────────────────────────────────────

async fn list_isos(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
) -> Result<Json<extras::BrowseFilesResponse>, AppError> {
    // Every sibling handler in this file gates on this; this one didn't, letting
    // any authenticated session enumerate ISO paths under `/root`, `/home`, and
    // `/tmp` (see `list_iso_files`) regardless of host-path-browsing rights.
    require_browse_host_paths(&actor)?;
    let mgr = manager.clone();
    let res = tokio::task::spawn_blocking(move || mgr.with_conn(extras::list_iso_files))
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(res))
}

#[derive(Deserialize)]
struct IsoUploadQuery {
    /// Bare filename chosen by the browser (never a path — see `iso_upload::sanitize_iso_filename`).
    filename: String,
    #[serde(default)]
    overwrite: bool,
}

/// Stream a browser-supplied ISO to the configured upload directory.
///
/// The body is written straight to disk in chunks so a multi-GiB Windows ISO
/// never has to fit in memory. Bytes land in a `.part` staging file and are
/// renamed into place only after the final byte, so a cancelled or truncated
/// upload can never be picked up by the ISO browser or VM create.
async fn upload_iso(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(q): Query<IsoUploadQuery>,
    headers: axum::http::HeaderMap,
    body: axum::body::Body,
) -> Result<Json<serde_json::Value>, AppError> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    require_browse_host_paths(&actor)?;

    let cfg = MachinaConfig::load();
    let max_gib = cfg.libvirt.iso_upload_max_gib;
    if max_gib == 0 {
        return Err(AppError::from(LibvirtError::Forbidden(
            "ISO upload is disabled ([libvirt] iso_upload_max_gib = 0)".into(),
        )));
    }
    let max_bytes = max_gib.saturating_mul(1024 * 1024 * 1024);
    let upload_dir = cfg.libvirt.iso_upload_dir.clone();

    // Reject an oversized upload before reading a single byte.
    let declared_len = headers
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    if declared_len > max_bytes {
        log_audit_with_actor(&actor, "iso.upload", &q.filename, "rejected: too large");
        return Err(AppError::from(LibvirtError::Invalid(format!(
            "upload is {} GiB; the limit is {max_gib} GiB ([libvirt] iso_upload_max_gib)",
            declared_len / (1024 * 1024 * 1024)
        ))));
    }

    let dir = std::path::PathBuf::from(upload_dir.trim());
    tokio::fs::create_dir_all(&dir).await.map_err(|e| {
        AppError::from(LibvirtError::Operation(format!(
            "cannot create ISO upload directory {}: {e}",
            dir.display()
        )))
    })?;
    machina_core::iso_upload::check_free_space(&dir, declared_len)?;
    let (final_path, staging_path) =
        machina_core::iso_upload::resolve_upload_target(&upload_dir, &q.filename, q.overwrite)?;

    // `create_new` doubles as the concurrency guard: a second upload of the same
    // name while one is in flight fails here instead of interleaving writes.
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging_path)
        .await
        .map_err(|e| {
            AppError::from(if e.kind() == std::io::ErrorKind::AlreadyExists {
                LibvirtError::Invalid(format!(
                    "an upload of {} is already in progress",
                    q.filename
                ))
            } else {
                LibvirtError::Operation(format!("cannot open {}: {e}", staging_path.display()))
            })
        })?;

    // Re-checked periodically, not just once up front: a request with no (or a
    // dishonest) Content-Length skips `check_free_space` above entirely, and
    // could otherwise fill the pool filesystem before `max_bytes` is reached.
    const FREE_SPACE_RECHECK_INTERVAL: u64 = 1024 * 1024 * 1024;
    let mut last_space_check = 0u64;

    let mut written: u64 = 0;
    let mut stream = body.into_data_stream();
    let mut failure: Option<LibvirtError> = None;
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            // Client disconnect / network drop mid-transfer.
            Err(e) => {
                failure = Some(LibvirtError::Operation(format!("upload interrupted: {e}")));
                break;
            }
        };
        written = written.saturating_add(chunk.len() as u64);
        // Enforce the real cap on the wire, not just the declared Content-Length.
        if written > max_bytes {
            failure = Some(LibvirtError::Invalid(format!(
                "upload exceeds the {max_gib} GiB limit ([libvirt] iso_upload_max_gib)"
            )));
            break;
        }
        if written - last_space_check >= FREE_SPACE_RECHECK_INTERVAL {
            last_space_check = written;
            let remaining_budget = max_bytes.saturating_sub(written);
            if let Err(e) = machina_core::iso_upload::check_free_space(&dir, remaining_budget) {
                failure = Some(e);
                break;
            }
        }
        if let Err(e) = file.write_all(&chunk).await {
            failure = Some(LibvirtError::Operation(format!(
                "write to {} failed: {e}",
                staging_path.display()
            )));
            break;
        }
    }

    if failure.is_none() {
        // Durability before the rename: a crash must not leave a valid-looking
        // name pointing at unflushed bytes.
        if let Err(e) = file.flush().await {
            failure = Some(LibvirtError::Operation(format!("flush failed: {e}")));
        } else if let Err(e) = file.sync_all().await {
            failure = Some(LibvirtError::Operation(format!("fsync failed: {e}")));
        }
    }
    drop(file);

    if let Some(err) = failure {
        let _ = tokio::fs::remove_file(&staging_path).await;
        log_audit_with_actor(&actor, "iso.upload", &q.filename, "failed");
        return Err(AppError::from(err));
    }

    if written == 0 {
        let _ = tokio::fs::remove_file(&staging_path).await;
        log_audit_with_actor(&actor, "iso.upload", &q.filename, "failed: empty");
        return Err(AppError::from(LibvirtError::Invalid(
            "uploaded file is empty".into(),
        )));
    }

    // `resolve_upload_target`'s no-overwrite check ran before this (potentially
    // long) transfer started; a competing writer could have created
    // `final_path` since. `rename` itself would clobber it unconditionally, so
    // it's re-checked immediately beforehand.
    if !q.overwrite && final_path.exists() {
        let _ = tokio::fs::remove_file(&staging_path).await;
        log_audit_with_actor(
            &actor,
            "iso.upload",
            &q.filename,
            "failed: raced by another upload",
        );
        return Err(AppError::from(LibvirtError::Invalid(format!(
            "{} was created by another request — not overwriting",
            q.filename
        ))));
    }

    tokio::fs::rename(&staging_path, &final_path)
        .await
        .map_err(|e| {
            AppError::from(LibvirtError::Operation(format!(
                "cannot finalise {}: {e}",
                final_path.display()
            )))
        })?;

    // qemu/libvirt run as a different user and must be able to read the ISO.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ =
            tokio::fs::set_permissions(&final_path, std::fs::Permissions::from_mode(0o644)).await;
    }

    log_audit_with_actor(
        &actor,
        "iso.upload",
        &final_path.display().to_string(),
        "success",
    );
    // Overwriting swaps the directory entry, but a running guest with this
    // exact path already mounted as CD-ROM keeps its old file descriptor —
    // it won't see the new bytes until the drive is ejected and reinserted.
    let final_path_display = final_path.display().to_string();
    let stale_mount_warning = tokio::task::spawn_blocking(move || {
        manager.with_conn(|conn| {
            Ok::<_, LibvirtError>(machina_core::libvirt::cdrom::vms_with_iso_mounted(
                conn,
                &final_path_display,
            ))
        })
    })
    .await
    .ok()
    .and_then(|r| r.ok())
    .filter(|hits| !hits.is_empty())
    .map(|hits| {
        let running: Vec<&str> = hits
            .iter()
            .filter(|(_, running)| *running)
            .map(|(name, _)| name.as_str())
            .collect();
        format!(
            "this ISO is currently mounted on: {} — {}",
            hits.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", "),
            if running.is_empty() {
                "eject and reinsert after the guest next boots to see the new content"
            } else {
                "running guest(s) will keep reading the old content until the drive is ejected and reinserted"
            }
        )
    });
    let mut response = serde_json::json!({
        "status": "ok",
        "name": final_path.file_name().and_then(|s| s.to_str()).unwrap_or_default(),
        "path": final_path.display().to_string(),
        "size_bytes": written,
    });
    if let Some(warning) = stale_mount_warning {
        response["warning"] = serde_json::Value::String(warning);
    }
    Ok(Json(response))
}

#[derive(Deserialize)]
struct IsoDownloadRequest {
    /// http(s) URL of the ISO to fetch onto the hypervisor.
    url: String,
    /// Optional override; otherwise derived from the URL's last path segment.
    #[serde(default)]
    filename: String,
    #[serde(default)]
    overwrite: bool,
}

/// Limits concurrent downloads so a burst cannot saturate the hypervisor's link
/// or fill the pool filesystem. Excess requests queue rather than being refused.
static ISO_DOWNLOAD_SEM: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(3));

/// Start a server-side ISO download and return a job to poll.
///
/// Downloads run on the hypervisor rather than through the operator's browser,
/// so they survive navigation, and several can run at once — the response is a
/// job id, not the finished file.
async fn download_iso(
    Extension(actor): Extension<RequestActor>,
    Extension(jobs): Extension<std::sync::Arc<crate::job_registry::JobRegistry>>,
    State(manager): State<LibvirtManager>,
    Json(req): Json<IsoDownloadRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browse_host_paths(&actor)?;

    let cfg = MachinaConfig::load();
    let max_gib = cfg.libvirt.iso_upload_max_gib;
    if max_gib == 0 {
        return Err(AppError::from(LibvirtError::Forbidden(
            "ISO download is disabled ([libvirt] iso_upload_max_gib = 0)".into(),
        )));
    }
    let max_bytes = max_gib.saturating_mul(1024 * 1024 * 1024);

    let url = req.url.trim().to_string();
    // Only plain http(s): the daemon runs as root, so `file://` or other schemes
    // would turn this into an arbitrary-read primitive.
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(AppError::from(LibvirtError::Invalid(
            "URL must start with http:// or https://".into(),
        )));
    }
    // The daemon runs as root on the hypervisor's own network: without this, a
    // caller could point the download at the cloud metadata endpoint or any
    // other internal host and have the result staged as a browsable, attachable
    // ISO. Re-checked on every redirect hop below (`safe_redirect_policy`), since
    // a same-URL DNS-rebind or a redirect to an internal host would otherwise
    // bypass this one-time check.
    machina_core::iso_upload::assert_public_http_host(&url).await?;

    // Derive a filename from the URL when the caller did not supply one.
    let raw_name = if req.filename.trim().is_empty() {
        url.split('?')
            .next()
            .unwrap_or(&url)
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_string()
    } else {
        req.filename.trim().to_string()
    };
    let name = machina_core::iso_upload::sanitize_iso_filename(&raw_name).map_err(|e| {
        AppError::from(LibvirtError::Invalid(format!(
            "{e} — pass an explicit `filename` ending in .iso"
        )))
    })?;

    let upload_dir = cfg.libvirt.iso_upload_dir.clone();
    let dir = std::path::PathBuf::from(upload_dir.trim());
    tokio::fs::create_dir_all(&dir).await.map_err(|e| {
        AppError::from(LibvirtError::Operation(format!(
            "cannot create ISO directory {}: {e}",
            dir.display()
        )))
    })?;
    let (final_path, staging_path) =
        machina_core::iso_upload::resolve_upload_target(&upload_dir, &name, req.overwrite)?;
    let overwrite = req.overwrite;

    let final_display = final_path.display().to_string();
    let job_id = jobs.start_iso_download(&url, &final_display);
    let actor_name = actor.username.clone();
    let final_display_for_task = final_display.clone();
    let manager_for_task = manager.clone();

    tokio::spawn(async move {
        let final_display = final_display_for_task;
        use futures_util::StreamExt;
        use tokio::io::AsyncWriteExt;

        let _permit = ISO_DOWNLOAD_SEM.acquire().await;
        jobs.append_log(job_id, &format!("GET {url}"));

        let fail = |jobs: &crate::job_registry::JobRegistry, msg: String| {
            jobs.append_log(job_id, &msg);
            jobs.fail(job_id, &msg);
        };

        // Redirects are followed manually (policy off) so each hop's target can be
        // re-checked against `assert_public_http_host` before it's fetched — a
        // redirect to an internal host would otherwise bypass the check above.
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(6 * 60 * 60))
            .redirect(reqwest::redirect::Policy::none())
            .build()
        {
            Ok(c) => c,
            Err(e) => return fail(&jobs, format!("http client: {e}")),
        };
        const MAX_REDIRECTS: u8 = 5;
        let mut current_url = url.clone();
        let mut redirects_left = MAX_REDIRECTS;
        let resp = loop {
            let attempt = match client.get(&current_url).send().await {
                Ok(r) => r,
                Err(e) => return fail(&jobs, format!("request failed: {e}")),
            };
            if attempt.status().is_redirection() {
                if redirects_left == 0 {
                    return fail(&jobs, "too many redirects".into());
                }
                redirects_left -= 1;
                let location = attempt
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                let next = match location {
                    Some(l) => match reqwest::Url::parse(&current_url).and_then(|b| b.join(&l)) {
                        Ok(u) => u.to_string(),
                        Err(e) => return fail(&jobs, format!("bad redirect target: {e}")),
                    },
                    None => return fail(&jobs, "redirect with no Location header".into()),
                };
                if let Err(e) = machina_core::iso_upload::assert_public_http_host(&next).await {
                    return fail(&jobs, e.to_string());
                }
                current_url = next;
                continue;
            }
            break attempt;
        };
        if !resp.status().is_success() {
            return fail(&jobs, format!("server returned HTTP {}", resp.status()));
        }
        let total = resp.content_length();
        if let Some(t) = total {
            if t > max_bytes {
                return fail(
                    &jobs,
                    format!(
                        "remote file is {} GiB; limit is {max_gib} GiB",
                        t / (1024 * 1024 * 1024)
                    ),
                );
            }
            jobs.set_download_total(job_id, Some(t));
            if let Err(e) = machina_core::iso_upload::check_free_space(&dir, t) {
                return fail(&jobs, e.to_string());
            }
        }

        // `create_new` doubles as the concurrency guard, same as the browser-upload
        // path: two downloads racing to the same filename would otherwise both
        // truncate-open the same `.part` file and interleave writes into it,
        // silently producing a corrupt ISO that both jobs report as "complete".
        let mut file = match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging_path)
            .await
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return fail(
                    &jobs,
                    format!("a download to {} is already in progress", final_display),
                );
            }
            Err(e) => {
                return fail(
                    &jobs,
                    format!("cannot open {}: {e}", staging_path.display()),
                )
            }
        };

        // Re-checked periodically below, not just once up front: a chunked
        // transfer (no Content-Length) skips the check above entirely, and a
        // server can simply lie about a small Content-Length while streaming
        // far more — both would otherwise be able to fill the pool filesystem
        // before the `max_bytes` cap is ever reached.
        const FREE_SPACE_RECHECK_INTERVAL: u64 = 1024 * 1024 * 1024;
        let mut last_space_check = 0u64;

        let mut written: u64 = 0;
        let mut stream = resp.bytes_stream();
        let mut last_report = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    let _ = tokio::fs::remove_file(&staging_path).await;
                    return fail(&jobs, format!("transfer interrupted: {e}"));
                }
            };
            written = written.saturating_add(chunk.len() as u64);
            if written - last_space_check >= FREE_SPACE_RECHECK_INTERVAL {
                last_space_check = written;
                // Budget for the rest of the transfer up to the hard cap, since
                // the true remaining size is unknown for a chunked response.
                let remaining_budget = max_bytes.saturating_sub(written);
                if let Err(e) = machina_core::iso_upload::check_free_space(&dir, remaining_budget) {
                    let _ = tokio::fs::remove_file(&staging_path).await;
                    return fail(&jobs, e.to_string());
                }
            }
            if written > max_bytes {
                let _ = tokio::fs::remove_file(&staging_path).await;
                return fail(&jobs, format!("download exceeds the {max_gib} GiB limit"));
            }
            if let Err(e) = file.write_all(&chunk).await {
                let _ = tokio::fs::remove_file(&staging_path).await;
                return fail(&jobs, format!("write failed: {e}"));
            }
            // Report every 8 MiB rather than every chunk — the registry takes a
            // mutex, and a 3 GiB ISO is ~200k chunks.
            if written - last_report >= 8 * 1024 * 1024 {
                last_report = written;
                jobs.update_download_progress(job_id, written);
            }
        }

        if let Err(e) = file.flush().await.and(file.sync_all().await) {
            let _ = tokio::fs::remove_file(&staging_path).await;
            return fail(&jobs, format!("flush failed: {e}"));
        }
        drop(file);

        if written == 0 {
            let _ = tokio::fs::remove_file(&staging_path).await;
            return fail(&jobs, "downloaded file is empty".into());
        }
        // `resolve_upload_target`'s no-overwrite check ran before this
        // (potentially long) transfer started; a competing writer could have
        // created `final_path` since. `rename` itself would clobber it
        // unconditionally, so it's re-checked immediately beforehand.
        if !overwrite && final_path.exists() {
            let _ = tokio::fs::remove_file(&staging_path).await;
            return fail(
                &jobs,
                format!("{final_display} was created by another request — not overwriting"),
            );
        }
        if let Err(e) = tokio::fs::rename(&staging_path, &final_path).await {
            let _ = tokio::fs::remove_file(&staging_path).await;
            return fail(&jobs, format!("cannot finalise: {e}"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = tokio::fs::set_permissions(&final_path, std::fs::Permissions::from_mode(0o644))
                .await;
        }

        jobs.update_download_progress(job_id, written);
        jobs.append_log(
            job_id,
            &format!("saved {} ({written} bytes)", final_path.display()),
        );
        // Overwriting swaps the directory entry, but a running guest with this
        // exact path already mounted as CD-ROM keeps its old file descriptor —
        // it won't see the new bytes until the drive is ejected and reinserted.
        let final_path_for_check = final_path.display().to_string();
        if let Ok(Ok(hits)) = tokio::task::spawn_blocking(move || {
            manager_for_task.with_conn(|conn| {
                Ok::<_, LibvirtError>(machina_core::libvirt::cdrom::vms_with_iso_mounted(
                    conn,
                    &final_path_for_check,
                ))
            })
        })
        .await
        {
            if !hits.is_empty() {
                jobs.append_log(
                    job_id,
                    &format!(
                        "note: this ISO is currently mounted on {} — eject and reinsert to see the new content",
                        hits.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ")
                    ),
                );
            }
        }
        jobs.complete_iso_download(job_id, &final_path.display().to_string(), written);
        log_audit(
            "iso.download",
            &format!("{} <- {url} by {actor_name}", final_path.display()),
            "success",
        );
    });

    Ok(Json(serde_json::json!({
        "status": "started",
        "job_id": job_id.to_string(),
        "name": name,
        "path": final_display,
    })))
}

/// Release that ships the in-guest agent bundles (ISO + MSI + Linux musl tarball).
const GUEST_AGENT_ISO_NAME: &str = "guestkit-agent-0.3.14.iso";
const GUEST_AGENT_ISO_URL: &str = "https://github.com/hypersdk/guestkit/releases/download/guestkit-agent-v0.3.14/guestkit-agent-0.3.14.iso";

#[derive(Deserialize)]
struct GuestAgentInstallRequest {
    /// Override the agent ISO URL (air-gapped mirrors, pinned versions).
    #[serde(default)]
    iso_url: String,
    /// Also add the QEMU guest-agent channel when the domain lacks one.
    #[serde(default = "default_true_flag")]
    ensure_channel: bool,
}

fn default_true_flag() -> bool {
    true
}

/// Stage everything a guest needs to run the in-guest agent.
///
/// Attaching media and adding the agent channel were separate manual steps that
/// each failed in their own way — the ISO had to be fetched out of band, the
/// CD-ROM collided with the root disk, and a domain with no virtio channel could
/// never talk to an agent at all. This does the three of them together and
/// reports plainly whether the guest must restart before it can see any of it.
async fn install_guest_agent(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(conn_q): Query<crate::conn_query::ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<GuestAgentInstallRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browse_host_paths(&actor)?;

    let cfg = MachinaConfig::load();
    let max_gib = cfg.libvirt.iso_upload_max_gib;
    let max_bytes = max_gib.saturating_mul(1024 * 1024 * 1024);
    let dir = std::path::PathBuf::from(cfg.libvirt.iso_upload_dir.trim());
    tokio::fs::create_dir_all(&dir).await.ok();
    let iso_path = dir.join(GUEST_AGENT_ISO_NAME);

    // Fetch the agent ISO only if it is not already on the host.
    let mut downloaded = false;
    if !iso_path.exists() {
        if max_gib == 0 {
            return Err(AppError::from(LibvirtError::Forbidden(
                "ISO download is disabled ([libvirt] iso_upload_max_gib = 0)".into(),
            )));
        }
        let url = if req.iso_url.trim().is_empty() {
            GUEST_AGENT_ISO_URL.to_string()
        } else {
            req.iso_url.trim().to_string()
        };
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(AppError::from(LibvirtError::Invalid(
                "iso_url must start with http:// or https://".into(),
            )));
        }
        // Same SSRF guard as the ISO-download job: `iso_url` is caller-overridable
        // ("air-gapped mirrors, pinned versions"), so without this a caller could
        // point the daemon at an internal host and stage the response as a
        // browsable, attachable ISO.
        machina_core::iso_upload::assert_public_http_host(&url).await?;
        let staging = dir.join(format!("{GUEST_AGENT_ISO_NAME}.part"));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(1800))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| AppError::from(LibvirtError::Operation(format!("http client: {e}"))))?;
        const MAX_REDIRECTS: u8 = 5;
        let mut current_url = url.clone();
        let mut redirects_left = MAX_REDIRECTS;
        let resp = loop {
            let attempt = client.get(&current_url).send().await.map_err(|e| {
                AppError::from(LibvirtError::Operation(format!(
                    "cannot fetch agent ISO: {e}"
                )))
            })?;
            if attempt.status().is_redirection() {
                if redirects_left == 0 {
                    return Err(AppError::from(LibvirtError::Operation(
                        "too many redirects".into(),
                    )));
                }
                redirects_left -= 1;
                let location = attempt
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                let next = match location {
                    Some(l) => reqwest::Url::parse(&current_url)
                        .and_then(|b| b.join(&l))
                        .map_err(|e| {
                            AppError::from(LibvirtError::Operation(format!(
                                "bad redirect target: {e}"
                            )))
                        })?
                        .to_string(),
                    None => {
                        return Err(AppError::from(LibvirtError::Operation(
                            "redirect with no Location header".into(),
                        )))
                    }
                };
                machina_core::iso_upload::assert_public_http_host(&next).await?;
                current_url = next;
                continue;
            }
            break attempt;
        };
        if !resp.status().is_success() {
            return Err(AppError::from(LibvirtError::Operation(format!(
                "agent ISO download returned HTTP {}",
                resp.status()
            ))));
        }
        if let Some(t) = resp.content_length() {
            if t > max_bytes {
                return Err(AppError::from(LibvirtError::Invalid(format!(
                    "remote file is {} GiB; limit is {max_gib} GiB",
                    t / (1024 * 1024 * 1024)
                ))));
            }
            machina_core::iso_upload::check_free_space(&dir, t)?;
        }

        use futures_util::StreamExt;
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .await
            .map_err(|e| {
                AppError::from(if e.kind() == std::io::ErrorKind::AlreadyExists {
                    LibvirtError::Invalid("a guest-agent ISO fetch is already in progress".into())
                } else {
                    LibvirtError::Operation(format!("cannot open {}: {e}", staging.display()))
                })
            })?;
        const FREE_SPACE_RECHECK_INTERVAL: u64 = 1024 * 1024 * 1024;
        let mut last_space_check = 0u64;
        let mut written: u64 = 0;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                AppError::from(LibvirtError::Operation(format!("agent ISO transfer: {e}")))
            })?;
            written = written.saturating_add(chunk.len() as u64);
            if written > max_bytes {
                let _ = tokio::fs::remove_file(&staging).await;
                return Err(AppError::from(LibvirtError::Invalid(format!(
                    "download exceeds the {max_gib} GiB limit"
                ))));
            }
            if written - last_space_check >= FREE_SPACE_RECHECK_INTERVAL {
                last_space_check = written;
                let remaining_budget = max_bytes.saturating_sub(written);
                if let Err(e) = machina_core::iso_upload::check_free_space(&dir, remaining_budget) {
                    let _ = tokio::fs::remove_file(&staging).await;
                    return Err(AppError::from(e));
                }
            }
            if let Err(e) = file.write_all(&chunk).await {
                let _ = tokio::fs::remove_file(&staging).await;
                return Err(AppError::from(LibvirtError::Operation(format!(
                    "write failed: {e}"
                ))));
            }
        }
        if let Err(e) = file.flush().await.and(file.sync_all().await) {
            let _ = tokio::fs::remove_file(&staging).await;
            return Err(AppError::from(LibvirtError::Operation(format!(
                "flush failed: {e}"
            ))));
        }
        drop(file);
        if written == 0 {
            let _ = tokio::fs::remove_file(&staging).await;
            return Err(AppError::from(LibvirtError::Invalid(
                "downloaded file is empty".into(),
            )));
        }
        tokio::fs::rename(&staging, &iso_path)
            .await
            .map_err(|e| AppError::from(LibvirtError::Operation(format!("rename failed: {e}"))))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ =
                tokio::fs::set_permissions(&iso_path, std::fs::Permissions::from_mode(0o644)).await;
        }
        downloaded = true;
    }

    let iso_str = iso_path.display().to_string();
    let vm = name.clone();
    let ensure_channel = req.ensure_channel;
    let iso_for_task = iso_str.clone();

    // Held across cdrom + channel setup: both mutate the same domain's device
    // list from a read-then-write sequence, same race the dedicated
    // insert/eject/channel handlers guard against.
    let _vm_guard = manager.lock_vm(&name).await;
    let (cdrom_outcome, channel_outcome) = spawn_libvirt_actor(
        manager,
        Some(&actor),
        conn_q,
        move |conn| -> Result<_, LibvirtError> {
            // Empty target = pick a free one; "sda" is the root disk on most guests.
            let cd = machina_core::libvirt::cdrom::insert_cdrom(conn, &vm, &iso_for_task, "")?;
            let ch = if ensure_channel {
                Some(machina_core::libvirt::qga_channel::ensure_guest_agent_channel(conn, &vm)?)
            } else {
                None
            };
            Ok((cd, ch))
        },
    )
    .await?;

    let needs_restart = cdrom_outcome.requires_restart
        || channel_outcome.as_ref().is_some_and(|c| c.requires_restart);

    log_audit_with_actor(&actor, "guest-agent.install-media", &name, "success");

    Ok(Json(serde_json::json!({
        "status": "ok",
        "vm": name,
        "iso_path": iso_str,
        "iso_downloaded": downloaded,
        "cdrom": cdrom_outcome,
        "channel": channel_outcome,
        "requires_restart": needs_restart,
        "next_step": if needs_restart {
            "Restart the VM, then run the installer from the mounted CD (Windows: the MSI; Linux: install.sh)."
        } else {
            "Open the VM console and run the installer from the mounted CD (Windows: the MSI; Linux: install.sh)."
        },
    })))
}

/// Enable Remote Desktop on a stopped Windows guest by editing its hive offline.
async fn enable_windows_rdp(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(conn_q): Query<crate::conn_query::ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browse_host_paths(&actor)?;
    // Held for the whole request, including the guestkit subprocess below: a
    // client retry after an HTTP-level timeout does not cancel the in-flight
    // server-side work (`spawn_blocking` isn't dropped with the response), so
    // without this a retry starts a *second* hive writer against the same disk
    // while the first is still running — a real corruption risk hit live against
    // this exact endpoint.
    let _vm_guard = manager.lock_vm(&name).await;
    let cfg = MachinaConfig::load();
    let guestkit_bin = cfg.libvirt.guestkit_agent_binary.clone();
    let vm = name.clone();

    // Resolve the root disk and refuse while the guest is running: mutating a
    // registry hive under a live Windows kernel can corrupt it.
    let disk = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        let domain = machina_core::libvirt::domain::lookup_domain(conn, &vm)?;
        let running = domain.get_info().map(|i| i.state == 1).unwrap_or(false);
        if running {
            return Err(LibvirtError::Invalid(
                "stop the VM first — editing a Windows registry hive while the guest is running can corrupt it".into(),
            ));
        }
        let xml = domain.get_xml_desc(0).unwrap_or_default();
        for block in machina_core::xml::split_blocks(&xml, "disk") {
            let device =
                machina_core::xml::extract_attr(&block, "disk", "device").unwrap_or_default();
            if device != "disk" {
                continue;
            }
            if let Some(p) = machina_core::xml::extract_attr(&block, "source", "file") {
                if !p.is_empty() {
                    return Ok(p);
                }
            }
        }
        Err(LibvirtError::NotFound(
            "no file-backed root disk found for this VM".into(),
        ))
    })
    .await?;

    let outcome = tokio::task::spawn_blocking(move || {
        machina_core::libvirt::windows_rdp::enable_rdp_offline(&disk, &guestkit_bin)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("task failed: {e}"))))??;

    log_audit_with_actor(&actor, "windows.enable-rdp", &name, "success");
    Ok(Json(serde_json::json!({
        "status": "ok",
        "vm": name,
        "result": outcome,
    })))
}

/// Resolve the first file-backed disk and refuse while the guest is running.
async fn linux_offline_disk(
    manager: LibvirtManager,
    actor: &RequestActor,
    conn_q: crate::conn_query::ConnQuery,
    name: &str,
) -> Result<String, AppError> {
    let vm = name.to_string();
    spawn_libvirt_actor(manager, Some(actor), conn_q, move |conn| {
        let domain = machina_core::libvirt::domain::lookup_domain(conn, &vm)?;
        let running = domain.get_info().map(|i| i.state == 1).unwrap_or(false);
        if running {
            return Err(LibvirtError::Invalid(
                "stop the VM first — offline GuestKit edits while the guest is running can corrupt the disk"
                    .into(),
            ));
        }
        let xml = domain.get_xml_desc(0).unwrap_or_default();
        for block in machina_core::xml::split_blocks(&xml, "disk") {
            let device =
                machina_core::xml::extract_attr(&block, "disk", "device").unwrap_or_default();
            if device != "disk" {
                continue;
            }
            if let Some(p) = machina_core::xml::extract_attr(&block, "source", "file") {
                if !p.is_empty() {
                    return Ok(p);
                }
            }
        }
        Err(LibvirtError::NotFound(
            "no file-backed root disk found for this VM".into(),
        ))
    })
    .await
}

#[derive(Deserialize)]
struct LinuxInjectSshKeyReq {
    user: String,
    public_key: String,
}

#[derive(Deserialize)]
struct LinuxResetPasswordReq {
    user: String,
    password: String,
}

#[derive(Deserialize)]
struct LinuxSetHostnameReq {
    hostname: String,
}

async fn enable_linux_ssh(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(conn_q): Query<crate::conn_query::ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browse_host_paths(&actor)?;
    let _vm_guard = manager.lock_vm(&name).await;
    let cfg = MachinaConfig::load();
    let guestkit_bin = cfg.libvirt.guestkit_agent_binary.clone();
    let disk = linux_offline_disk(manager, &actor, conn_q, &name).await?;
    let outcome = tokio::task::spawn_blocking(move || {
        machina_core::libvirt::linux_guestkit::enable_ssh_offline(&disk, &guestkit_bin)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("task failed: {e}"))))??;
    log_audit_with_actor(&actor, "linux.enable-ssh", &name, "success");
    Ok(Json(serde_json::json!({
        "status": "ok",
        "vm": name,
        "result": outcome,
    })))
}

async fn linux_inject_ssh_key(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(conn_q): Query<crate::conn_query::ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<LinuxInjectSshKeyReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browse_host_paths(&actor)?;
    let _vm_guard = manager.lock_vm(&name).await;
    let cfg = MachinaConfig::load();
    let guestkit_bin = cfg.libvirt.guestkit_agent_binary.clone();
    let disk = linux_offline_disk(manager, &actor, conn_q, &name).await?;
    let user = req.user;
    let public_key = req.public_key;
    let outcome = tokio::task::spawn_blocking(move || {
        machina_core::libvirt::linux_guestkit::inject_ssh_key_offline(
            &disk,
            &guestkit_bin,
            &user,
            &public_key,
        )
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("task failed: {e}"))))??;
    log_audit_with_actor(&actor, "linux.inject-ssh-key", &name, "success");
    Ok(Json(serde_json::json!({
        "status": "ok",
        "vm": name,
        "result": outcome,
    })))
}

async fn linux_reset_password(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(conn_q): Query<crate::conn_query::ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<LinuxResetPasswordReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browse_host_paths(&actor)?;
    let _vm_guard = manager.lock_vm(&name).await;
    let cfg = MachinaConfig::load();
    let guestkit_bin = cfg.libvirt.guestkit_agent_binary.clone();
    let disk = linux_offline_disk(manager, &actor, conn_q, &name).await?;
    let user = req.user;
    let password = req.password;
    let outcome = tokio::task::spawn_blocking(move || {
        machina_core::libvirt::linux_guestkit::reset_password_offline(
            &disk,
            &guestkit_bin,
            &user,
            &password,
        )
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("task failed: {e}"))))??;
    // Never include the plaintext password in audit details.
    log_audit_with_actor(&actor, "linux.reset-password", &name, "success");
    Ok(Json(serde_json::json!({
        "status": "ok",
        "vm": name,
        "result": outcome,
    })))
}

async fn linux_fix_fstab(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(conn_q): Query<crate::conn_query::ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browse_host_paths(&actor)?;
    let _vm_guard = manager.lock_vm(&name).await;
    let cfg = MachinaConfig::load();
    let guestkit_bin = cfg.libvirt.guestkit_agent_binary.clone();
    let disk = linux_offline_disk(manager, &actor, conn_q, &name).await?;
    let outcome = tokio::task::spawn_blocking(move || {
        machina_core::libvirt::linux_guestkit::fix_fstab_offline(&disk, &guestkit_bin)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("task failed: {e}"))))??;
    log_audit_with_actor(&actor, "linux.fix-fstab", &name, "success");
    Ok(Json(serde_json::json!({
        "status": "ok",
        "vm": name,
        "result": outcome,
    })))
}

async fn linux_set_hostname(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(conn_q): Query<crate::conn_query::ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<LinuxSetHostnameReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browse_host_paths(&actor)?;
    let _vm_guard = manager.lock_vm(&name).await;
    let cfg = MachinaConfig::load();
    let guestkit_bin = cfg.libvirt.guestkit_agent_binary.clone();
    let disk = linux_offline_disk(manager, &actor, conn_q, &name).await?;
    let hostname = req.hostname;
    let outcome = tokio::task::spawn_blocking(move || {
        machina_core::libvirt::linux_guestkit::set_hostname_offline(&disk, &guestkit_bin, &hostname)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("task failed: {e}"))))??;
    log_audit_with_actor(&actor, "linux.set-hostname", &name, "success");
    Ok(Json(serde_json::json!({
        "status": "ok",
        "vm": name,
        "result": outcome,
    })))
}

async fn ensure_guest_agent_channel_handler(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(conn_q): Query<crate::conn_query::ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browse_host_paths(&actor)?;
    // Held across the read-XML → attach sequence: two concurrent calls for the
    // same VM would otherwise both see "no channel" and both attach one,
    // racing on the domain (the loser's attach fails as "already exists"
    // even though a channel is in fact present).
    let _vm_guard = manager.lock_vm(&name).await;
    let vm = name.clone();
    let outcome = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        machina_core::libvirt::qga_channel::ensure_guest_agent_channel(conn, &vm)
    })
    .await?;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "vm": name,
        "channel": outcome,
    })))
}

async fn list_disk_images(
    State(manager): State<LibvirtManager>,
) -> Result<Json<extras::BrowseFilesResponse>, AppError> {
    let mgr = manager.clone();
    let res = tokio::task::spawn_blocking(move || mgr.with_conn(extras::list_disk_images))
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(res))
}

#[derive(Deserialize)]
struct BrowseDirQuery {
    /// Absolute directory on the hypervisor; omit or empty to open the first allowed root.
    path: Option<String>,
}

async fn browse_directory_handler(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(q): Query<BrowseDirQuery>,
) -> Result<Json<extras::BrowseDirResponse>, AppError> {
    require_browse_host_paths(&actor)?;
    let path = q.path.unwrap_or_default();
    let mgr = manager.clone();
    let res =
        tokio::task::spawn_blocking(move || mgr.with_conn(|c| extras::browse_directory(c, &path)))
            .await
            .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(res))
}

#[derive(Deserialize)]
struct DeleteImageQuery {
    path: String,
}

async fn delete_disk_image(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(q): Query<DeleteImageQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_destroy_vm(&actor)?;
    let path = q.path.trim().to_string();
    if path.is_empty() {
        return Err(AppError::from(LibvirtError::Invalid(
            "path is required".into(),
        )));
    }
    let mgr = manager.clone();
    let allowed_prefixes: Vec<String> = tokio::task::spawn_blocking(move || {
        mgr.with_conn(storage::disk_image_delete_allowed_prefixes)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    if !allowed_prefixes.iter().any(|p| path.starts_with(p)) {
        return Err(AppError::from(LibvirtError::Invalid(format!(
            "Path not in an allowed images directory: {path}"
        ))));
    }
    // Reject path traversal.
    if path.contains("..") {
        return Err(AppError::from(LibvirtError::Invalid(
            "Path traversal not allowed".into(),
        )));
    }
    // Only delete known disk image extensions.
    let ok_ext = path.ends_with(".qcow2")
        || path.ends_with(".raw")
        || path.ends_with(".img")
        || path.ends_with(".vmdk");
    if !ok_ext {
        return Err(AppError::from(LibvirtError::Invalid(format!(
            "File extension not allowed for deletion: {path}"
        ))));
    }
    match std::fs::remove_file(&path) {
        Ok(()) => {
            log_audit("delete-disk-image", &path, "ok");
            Ok(Json(
                serde_json::json!({ "status": "deleted", "path": path }),
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Json(
            serde_json::json!({ "status": "not_found", "path": path }),
        )),
        Err(e) => Err(AppError::from(LibvirtError::Operation(format!(
            "Failed to delete {path}: {e}"
        )))),
    }
}

#[derive(Debug, Deserialize)]
struct VirtBuilderListQuery {
    /// When true, bypass the in-memory cache and re-run `virt-builder --list --list-format json`.
    #[serde(default)]
    refresh: bool,
}

struct VirtBuilderCatalogMeta {
    allowed: bool,
    installed: bool,
    version: Option<String>,
    catalog_error: Option<String>,
}

fn virt_builder_catalog_json(
    index: &virt_builder::VirtBuilderIndex,
    cached: bool,
    cache_age_secs: Option<u64>,
    meta: VirtBuilderCatalogMeta,
) -> serde_json::Value {
    let names: Vec<String> = index.items.iter().map(|i| i.name.clone()).collect();
    serde_json::json!({
        "virt_builder_allowed": meta.allowed,
        "virt_builder_installed": meta.installed,
        "virt_builder_version": meta.version,
        "catalog_error": meta.catalog_error,
        "format_version": index.format_version,
        "source_uri": index.source_uri,
        "items": index.items,
        "templates": names,
        "cached": cached,
        "cache_age_secs": cache_age_secs,
    })
}

fn empty_virt_builder_index() -> virt_builder::VirtBuilderIndex {
    virt_builder::VirtBuilderIndex {
        format_version: 0,
        source_uri: None,
        items: Vec::new(),
    }
}

async fn list_virt_builder_templates(
    Query(q): Query<VirtBuilderListQuery>,
    State(_m): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let cfg = MachinaConfig::load();
    let allowed = cfg.libvirt.virt_builder_allowed;

    let (installed, version) = tokio::task::spawn_blocking(|| {
        let ins = virt_builder::virt_builder_installed();
        let ver = if ins {
            virt_builder::virt_builder_version_line().ok()
        } else {
            None
        };
        (ins, ver)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))?;

    if !allowed {
        return Ok(Json(virt_builder_catalog_json(
            &empty_virt_builder_index(),
            false,
            None,
            VirtBuilderCatalogMeta {
                allowed: false,
                installed,
                version,
                catalog_error: Some(
                    "virt-builder is disabled ([libvirt] virt_builder_allowed = false); use mkosi_workspace / mkosi build."
                        .into(),
                ),
            },
        )));
    }

    if !installed {
        return Ok(Json(virt_builder_catalog_json(
            &empty_virt_builder_index(),
            false,
            None,
            VirtBuilderCatalogMeta {
                allowed: true,
                installed: false,
                version: None,
                catalog_error: Some(
                    "virt-builder is not installed on this host (install libguestfs-tools or guestfs-tools)."
                        .into(),
                ),
            },
        )));
    }

    if !q.refresh {
        if let Ok(guard) = VIRT_BUILDER_INDEX_CACHE.lock() {
            if let Some(ref c) = *guard {
                let age = c.fetched_at.elapsed();
                if age < VIRT_BUILDER_LIST_CACHE_TTL {
                    return Ok(Json(virt_builder_catalog_json(
                        &c.index,
                        true,
                        Some(age.as_secs()),
                        VirtBuilderCatalogMeta {
                            allowed: true,
                            installed: true,
                            version: version.clone(),
                            catalog_error: None,
                        },
                    )));
                }
            }
        }
    }

    match tokio::task::spawn_blocking(virt_builder::list_builder_index).await {
        Ok(Ok(index)) => {
            if let Ok(mut g) = VIRT_BUILDER_INDEX_CACHE.lock() {
                *g = Some(VirtBuilderIndexCache {
                    fetched_at: Instant::now(),
                    index: index.clone(),
                });
            }
            Ok(Json(virt_builder_catalog_json(
                &index,
                false,
                None,
                VirtBuilderCatalogMeta {
                    allowed: true,
                    installed: true,
                    version,
                    catalog_error: None,
                },
            )))
        }
        Ok(Err(e)) => {
            let msg = e.to_string();
            Ok(Json(virt_builder_catalog_json(
                &empty_virt_builder_index(),
                false,
                None,
                VirtBuilderCatalogMeta {
                    allowed: true,
                    installed: true,
                    version,
                    catalog_error: Some(msg),
                },
            )))
        }
        Err(e) => Err(AppError::from(LibvirtError::Internal(format!(
            "Task failed: {e}"
        )))),
    }
}

async fn virt_builder_probe_template_handler(
    State(_m): State<LibvirtManager>,
    Path(template): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let cfg = MachinaConfig::load();
    if !cfg.libvirt.virt_builder_allowed {
        return Ok(Json(serde_json::json!({
            "virt_builder_allowed": false,
            "name_valid": false,
            "in_cached_catalog": false,
            "hint": "Enable [libvirt] virt_builder_allowed = true on the daemon.",
        })));
    }
    let t = template.trim().to_string();
    if let Err(e) = machina_core::validate::validate_virt_builder_os(&t) {
        return Ok(Json(serde_json::json!({
            "virt_builder_allowed": true,
            "name_valid": false,
            "in_cached_catalog": false,
            "hint": e.to_string(),
        })));
    }

    let in_cached = tokio::task::spawn_blocking(move || {
        if let Ok(guard) = VIRT_BUILDER_INDEX_CACHE.lock() {
            if let Some(ref c) = *guard {
                return c.index.items.iter().any(|i| i.name == t);
            }
        }
        false
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))?;

    let hint = if in_cached {
        serde_json::Value::Null
    } else {
        serde_json::json!("Name format is valid but this id is not in the server catalog cache; refresh the virt-builder catalog on Disk images or check spelling.")
    };

    Ok(Json(serde_json::json!({
        "virt_builder_allowed": true,
        "name_valid": true,
        "in_cached_catalog": in_cached,
        "hint": hint,
    })))
}

async fn list_virt_image_output_roots(
    State(manager): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = manager.clone();
    let prefixes = tokio::task::spawn_blocking(move || {
        mgr.with_conn(storage::disk_image_delete_allowed_prefixes)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    let tmpdir = build_precheck::effective_tmpdir();
    Ok(Json(serde_json::json!({
        "allowed_prefixes": prefixes,
        "effective_tmpdir": tmpdir.to_string_lossy(),
    })))
}

async fn virt_image_build_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Extension(vib_slots): Extension<std::sync::Arc<Semaphore>>,
    Query(conn_q): Query<ConnQuery>,
    Json(mut req): Json<virt_image_build::BuildDiskRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Spawns a root-privileged, potentially long-running virt-builder invocation.
    // The sibling async job endpoint (`jobs::post_virt_image_build_job`) gates on
    // write role and the configured concurrency limiter; this synchronous route
    // had neither, letting a read-only-role user (or any authenticated caller)
    // kick off unlimited concurrent builds.
    require_write(&actor, "vms:write")?;
    if !MachinaConfig::load().libvirt.virt_builder_allowed {
        return Err(AppError::from(LibvirtError::Invalid(
            "virt-builder / virt-image-build is disabled ([libvirt] virt_builder_allowed = false)"
                .into(),
        )));
    }

    let out_path = req.output.trim().to_string();
    if out_path.is_empty() {
        return Err(AppError::from(LibvirtError::Invalid(
            "output is required".into(),
        )));
    }

    let timeout_secs = MachinaConfig::load().libvirt.virt_image_build_timeout_secs;
    if req.timeout_secs == 0 && timeout_secs > 0 {
        req.timeout_secs = timeout_secs;
    }

    let _permit = vib_slots.acquire().await.map_err(|_| {
        AppError::from(LibvirtError::Internal(
            "virt-image-build concurrency limiter closed".into(),
        ))
    })?;

    let block = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        crate::virt_image_validate::validate_virt_image_build(conn, &req)?;
        virt_image_build::build_disk_image(&req).map_err(|e| LibvirtError::Operation(e.to_string()))
    })
    .await;

    match block {
        Ok(()) => {
            log_audit("virt-image-build", &out_path, "ok");
            Ok(Json(
                serde_json::json!({ "status": "ok", "path": out_path }),
            ))
        }
        Err(e) => {
            log_audit("virt-image-build", &out_path, "error");
            Err(e)
        }
    }
}

async fn list_mkosi_workspaces_handler(
    State(_m): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let workspaces = extras::list_mkosi_workspaces();
    Ok(Json(serde_json::json!(workspaces)))
}

async fn virt_builder_notes_handler(
    State(_m): State<LibvirtManager>,
    Path(template): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !MachinaConfig::load().libvirt.virt_builder_allowed {
        return Err(AppError::from(LibvirtError::Invalid(
            "virt-builder is disabled ([libvirt] virt_builder_allowed = false); use mkosi_workspace / mkosi build".into(),
        )));
    }
    let t = template.clone();
    let notes = tokio::task::spawn_blocking(move || virt_builder::template_notes(&t))
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(
        serde_json::json!({ "template": template, "notes": notes }),
    ))
}

// ── USB Passthrough ────────────────────────────────────────────────

async fn list_usb(State(_m): State<LibvirtManager>) -> Result<Json<serde_json::Value>, AppError> {
    let devices = extras::list_usb_devices()?;
    Ok(Json(serde_json::json!(devices)))
}

#[derive(Deserialize)]
struct UsbRequest {
    vendor_id: String,
    product_id: String,
}

async fn attach_usb_handler(
    Extension(actor): Extension<RequestActor>,
    State(m): State<LibvirtManager>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<UsbRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_usb_pci(&actor)?;
    let vid = req.vendor_id.clone();
    let pid = req.product_id.clone();
    let name2 = name.clone();
    spawn_libvirt_actor(m, Some(&actor), conn_q, move |conn| {
        extras::attach_usb(conn, &name2, &vid, &pid)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "attached", "name": name }),
    ))
}

async fn detach_usb_handler(
    Extension(actor): Extension<RequestActor>,
    State(m): State<LibvirtManager>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<UsbRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_usb_pci(&actor)?;
    let vid = req.vendor_id.clone();
    let pid = req.product_id.clone();
    let name2 = name.clone();
    spawn_libvirt_actor(m, Some(&actor), conn_q, move |conn| {
        extras::detach_usb(conn, &name2, &vid, &pid)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "detached", "name": name }),
    ))
}

// ── Cloud-init ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CloudInitRequest {
    hostname: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    ssh_key: String,
    #[serde(default)]
    output_path: String,
}

async fn generate_cloud_init(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<CloudInitRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let output_path = req.output_path.clone();
    let hostname = req.hostname.clone();
    let username = req.username.clone();
    let password = req.password.clone();
    let ssh_key = req.ssh_key.clone();
    let path = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        let default_dir = storage::primary_vm_disk_base_dir(conn)
            .unwrap_or_else(|| "/var/lib/libvirt/images".to_string());
        // `output_path` is otherwise an unchecked absolute path: without this,
        // any caller with vms:write could overwrite an arbitrary file the
        // daemon can write to, not just create an ISO in an images directory.
        if !output_path.trim().is_empty() {
            storage::assert_new_disk_output_parent_allowed(conn, &output_path)?;
        }
        let cfg = MachinaConfig::load().libvirt;
        extras::generate_cloud_init_iso(
            &output_path,
            &default_dir,
            &hostname,
            &username,
            &password,
            &ssh_key,
            Some(&cfg),
        )
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "created", "path": path }),
    ))
}

// ── VM Import ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ImportRequest {
    source: String,
    dest_name: String,
}

async fn import_disk(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<ImportRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // `source` is an arbitrary absolute path read straight off the hypervisor's
    // filesystem (see `extras::import_disk_image`), the same class of operation
    // every other host-path handler in this file gates on.
    require_browse_host_paths(&actor)?;
    let source = req.source.clone();
    let dest_name = req.dest_name.clone();
    let path = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        extras::import_disk_image(conn, &source, &dest_name)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "imported", "path": path }),
    ))
}

// ── Live Resize ────────────────────────────────────────────────────

async fn live_vcpus_handler(
    State(m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path((name, count)): Path<(String, u32)>,
) -> Result<Json<serde_json::Value>, AppError> {
    // The offline sibling (`vms::set_vcpus`) gates on write role; this live
    // hot-plug variant had no check at all, letting any authenticated (incl.
    // read-only) caller resize a running VM's vCPUs.
    require_write(&actor, "vms:write")?;
    if conn_q.is_fluxvm() {
        return Ok(Json(super::fluxvm::resize(&name, Some(count), None).await?));
    }
    let name2 = name.clone();
    spawn_libvirt_actor(m, Some(&actor), conn_q, move |conn| {
        extras::live_set_vcpus(conn, &name2, count)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "name": name, "vcpus": count, "live": true }),
    ))
}

async fn live_memory_handler(
    State(m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path((name, mb)): Path<(String, u64)>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Same gap as `live_vcpus_handler` above: the offline sibling requires
    // write role, this live hot-plug variant did not.
    require_write(&actor, "vms:write")?;
    if conn_q.is_fluxvm() {
        return Ok(Json(super::fluxvm::resize(&name, None, Some(mb)).await?));
    }
    let name2 = name.clone();
    let outcome = spawn_libvirt_actor(m, Some(&actor), conn_q, move |conn| {
        extras::live_set_memory(conn, &name2, mb)
    })
    .await?;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "name": name,
        "memory_mb": mb,
        "live": outcome.live_applied,
    })))
}

// ── DHCP Leases ────────────────────────────────────────────────────

async fn list_dhcp_leases(
    State(m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = spawn_libvirt_actor(m, Some(&actor), conn_q, extras::list_dhcp_leases).await?;
    Ok(Json(serde_json::json!(result)))
}

// ── Host System Stats ──────────────────────────────────────────────

async fn get_host_stats(
    State(_m): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let stats = extras::get_host_stats();
    Ok(Json(serde_json::json!(stats)))
}

async fn get_host_linux_observability(
    State(_m): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let obs = tokio::task::spawn_blocking(machina_core::host_linux_obs::gather_linux_observability)
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(serde_json::json!(obs)))
}

async fn get_host_linux_audit(
    State(_m): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let report =
        tokio::task::spawn_blocking(machina_core::linux_audit::gather_linux_audit_configured)
            .await
            .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(serde_json::json!(report)))
}

async fn get_host_filesystems(
    State(_m): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rows = tokio::task::spawn_blocking(extras::list_host_filesystems)
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(serde_json::json!(rows)))
}

#[derive(Deserialize)]
struct HostProcessesQuery {
    /// Max rows to return (1–100, default 20).
    #[serde(default)]
    limit: Option<u32>,
    /// `rss` (default) = highest memory; `cpu` = highest %CPU.
    #[serde(default)]
    sort: Option<String>,
}

fn host_top_process_order(q: &HostProcessesQuery) -> extras::HostTopProcessOrder {
    match q
        .sort
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("cpu" | "pcpu") => extras::HostTopProcessOrder::Cpu,
        _ => extras::HostTopProcessOrder::Rss,
    }
}

async fn get_host_processes(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<HostProcessesQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Each row includes the process's full `/proc/<pid>/cmdline` (see
    // `read_proc_cmdline` in core), which frequently leaks secrets passed as
    // CLI args (DB passwords, API keys). Every other host-insight handler in
    // this file gates on this; this one had no check at all, letting any
    // authenticated caller — including an API token — scrape it.
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    let limit = q.limit.unwrap_or(20);
    let order = host_top_process_order(&q);
    let rows = tokio::task::spawn_blocking(move || extras::list_host_top_processes(limit, order))
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(serde_json::json!(rows)))
}

#[derive(Deserialize)]
struct HostKillProcessBody {
    pid: u32,
    /// `TERM` (default) or `KILL`.
    #[serde(default)]
    signal: Option<String>,
}

async fn post_host_kill_process(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(body): Json<HostKillProcessBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    require_browse_host_paths(&actor)?;
    let pid = body.pid;
    let sig = body
        .signal
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("TERM")
        .to_string();
    let target = format!("pid={pid} signal={sig}");
    let sig_for_block = sig.clone();
    let res = tokio::task::spawn_blocking(move || extras::kill_host_process(pid, &sig_for_block))
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))?;
    match res {
        Ok(()) => {
            log_audit_with_actor(&actor, "host-process-kill", &target, "ok");
            Ok(Json(serde_json::json!({
                "ok": true,
                "pid": pid,
                "signal": sig,
            })))
        }
        Err(e) => {
            let msg = e.to_string();
            log_audit_with_actor(&actor, "host-process-kill", &target, &msg);
            Err(AppError::from(e))
        }
    }
}

#[derive(Deserialize)]
struct HostListLimitQuery {
    /// 1–500; defaults differ per handler.
    #[serde(default)]
    limit: Option<u32>,
}

async fn get_host_package_updates(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    let _permit = HOST_HEAVY_PROBE_SEM.acquire().await.map_err(|_| {
        AppError::from(LibvirtError::Internal(
            "host probe concurrency limiter closed".into(),
        ))
    })?;
    let res = tokio::task::spawn_blocking(host_platform::check_package_updates)
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(serde_json::json!(res)))
}

#[derive(Deserialize, Default)]
struct HostPackageUpgradeBody {
    /// When true, only simulates upgrade (no system changes); returns tool output in the same shape as a real run.
    #[serde(default)]
    dry_run: Option<bool>,
}

async fn post_host_package_upgrade(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(body): Json<HostPackageUpgradeBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    let _permit = HOST_PACKAGE_ACTION_SEM.acquire().await.map_err(|_| {
        AppError::from(LibvirtError::Internal(
            "host package action concurrency limiter closed".into(),
        ))
    })?;
    let dry = body.dry_run.unwrap_or(false);
    let res = if dry {
        tokio::task::spawn_blocking(host_platform::package_upgrade_preview)
            .await
            .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??
    } else {
        tokio::task::spawn_blocking(host_platform::package_upgrade)
            .await
            .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??
    };
    let audit_result = if res.ok { "ok" } else { "failed" };
    log_audit(
        if dry {
            "host-package-upgrade-preview"
        } else {
            "host-package-upgrade"
        },
        "host",
        &format!("{audit_result} exit={}", res.exit_code),
    );
    Ok(Json(serde_json::json!(res)))
}

async fn post_host_package_autoremove(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    let _permit = HOST_PACKAGE_ACTION_SEM.acquire().await.map_err(|_| {
        AppError::from(LibvirtError::Internal(
            "host package action concurrency limiter closed".into(),
        ))
    })?;
    let res = tokio::task::spawn_blocking(host_platform::package_autoremove)
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    let audit_result = if res.ok { "ok" } else { "failed" };
    log_audit(
        "host-package-autoremove",
        "host",
        &format!("{audit_result} exit={}", res.exit_code),
    );
    Ok(Json(serde_json::json!(res)))
}

#[derive(Deserialize)]
struct HostPackagesBody {
    /// Package names or pins (install/remove); max 32.
    packages: Vec<String>,
}

async fn post_host_package_install(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(body): Json<HostPackagesBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    let _permit = HOST_PACKAGE_ACTION_SEM.acquire().await.map_err(|_| {
        AppError::from(LibvirtError::Internal(
            "host package action concurrency limiter closed".into(),
        ))
    })?;
    let pkgs = body.packages;
    let target = pkgs.join(",").chars().take(240).collect::<String>();
    let res = tokio::task::spawn_blocking(move || host_platform::package_install(pkgs))
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    let audit_result = if res.ok { "ok" } else { "failed" };
    log_audit(
        "host-package-install",
        &target,
        &format!("{audit_result} exit={}", res.exit_code),
    );
    Ok(Json(serde_json::json!(res)))
}

#[derive(Deserialize)]
struct HostPackageRemoveBody {
    packages: Vec<String>,
    #[serde(default)]
    purge: Option<bool>,
}

async fn post_host_package_remove(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(body): Json<HostPackageRemoveBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    let _permit = HOST_PACKAGE_ACTION_SEM.acquire().await.map_err(|_| {
        AppError::from(LibvirtError::Internal(
            "host package action concurrency limiter closed".into(),
        ))
    })?;
    let pkgs = body.packages;
    let purge = body.purge.unwrap_or(false);
    let target = pkgs.join(",").chars().take(240).collect::<String>();
    let res = tokio::task::spawn_blocking(move || host_platform::package_remove(pkgs, purge))
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    let audit_result = if res.ok { "ok" } else { "failed" };
    log_audit(
        "host-package-remove",
        &target,
        &format!("{audit_result} exit={}", res.exit_code),
    );
    Ok(Json(serde_json::json!(res)))
}

async fn get_host_net_counters(
    State(_m): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rows = tokio::task::spawn_blocking(host_platform::list_net_dev_counters)
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(serde_json::json!(rows)))
}

#[derive(Deserialize)]
struct NetRatesQuery {
    /// Milliseconds between two `/proc/net/dev` reads (50–5000, default 1000).
    #[serde(default)]
    interval_ms: Option<u64>,
}

async fn get_host_net_rates(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<NetRatesQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    let _permit = HOST_HEAVY_PROBE_SEM.acquire().await.map_err(|_| {
        AppError::from(LibvirtError::Internal(
            "host probe concurrency limiter closed".into(),
        ))
    })?;
    let ms = q.interval_ms.unwrap_or(1000).clamp(50, 5000);
    let res = tokio::task::spawn_blocking(move || host_platform::list_net_dev_rates(ms))
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(serde_json::json!(res)))
}

async fn get_host_passwd_users(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<HostListLimitQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    let lim = q.limit.unwrap_or(150).clamp(1, 500) as usize;
    let rows = tokio::task::spawn_blocking(move || host_platform::list_passwd_entries(lim))
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(serde_json::json!(rows)))
}

async fn get_host_groups(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<HostListLimitQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    let lim = q.limit.unwrap_or(150).clamp(1, 500) as usize;
    let rows = tokio::task::spawn_blocking(move || host_platform::list_group_entries(lim))
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(serde_json::json!(rows)))
}

async fn get_host_security_summary(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    let s = tokio::task::spawn_blocking(host_platform::host_security_summary)
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(serde_json::json!(s)))
}

// ── Save VM as Template ────────────────────────────────────────────

#[derive(Deserialize)]
struct SaveTemplateRequest {
    template_name: String,
}

async fn save_template_handler(
    State(m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<SaveTemplateRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Writes a new template file under /var/lib/machina/templates referencing
    // this VM's disk as a backing image; every other handler that writes state
    // on the caller's behalf gates on write role, this one did not.
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let template_name = req.template_name.clone();
    spawn_libvirt_actor(m, Some(&actor), conn_q, move |conn| {
        extras::save_vm_as_template(conn, &name2, &template_name)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "saved", "name": name, "template": req.template_name }),
    ))
}

// ── Audit Log ──────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct AuditLogQuery {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    actor: Option<String>,
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn get_audit_log(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<AuditLogQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Its siblings below (`/audit/export`, `/audit/verify`) both require the
    // `audit:read` scope for API-token callers; this one read up to 10,000
    // audit events (actor, action, target) with no scope check at all.
    require_api_scope(&actor, "audit:read").map_err(AppError::from)?;
    let mut events = audit::load_audit_events(10_000);
    if let Some(ref a) = q.action {
        let a = a.to_lowercase();
        events.retain(|e| e.action.to_lowercase().contains(&a));
    }
    if let Some(ref ac) = q.actor {
        let ac = ac.to_lowercase();
        events.retain(|e| e.actor.to_lowercase().contains(&ac));
    }
    if let Some(ref s) = q.q {
        let s = s.to_lowercase();
        events.retain(|e| {
            e.action.to_lowercase().contains(&s)
                || e.target.to_lowercase().contains(&s)
                || e.result.to_lowercase().contains(&s)
                || e.actor.to_lowercase().contains(&s)
        });
    }
    if let Some(lim) = q.limit {
        let lim = lim.clamp(1, 10_000);
        if events.len() > lim {
            events.truncate(lim);
        }
    } else if events.len() > 500 {
        events.truncate(500);
    }
    Ok(Json(serde_json::json!(events)))
}

async fn export_audit_log(
    Extension(actor): Extension<RequestActor>,
) -> Result<impl axum::response::IntoResponse, AppError> {
    require_api_scope(&actor, "audit:read").map_err(AppError::from)?;
    use axum::http::header;
    let body = audit::export_audit_ndjson(100_000);
    Ok(([(header::CONTENT_TYPE, "application/x-ndjson")], body))
}

#[derive(Deserialize)]
struct AuditVerifyQuery {
    #[serde(default = "default_audit_verify_max")]
    max_lines: usize,
}

fn default_audit_verify_max() -> usize {
    50_000
}

async fn verify_audit_log_handler(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<AuditVerifyQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_api_scope(&actor, "audit:read").map_err(AppError::from)?;
    let report = audit::verify_audit_log(q.max_lines);
    Ok(Json(serde_json::json!(report)))
}

// ── Tags (all tags summary) ──────────────────────────────────────

async fn get_all_tags_handler(
    State(_m): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let map = extras::load_tags();
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for tags in map.values() {
        for tag in tags {
            *counts.entry(tag.clone()).or_insert(0) += 1;
        }
    }
    Ok(Json(serde_json::json!(counts)))
}

// ── PCI Passthrough ───────────────────────────────────────────────

async fn list_pci_handler(
    State(_m): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let devices = extras::list_pci_devices()?;
    Ok(Json(serde_json::json!(devices)))
}

// ── IOMMU Groups ─────────────────────────────────────────────────

async fn list_iommu_groups_handler(
    State(_m): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let groups = extras::list_iommu_groups()?;
    Ok(Json(serde_json::json!(groups)))
}

// ── VFIO Status ──────────────────────────────────────────────────

async fn vfio_status_handler(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(addr): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_usb_pci(&actor)?;
    // Normalize addr: "0000:01:00.0" -> "0000:01:00.0"
    let addr_clean: String = addr
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ':' || *c == '.')
        .collect();
    let driver_link = std::path::Path::new("/sys/bus/pci/devices")
        .join(&addr_clean)
        .join("driver");
    let (driver, vfio_bound) = match std::fs::read_link(&driver_link) {
        Ok(target) => {
            let name = target
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            let is_vfio = name == "vfio-pci";
            (name, is_vfio)
        }
        Err(_) => ("none".to_string(), false),
    };
    Ok(Json(serde_json::json!({
        "addr": addr_clean,
        "driver": driver,
        "vfio_bound": vfio_bound,
    })))
}

// ── Systemd Services ──────────────────────────────────────────────

async fn list_services_handler(
    State(_m): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let services = extras::list_services()?;
    Ok(Json(serde_json::json!(services)))
}

async fn service_action_handler(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path((name, action)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_destroy_vm(&actor)?;
    log_audit_with_actor(&actor, "service_action", &format!("{action} {name}"), "");
    extras::service_action(&name, &action)?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "service": name, "action": action }),
    ))
}

// ── System Logs ───────────────────────────────────────────────────

async fn get_logs_handler(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    let lines: u32 = params
        .get("lines")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    let priority = params.get("priority").map(|s| s.as_str());
    let unit = params.get("unit").map(|s| s.as_str());
    let boot = params.get("boot").and_then(|v| v.parse::<i32>().ok());
    let since = params.get("since").map(|s| s.as_str());
    let until = params.get("until").map(|s| s.as_str());
    let grep = params.get("grep").map(|s| s.as_str());
    let uid = params.get("uid").and_then(|v| v.parse::<u32>().ok());
    let pid = params.get("pid").and_then(|v| v.parse::<u32>().ok());
    let kernel_only = params
        .get("kernel")
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false);
    let entries = extras::get_journal_logs(
        lines,
        priority,
        unit,
        boot,
        since,
        until,
        grep,
        uid,
        pid,
        kernel_only,
    )?;
    Ok(Json(serde_json::json!(entries)))
}

async fn list_log_boots_handler(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    let boots = extras::get_journal_boots()?;
    Ok(Json(serde_json::json!(boots)))
}

// ── Host Shutdown/Reboot ──────────────────────────────────────────

async fn host_shutdown_handler(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_destroy_vm(&actor)?;
    log_audit_with_actor(&actor, "host_shutdown", "host", "");
    extras::host_shutdown()?;
    Ok(Json(serde_json::json!({ "status": "shutting_down" })))
}

async fn host_reboot_handler(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_destroy_vm(&actor)?;
    log_audit_with_actor(&actor, "host_reboot", "host", "");
    extras::host_reboot()?;
    Ok(Json(serde_json::json!({ "status": "rebooting" })))
}

// ── Host System Info ──────────────────────────────────────────────

/// Linux sysfs + DMI + `/proc/cpuinfo`, merged with libvirt node caps (inventory / audit).
async fn get_hardware_inventory_handler(
    State(manager): State<LibvirtManager>,
) -> Result<Json<HardwareInventoryReport>, AppError> {
    let mgr = manager.clone();
    let report = tokio::task::spawn_blocking(move || {
        let libvirt = mgr.with_conn(node::get_node_info).ok();
        gather_hardware_inventory_report(libvirt)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(report))
}

#[derive(Deserialize)]
struct InventoryHistoryQuery {
    limit: Option<usize>,
}

/// Newest JSON Lines from `/var/lib/machina/hardware-inventory.jsonl` (written by the periodic inventory task).
async fn get_hardware_inventory_history_handler(
    State(_manager): State<LibvirtManager>,
    Query(q): Query<InventoryHistoryQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let limit = q.limit.unwrap_or(80).clamp(1, 5000);
    let entries = load_inventory_history_entries(limit)?;
    Ok(Json(serde_json::json!({
        "path": inventory_history_jsonl_path().display().to_string(),
        "entries": entries,
    })))
}

async fn get_system_info_handler(
    State(_m): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let info = extras::get_system_info()?;
    Ok(Json(serde_json::json!(info)))
}

#[derive(Deserialize)]
struct SetHostnameRequest {
    hostname: String,
}

async fn set_hostname_handler(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<SetHostnameRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    log_audit_with_actor(&actor, "set_hostname", &req.hostname, "");
    extras::set_hostname(&req.hostname)?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "hostname": req.hostname }),
    ))
}

#[derive(Deserialize)]
struct SetTimezoneRequest {
    timezone: String,
}

async fn set_timezone_handler(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<SetTimezoneRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    log_audit_with_actor(&actor, "set_timezone", &req.timezone, "");
    extras::set_timezone(&req.timezone)?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "timezone": req.timezone }),
    ))
}

#[derive(Deserialize)]
struct CockpitSectionQuery {
    section: Option<String>,
}

async fn get_host_cockpit_handler(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<CockpitSectionQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_browser_session_for_host_insight(&actor).map_err(AppError::from)?;
    let section = q.section.as_deref().unwrap_or("all");
    let storage = if section == "all" || section == "storage" {
        Some(
            tokio::task::spawn_blocking(machina_core::host_cockpit::storage_inventory)
                .await
                .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))?
                .map_err(AppError::from)?,
        )
    } else {
        None
    };
    let network = if section == "all" || section == "network" {
        Some(
            tokio::task::spawn_blocking(machina_core::host_cockpit::network_inventory)
                .await
                .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))?
                .map_err(AppError::from)?,
        )
    } else {
        None
    };
    let system = if section == "all" || section == "system" {
        Some(
            tokio::task::spawn_blocking(machina_core::host_cockpit::system_inventory)
                .await
                .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))?
                .map_err(AppError::from)?,
        )
    } else {
        None
    };
    Ok(Json(
        serde_json::json!({ "storage": storage, "network": network, "system": system }),
    ))
}

#[derive(Deserialize)]
struct CockpitActionBody {
    action: String,
    #[serde(default)]
    payload: serde_json::Value,
}

async fn post_host_cockpit_action_handler(
    Extension(actor): Extension<RequestActor>,
    Json(body): Json<CockpitActionBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_destroy_vm(&actor)?;
    let result = tokio::task::spawn_blocking(move || match body.action.as_str() {
        "cockpit.firewalld.add_service" => {
            let zone = body
                .payload
                .get("zone")
                .and_then(|v| v.as_str())
                .unwrap_or("public");
            let service = body
                .payload
                .get("service")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LibvirtError::Invalid("service required".into()))?;
            machina_core::host_cockpit::firewalld_add_service(zone, service)
        }
        "cockpit.selinux.set_enforce" => {
            let enforcing = body
                .payload
                .get("enforcing")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            machina_core::host_cockpit::selinux_set_enforce(enforcing)
        }
        "cockpit.tuned.set_profile" => {
            let profile = body
                .payload
                .get("profile")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LibvirtError::Invalid("profile required".into()))?;
            machina_core::host_cockpit::tuned_set_profile(profile)
        }
        "cockpit.nm.create_bond" => {
            let name = body
                .payload
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LibvirtError::Invalid("name required".into()))?;
            let ifaces: Vec<String> = body
                .payload
                .get("interfaces")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            machina_core::host_cockpit::nm_create_bond(name, &ifaces)
        }
        "cockpit.nm.create_team" => {
            let name = body
                .payload
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LibvirtError::Invalid("name required".into()))?;
            let runner = body
                .payload
                .get("runner")
                .and_then(|v| v.as_str())
                .unwrap_or("loadbalance");
            let ifaces: Vec<String> = body
                .payload
                .get("interfaces")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            machina_core::host_cockpit::nm_create_team(name, &ifaces, runner)
        }
        "cockpit.nm.create_vlan" => {
            let name = body
                .payload
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let parent = body
                .payload
                .get("parent")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LibvirtError::Invalid("parent required".into()))?;
            let vlan_id = body
                .payload
                .get("vlan_id")
                .and_then(|v| v.as_u64())
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| LibvirtError::Invalid("vlan_id required".into()))?;
            machina_core::host_cockpit::nm_create_vlan(name, parent, vlan_id)
        }
        "cockpit.nm.create_wifi" => {
            let ssid = body
                .payload
                .get("ssid")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LibvirtError::Invalid("ssid required".into()))?;
            let password = body
                .payload
                .get("password")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            machina_core::host_cockpit::nm_create_wifi(ssid, password)
        }
        "cockpit.nm.create_wireguard" => {
            let name = body
                .payload
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LibvirtError::Invalid("name required".into()))?;
            let address = body
                .payload
                .get("address")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LibvirtError::Invalid("address required".into()))?;
            let private_key = body
                .payload
                .get("private_key")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let peer_public_key = body
                .payload
                .get("peer_public_key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LibvirtError::Invalid("peer_public_key required".into()))?;
            let endpoint = body
                .payload
                .get("endpoint")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LibvirtError::Invalid("endpoint required".into()))?;
            let allowed_ips = body
                .payload
                .get("allowed_ips")
                .and_then(|v| v.as_str())
                .unwrap_or("0.0.0.0/0");
            machina_core::host_cockpit::nm_create_wireguard(
                name,
                address,
                private_key,
                peer_public_key,
                endpoint,
                allowed_ips,
            )
        }
        "cockpit.packagekit.refresh" => machina_core::host_cockpit::packagekit_refresh(),
        "host.package.install" => {
            let pkgs: Vec<String> = body
                .payload
                .get("packages")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if pkgs.is_empty() {
                return Err(LibvirtError::Invalid("packages required".into()));
            }
            let res = machina_core::host_platform::package_install(pkgs)?;
            if !res.ok {
                let detail = res.stderr.trim();
                return Err(LibvirtError::Operation(if detail.is_empty() {
                    format!("{} failed (exit {})", res.command, res.exit_code)
                } else {
                    detail.lines().next().unwrap_or(detail).to_string()
                }));
            }
            let msg = res.stdout.trim();
            Ok(if msg.is_empty() {
                format!("{} succeeded", res.command)
            } else {
                msg.lines().last().unwrap_or(msg).to_string()
            })
        }
        "host.package.remove" => {
            let pkgs: Vec<String> = body
                .payload
                .get("packages")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if pkgs.is_empty() {
                return Err(LibvirtError::Invalid("packages required".into()));
            }
            let purge = body
                .payload
                .get("purge")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let res = machina_core::host_platform::package_remove(pkgs, purge)?;
            if !res.ok {
                let detail = res.stderr.trim();
                return Err(LibvirtError::Operation(if detail.is_empty() {
                    format!("{} failed (exit {})", res.command, res.exit_code)
                } else {
                    detail.lines().next().unwrap_or(detail).to_string()
                }));
            }
            let msg = res.stdout.trim();
            Ok(if msg.is_empty() {
                format!("{} succeeded", res.command)
            } else {
                msg.lines().last().unwrap_or(msg).to_string()
            })
        }
        other => Err(LibvirtError::Invalid(format!("unknown action: {other}"))),
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))?
    .map_err(AppError::from)?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "message": result }),
    ))
}

// ── Router ─────────────────────────────────────────────────────────

pub fn extras_routes() -> Router<LibvirtManager> {
    // The handler streams the body to disk and enforces its own
    // `[libvirt] iso_upload_max_gib` cap; axum's buffering default would reject a
    // multi-GiB ISO before the handler ever ran.
    let iso_upload = Router::new()
        .route("/browse/isos/upload", post(upload_iso))
        .layer(DefaultBodyLimit::disable());

    Router::new()
        .merge(iso_upload)
        // Browser
        .route("/browse/isos", get(list_isos))
        .route("/browse/isos/download", post(download_iso))
        .route(
            "/vms/{name}/guest-agent/install-media",
            post(install_guest_agent),
        )
        .route(
            "/vms/{name}/guest-agent/channel",
            post(ensure_guest_agent_channel_handler),
        )
        .route("/vms/{name}/windows/enable-rdp", post(enable_windows_rdp))
        .route("/vms/{name}/linux/enable-ssh", post(enable_linux_ssh))
        .route(
            "/vms/{name}/linux/inject-ssh-key",
            post(linux_inject_ssh_key),
        )
        .route(
            "/vms/{name}/linux/reset-password",
            post(linux_reset_password),
        )
        .route("/vms/{name}/linux/fix-fstab", post(linux_fix_fstab))
        .route("/vms/{name}/linux/set-hostname", post(linux_set_hostname))
        .route("/browse/dir", get(browse_directory_handler))
        .route("/browse/disks", get(list_disk_images))
        .route("/browse/disks/delete", delete(delete_disk_image))
        .route(
            "/browse/virt-image-output-roots",
            get(list_virt_image_output_roots),
        )
        .route("/browse/virt-builder", get(list_virt_builder_templates))
        .route(
            "/browse/virt-builder/probe/{template}",
            get(virt_builder_probe_template_handler),
        )
        .route("/browse/virt-image-build", post(virt_image_build_handler))
        .route(
            "/browse/virt-builder/notes/{template}",
            get(virt_builder_notes_handler),
        )
        .route(
            "/browse/mkosi-workspaces",
            get(list_mkosi_workspaces_handler),
        )
        // USB
        .route("/host/usb", get(list_usb))
        .route("/vms/{name}/usb/attach", post(attach_usb_handler))
        .route("/vms/{name}/usb/detach", post(detach_usb_handler))
        // Cloud-init
        .route("/cloud-init", post(generate_cloud_init))
        // Import
        .route("/import/disk", post(import_disk))
        // Live resize
        .route("/vms/{name}/live/vcpus/{count}", post(live_vcpus_handler))
        .route("/vms/{name}/live/memory/{mb}", post(live_memory_handler))
        // Audit
        .route("/audit", get(get_audit_log))
        .route("/audit/export", get(export_audit_log))
        .route("/audit/verify", get(verify_audit_log_handler))
        // Tags (per-VM tags are in vms.rs)
        .route("/tags", get(get_all_tags_handler))
        // PCI
        .route("/host/pci", get(list_pci_handler))
        // IOMMU
        .route("/host/iommu-groups", get(list_iommu_groups_handler))
        // VFIO status
        .route("/host/devices/{addr}/vfio-status", get(vfio_status_handler))
        // Host stats + DHCP
        .route("/host/stats", get(get_host_stats))
        .route(
            "/host/linux-observability",
            get(get_host_linux_observability),
        )
        .route("/host/linux-audit", get(get_host_linux_audit))
        .route("/host/filesystems", get(get_host_filesystems))
        .route("/host/processes", get(get_host_processes))
        .route("/host/processes/kill", post(post_host_kill_process))
        .route("/host/package-updates", get(get_host_package_updates))
        .route("/host/package-upgrade", post(post_host_package_upgrade))
        .route(
            "/host/package-autoremove",
            post(post_host_package_autoremove),
        )
        .route("/host/package-install", post(post_host_package_install))
        .route("/host/package-remove", post(post_host_package_remove))
        .route("/host/net-counters", get(get_host_net_counters))
        .route("/host/net-rates", get(get_host_net_rates))
        .route("/host/passwd-users", get(get_host_passwd_users))
        .route("/host/groups", get(get_host_groups))
        .route("/host/security-summary", get(get_host_security_summary))
        .route("/dhcp-leases", get(list_dhcp_leases))
        // Save as template
        .route("/vms/{name}/save-template", post(save_template_handler))
        // Systemd services
        .route("/services", get(list_services_handler))
        .route("/services/{name}/{action}", post(service_action_handler))
        // System logs
        .route("/logs", get(get_logs_handler))
        .route("/logs/boots", get(list_log_boots_handler))
        // Host shutdown/reboot
        .route("/host/shutdown", post(host_shutdown_handler))
        .route("/host/reboot", post(host_reboot_handler))
        // Host system info
        .route(
            "/host/hardware-inventory/history",
            get(get_hardware_inventory_history_handler),
        )
        .route(
            "/host/hardware-inventory",
            get(get_hardware_inventory_handler),
        )
        .route("/host/system-info", get(get_system_info_handler))
        .route("/host/hostname", post(set_hostname_handler))
        .route("/host/timezone", post(set_timezone_handler))
        .route("/host/cockpit", get(get_host_cockpit_handler))
        .route(
            "/host/cockpit/actions",
            post(post_host_cockpit_action_handler),
        )
}
