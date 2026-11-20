// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::convert::Infallible;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path as FsPath, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use axum::extract::{Extension, Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream;
use machina_core::{audit, AuditEvent, LibvirtError, LibvirtManager, MachinaConfig};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Semaphore;
use uuid::Uuid;
use virt::connect::Connect;
use virt_image_build::BuildDiskRequest;

use crate::auth::{require_write, RequestActor};
use crate::error::AppError;
use crate::job_registry::{JobDetail, JobRegistry, JobStatus, JobSummary};

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

async fn list_jobs(
    State(_manager): State<LibvirtManager>,
    Extension(jobs): Extension<std::sync::Arc<JobRegistry>>,
) -> Json<Vec<JobSummary>> {
    Json(jobs.list_summaries())
}

async fn get_job_handler(
    State(_manager): State<LibvirtManager>,
    Extension(jobs): Extension<std::sync::Arc<JobRegistry>>,
    Path(id): Path<String>,
) -> Result<Json<JobDetail>, AppError> {
    let uid = Uuid::parse_str(&id)
        .map_err(|_| AppError::from(LibvirtError::Invalid("invalid job id".into())))?;
    jobs.get_detail(&uid)
        .map(Json)
        .ok_or_else(|| AppError::from(LibvirtError::NotFound(format!("job not found: {id}"))))
}

async fn post_virt_image_build_job(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Extension(jobs): Extension<std::sync::Arc<JobRegistry>>,
    Extension(vib_slots): Extension<Arc<Semaphore>>,
    Json(mut req): Json<BuildDiskRequest>,
) -> Result<Json<Value>, AppError> {
    // Spawns a root-privileged, long-running virt-builder job. Every sibling
    // mutating handler elsewhere in this codebase gates on write role; this one
    // previously had no check at all, letting a read-only-role user (or any
    // authenticated caller) kick off image builds.
    require_write(&actor, "vms:write")?;
    if !MachinaConfig::load().libvirt.virt_builder_allowed {
        return Err(AppError::from(LibvirtError::Invalid(
            "virt-builder / virt-image-build is disabled ([libvirt] virt_builder_allowed = false)"
                .into(),
        )));
    }
    if req.output.trim().is_empty() {
        return Err(AppError::from(LibvirtError::Invalid(
            "output is required".into(),
        )));
    }

    let timeout_secs = MachinaConfig::load().libvirt.virt_image_build_timeout_secs;
    if req.timeout_secs == 0 && timeout_secs > 0 {
        req.timeout_secs = timeout_secs;
    }

    let id = jobs.start_virt_image_build(req.os.trim(), req.output.trim());
    let cfg = MachinaConfig::load();
    let libvirt_uri = cfg.libvirt.uri.clone();
    let jobs_bg = jobs.clone();
    let jobs_for_blocking = jobs_bg.clone();
    let req_bg = req.clone();
    let out_path = req.output.trim().to_string();
    let sem = vib_slots.clone();

    tokio::spawn(async move {
        let permit = match sem.acquire_owned().await {
            Ok(p) => p,
            Err(_) => {
                let msg = "virt-image-build concurrency limiter closed";
                log_audit("virt-image-build", &out_path, &format!("error: {msg}"));
                jobs_bg.fail(id, msg);
                return;
            }
        };

        let res = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let conn = Connect::open(Some(&libvirt_uri)).map_err(|e| {
                LibvirtError::Connection(format!(
                    "Failed to connect to libvirt ({}): {e}",
                    libvirt_uri
                ))
            })?;
            (|| {
                crate::virt_image_validate::validate_virt_image_build(&conn, &req_bg)?;
                virt_image_build::build_disk_image_with_logs(&req_bg, |line| {
                    jobs_for_blocking.append_log(id, line);
                })
                .map_err(|e| LibvirtError::Operation(e.to_string()))
            })()
        })
        .await;

        match res {
            Ok(Ok(())) => {
                log_audit("virt-image-build", &out_path, "ok");
                jobs_bg.complete_virt_image(id, &out_path);
            }
            Ok(Err(e)) => {
                let msg = e.to_string();
                log_audit("virt-image-build", &out_path, &format!("error: {msg}"));
                jobs_bg.fail(id, &msg);
            }
            Err(e) => {
                let msg = format!("Task failed: {e}");
                log_audit("virt-image-build", &out_path, &format!("error: {msg}"));
                jobs_bg.fail(id, &msg);
            }
        }
    });

    Ok(Json(json!({
        "id": id.to_string(),
        "status": "started",
        "message": "Subscribe to GET /jobs/{id}/stream for live logs or poll GET /jobs/{id}",
    })))
}

const PACKER_LINUX_SCRIPT: &str = "/usr/local/share/machina/packer/build-linux-image.sh";
const PACKER_WINDOWS_DOCKUR_SCRIPT: &str =
    "/usr/local/share/machina/packer/build-windows-dockur.sh";
const PACKER_GOLDEN_ROOT: &str = "/var/lib/machina/packer-builds";

fn packer_guest_is_windows_dockur(g: &str) -> bool {
    matches!(
        g,
        "win10" | "win11" | "windows-server-2022" | "windows-server-2025"
    )
}

fn packer_guest_allowed(g: &str) -> bool {
    packer_guest_is_windows_dockur(g)
        || matches!(
            g,
            "fedora43"
                | "ubuntu2204"
                | "ubuntu2404"
                | "ubuntu2504"
                | "ubuntu2510"
                | "ubuntu2604"
                | "debian12"
                | "debian13"
                | "almalinux9"
                | "rocky9"
                | "centos9stream"
                | "oraclelinux9"
        )
}

fn packer_script_for_guest(g: &str) -> &'static str {
    if packer_guest_is_windows_dockur(g) {
        PACKER_WINDOWS_DOCKUR_SCRIPT
    } else {
        PACKER_LINUX_SCRIPT
    }
}

#[derive(Debug, Deserialize)]
struct PackerGoldenBuildBody {
    guest: String,
}

async fn post_packer_golden_build_job(
    State(_manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Extension(jobs): Extension<std::sync::Arc<JobRegistry>>,
    Json(body): Json<PackerGoldenBuildBody>,
) -> Result<Json<Value>, AppError> {
    // Same rationale as post_virt_image_build_job: spawns a root-privileged
    // build (bash script + packer) and had no role gate at all.
    require_write(&actor, "vms:write")?;
    let guest = body.guest.trim().to_string();
    if guest.is_empty() {
        return Err(AppError::from(LibvirtError::Invalid(
            "guest is required".into(),
        )));
    }
    if !packer_guest_allowed(&guest) {
        return Err(AppError::from(LibvirtError::Invalid(format!(
            "unknown packer guest id: {guest}"
        ))));
    }

    let cfg = MachinaConfig::load();
    if packer_guest_is_windows_dockur(&guest) && !cfg.libvirt.dockur_windows_allowed {
        return Err(AppError::from(LibvirtError::Invalid(
            "dockur Windows golden builds are disabled ([libvirt] dockur_windows_allowed = false)"
                .into(),
        )));
    }

    let script = packer_script_for_guest(&guest);
    if !FsPath::new(script).is_file() {
        return Err(AppError::from(LibvirtError::Invalid(format!(
            "packer script not found: {script}"
        ))));
    }

    let dockur_disk_size = cfg.libvirt.dockur_disk_size.clone();
    let dockur_ram_size = cfg.libvirt.dockur_ram_size.clone();
    let dockur_cpu_cores = cfg.libvirt.dockur_cpu_cores.clone();

    let id = jobs.start_packer_golden_build(&guest);
    let jobs_bg = jobs.clone();
    let guest_bg = guest.clone();

    std::thread::spawn(move || {
        let root = PathBuf::from(PACKER_GOLDEN_ROOT).join(id.to_string());
        if let Err(e) = fs::create_dir_all(&root) {
            jobs_bg.fail(id, &format!("create work dir: {e}"));
            return;
        }
        let work_dir = root.join("work");
        if let Err(e) = fs::create_dir_all(&work_dir) {
            jobs_bg.fail(id, &format!("create work subdir: {e}"));
            return;
        }

        let mut cmd = Command::new("bash");
        cmd.arg(script)
            .arg(&guest_bg)
            .arg("work")
            .current_dir(&root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if packer_guest_is_windows_dockur(&guest_bg) {
            if !dockur_disk_size.trim().is_empty() {
                cmd.env("MACHINA_DOCKUR_DISK_SIZE", dockur_disk_size.trim());
            }
            if !dockur_ram_size.trim().is_empty() {
                cmd.env("MACHINA_DOCKUR_RAM_SIZE", dockur_ram_size.trim());
            }
            if !dockur_cpu_cores.trim().is_empty() {
                cmd.env("MACHINA_DOCKUR_CPU_CORES", dockur_cpu_cores.trim());
            }
        } else {
            cmd.env("MACHINA_SKIP_PACKER_INSTALL_DEPS", "1");
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                jobs_bg.fail(id, &format!("spawn packer script: {e}"));
                return;
            }
        };

        let stdout = match child.stdout.take() {
            Some(s) => s,
            None => {
                jobs_bg.fail(id, "no stdout from packer child");
                return;
            }
        };
        let stderr = match child.stderr.take() {
            Some(s) => s,
            None => {
                jobs_bg.fail(id, "no stderr from packer child");
                return;
            }
        };

        let (tx, rx) = mpsc::channel::<String>();
        let tx_out = tx.clone();
        let h_out = thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = tx_out.send(line);
            }
        });
        let tx_err = tx.clone();
        let h_err = thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let _ = tx_err.send(line);
            }
        });
        drop(tx);
        for line in rx {
            jobs_bg.append_log(id, &line);
        }
        let _ = h_out.join();
        let _ = h_err.join();

        let status = match child.wait() {
            Ok(s) => s,
            Err(e) => {
                jobs_bg.fail(id, &format!("wait packer: {e}"));
                return;
            }
        };

        if !status.success() {
            jobs_bg.fail(id, &format!("packer build exited with status {}", status));
            return;
        }

        let artifact = root
            .join("work")
            .join(format!("output-{}", guest_bg))
            .join(format!("{}.qcow2", guest_bg));
        if artifact.is_file() {
            let mut target = artifact.to_string_lossy().to_string();
            if packer_guest_is_windows_dockur(&guest_bg) {
                match machina_core::libvirt::extras::register_dockur_windows_golden(
                    &guest_bg, &artifact,
                ) {
                    Ok(stable) => {
                        jobs_bg.append_log(
                            id,
                            &format!(
                                "[machina] Registered template '{guest_bg}' → {}",
                                stable.display()
                            ),
                        );
                        jobs_bg.append_log(
                            id,
                            "[machina] Marketplace path ready for Platform Templates + KubeVirt qcow2-bundle",
                        );
                        target = stable.to_string_lossy().to_string();
                    }
                    Err(e) => {
                        jobs_bg.append_log(
                            id,
                            &format!(
                                "[machina] WARN: could not register dockur template (artifact kept): {e}"
                            ),
                        );
                    }
                }
            }
            jobs_bg.complete_packer_golden(id, &target);
        } else {
            jobs_bg.fail(
                id,
                &format!(
                    "build finished but qcow2 not found at {}",
                    artifact.display()
                ),
            );
        }
    });

    Ok(Json(json!({
        "id": id.to_string(),
        "status": "started",
        "message": "Subscribe to GET /jobs/{id}/stream for live logs or poll GET /jobs/{id}",
    })))
}

async fn job_stream_handler(
    State(_manager): State<LibvirtManager>,
    Extension(jobs): Extension<std::sync::Arc<JobRegistry>>,
    Path(id): Path<String>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>> + Send>, AppError> {
    let uid = Uuid::parse_str(&id)
        .map_err(|_| AppError::from(LibvirtError::Invalid("invalid job id".into())))?;
    if jobs.get_detail(&uid).is_none() {
        return Err(AppError::from(LibvirtError::NotFound(format!(
            "job not found: {id}"
        ))));
    }

    let jobs2 = jobs.clone();
    let job_uid = uid;
    let stream = stream::unfold(
        (
            tokio::time::interval(Duration::from_millis(420)),
            0usize,
            false,
        ),
        move |(mut interval, mut offset, mut terminal_sent)| {
            let jobs = jobs2.clone();
            let uid = job_uid;
            async move {
                if terminal_sent {
                    return None;
                }
                interval.tick().await;
                let Some(detail) = jobs.get_detail(&uid) else {
                    terminal_sent = true;
                    return Some((
                        Ok(Event::default()
                            .event("error")
                            .data("job no longer available")),
                        (interval, offset, terminal_sent),
                    ));
                };
                if offset < detail.logs.len() {
                    let chunk = detail.logs[offset..].join("\n");
                    offset = detail.logs.len();
                    return Some((
                        Ok(Event::default().data(chunk)),
                        (interval, offset, terminal_sent),
                    ));
                }
                match detail.summary.status {
                    JobStatus::Running => Some((
                        Ok(Event::default().comment("poll")),
                        (interval, offset, terminal_sent),
                    )),
                    JobStatus::Completed => {
                        terminal_sent = true;
                        let payload = json!({
                            "status": "completed",
                            "path": detail.summary.target_path,
                            "job": detail.summary,
                        })
                        .to_string();
                        Some((
                            Ok(Event::default().event("complete").data(payload)),
                            (interval, offset, terminal_sent),
                        ))
                    }
                    JobStatus::Failed => {
                        terminal_sent = true;
                        let msg = detail
                            .summary
                            .error
                            .clone()
                            .unwrap_or_else(|| "failed".into());
                        Some((
                            Ok(Event::default().event("error").data(msg)),
                            (interval, offset, terminal_sent),
                        ))
                    }
                }
            }
        },
    );

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(25))
            .text("keepalive"),
    ))
}

pub fn job_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/jobs", get(list_jobs))
        .route("/jobs/virt-image-build", post(post_virt_image_build_job))
        .route(
            "/jobs/packer-golden-build",
            post(post_packer_golden_build_job),
        )
        .route("/jobs/{id}/stream", get(job_stream_handler))
        .route("/jobs/{id}", get(get_job_handler))
}
