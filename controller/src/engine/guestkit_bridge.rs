// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// GuestKit (LGPL-3.0-or-later) — offline disk assurance bridge for Machina Migration Radar.

use std::path::{Path, PathBuf};

use guestkit::boot::BootTarget;
use guestkit::cli::commands::assurance::collect_assurance_data;
use guestkit::cli::migrate::plan::compute_migration_score;
use guestkit_job_spec::builder::JobBuilder;
use guestkit_job_spec::operations::GUESTKIT_INSPECT;
use guestkit_job_spec::JobDocument;
use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use crate::config::ControllerConfig;

#[derive(Debug, Clone, Serialize)]
pub struct GuestkitStatus {
    pub enabled: bool,
    pub library_version: String,
    pub worker_url: String,
    pub worker_reachable: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GuestkitDoctorReport {
    pub image_path: String,
    pub target: String,
    pub boot_score: f64,
    pub confidence: f64,
    pub summary: String,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub checks_passed: usize,
    pub checks_total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_cause: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GuestkitMigratePlanReport {
    pub image_path: String,
    pub target: String,
    pub migration_score: f64,
    pub boot_score: f64,
    pub estimated_downtime_minutes: u32,
    pub driver_injections: Vec<String>,
    pub required_changes: Vec<String>,
    pub licensing_warnings: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GuestkitJobSubmitResult {
    pub job_id: String,
    pub status: String,
    pub summary: String,
}

pub fn status(cfg: &ControllerConfig) -> GuestkitStatus {
    let worker_reachable = if cfg.guestkit_enabled {
        probe_worker_health(&cfg.guestkit_worker_url, cfg.guestkit_insecure_tls)
    } else {
        false
    };
    let summary = if !cfg.guestkit_enabled {
        "GuestKit bridge disabled — set GUESTKIT_ENABLED=1".into()
    } else if worker_reachable {
        "GuestKit library linked · worker reachable for distributed inspect jobs".into()
    } else {
        "GuestKit library linked · local doctor/migrate-plan available (worker offline)".into()
    };
    GuestkitStatus {
        enabled: cfg.guestkit_enabled,
        library_version: guestkit::VERSION.into(),
        worker_url: cfg.guestkit_worker_url.clone(),
        worker_reachable,
        summary,
    }
}

pub async fn doctor_disk(
    cfg: &ControllerConfig,
    image_path: &str,
    target: &str,
    explain: bool,
) -> anyhow::Result<GuestkitDoctorReport> {
    ensure_enabled(cfg)?;
    let path = resolve_image_path(image_path)?;
    let path_for_result = path.display().to_string();
    let target_enum = BootTarget::parse(target);
    let boot_report = tokio::task::spawn_blocking(move || {
        let (evidence, boot) = collect_assurance_data(&path, target_enum, false)?;
        let root = if explain {
            Some(guestkit::inference::infer_root_cause(&evidence, &boot).summary)
        } else {
            None
        };
        Ok::<_, anyhow::Error>((boot, root))
    })
    .await??;

    let (boot, root_cause) = boot_report;
    let checks_total = boot.checks.iter().filter(|c| c.weight > 0.0).count();
    let checks_passed = boot
        .checks
        .iter()
        .filter(|c| c.weight > 0.0 && c.passed)
        .count();

    Ok(GuestkitDoctorReport {
        image_path: path_for_result,
        target: boot.target.clone(),
        boot_score: boot.score,
        confidence: boot.confidence,
        summary: boot.summary.clone(),
        blockers: boot
            .blockers
            .iter()
            .map(|b| format!("{} — {}", b.title, b.message))
            .collect(),
        warnings: boot
            .warnings
            .iter()
            .map(|w| format!("{} — {}", w.title, w.message))
            .collect(),
        checks_passed,
        checks_total,
        root_cause,
    })
}

pub async fn migrate_plan_disk(
    cfg: &ControllerConfig,
    image_path: &str,
    target: &str,
) -> anyhow::Result<GuestkitMigratePlanReport> {
    ensure_enabled(cfg)?;
    let path = resolve_image_path(image_path)?;
    let path_for_result = path.display().to_string();
    let target_owned = target.to_string();
    let report = tokio::task::spawn_blocking(move || {
        let boot_target = BootTarget::parse(&target_owned);
        let (evidence, boot) = collect_assurance_data(&path, boot_target, false)?;
        let migration = compute_migration_score(&evidence, &boot, &target_owned);
        Ok::<_, anyhow::Error>((boot, migration))
    })
    .await??;

    let (boot, migration) = report;
    let summary = format!(
        "Migration to {target}: {:.0}% readiness · {:.0}% boot · ~{} min downtime",
        migration.score, boot.score, migration.estimated_downtime_minutes
    );

    Ok(GuestkitMigratePlanReport {
        image_path: path_for_result,
        target: target.to_string(),
        migration_score: migration.score,
        boot_score: boot.score,
        estimated_downtime_minutes: migration.estimated_downtime_minutes,
        driver_injections: migration.driver_injections,
        required_changes: migration.required_changes,
        licensing_warnings: migration.licensing_warnings,
        summary,
    })
}

pub async fn doctor_vm(
    cfg: &ControllerConfig,
    pool: &DbPool,
    disk_dir: &Path,
    vm_id: Uuid,
    target: &str,
    explain: bool,
) -> anyhow::Result<GuestkitDoctorReport> {
    let image = resolve_vm_disk_path(pool, disk_dir, vm_id).await?;
    doctor_disk(cfg, &image.to_string_lossy(), target, explain).await
}

pub async fn migrate_plan_vm(
    cfg: &ControllerConfig,
    pool: &DbPool,
    disk_dir: &Path,
    vm_id: Uuid,
    target: &str,
) -> anyhow::Result<GuestkitMigratePlanReport> {
    let image = resolve_vm_disk_path(pool, disk_dir, vm_id).await?;
    migrate_plan_disk(cfg, &image.to_string_lossy(), target).await
}

pub async fn resolve_vm_disk_path(
    pool: &DbPool,
    disk_dir: &Path,
    vm_id: Uuid,
) -> anyhow::Result<PathBuf> {
    let (name,): (String,) = crate::db::query_as("SELECT name FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("vm not found"))?;

    let disk_path: Option<String> = crate::db::query_scalar(
        "SELECT path FROM vm_disks WHERE vm_id = ? AND path IS NOT NULL AND path != '' ORDER BY name LIMIT 1",
    )
    .bind(vm_id)
    .fetch_optional(pool)
    .await?;

    if let Some(p) = disk_path {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
    }

    for ext in ["qcow2", "img", "raw"] {
        let candidate = disk_dir.join(format!("{name}.{ext}"));
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    anyhow::bail!(
        "no disk image found for VM {name} — set vm_disks.path or place image in {}",
        disk_dir.display()
    )
}

pub async fn submit_inspect_job(
    cfg: &ControllerConfig,
    image_path: &str,
    name: &str,
) -> anyhow::Result<GuestkitJobSubmitResult> {
    ensure_enabled(cfg)?;
    let path = resolve_image_path(image_path)?;
    let format = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("qcow2")
        .to_string();

    let job = JobBuilder::new()
        .generate_job_id()
        .operation(GUESTKIT_INSPECT)
        .name(name)
        .label("source", "machina")
        .label("product", "machina-migration-radar")
        .require_capability("guestkit.inspect")
        .payload(
            "guestkit.inspect.v1",
            serde_json::json!({
                "image": {
                    "path": path.display().to_string(),
                    "format": format,
                    "read_only": true
                },
                "options": {
                    "deep_scan": true,
                    "include_security": true
                }
            }),
        )
        .build()
        .map_err(|e| anyhow::anyhow!("job build: {e}"))?;

    submit_worker_job(cfg, job).await
}

pub async fn get_worker_job_status(
    cfg: &ControllerConfig,
    job_id: &str,
) -> anyhow::Result<serde_json::Value> {
    ensure_enabled(cfg)?;
    worker_get(cfg, &format!("/api/v1/jobs/{job_id}")).await
}

fn ensure_enabled(cfg: &ControllerConfig) -> anyhow::Result<()> {
    if cfg.guestkit_enabled {
        Ok(())
    } else {
        anyhow::bail!("GuestKit integration disabled (GUESTKIT_ENABLED=0)")
    }
}

fn resolve_image_path(image_path: &str) -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(image_path);
    if !path.exists() {
        anyhow::bail!("disk image not found: {image_path}");
    }
    if !path.is_file() {
        anyhow::bail!("path is not a file: {image_path}");
    }
    Ok(path)
}

async fn submit_worker_job(
    cfg: &ControllerConfig,
    job: JobDocument,
) -> anyhow::Result<GuestkitJobSubmitResult> {
    let client = worker_client(cfg)?;
    let url = format!(
        "{}/api/v1/jobs",
        cfg.guestkit_worker_url.trim_end_matches('/')
    );
    // Worker JobSubmitRequest flattens JobDocument at the root (not under "job").
    let body = serde_json::to_value(&job)?;
    let resp = client.post(&url).json(&body).send().await?;
    let status = resp.status();
    let out: serde_json::Value = resp.json().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("worker submit HTTP {status}: {out}");
    }
    let job_id = out
        .pointer("/data/job_id")
        .and_then(|v| v.as_str())
        .unwrap_or(&job.job_id)
        .to_string();
    Ok(GuestkitJobSubmitResult {
        job_id: job_id.clone(),
        status: "submitted".into(),
        summary: format!("GuestKit inspect job {job_id} queued on worker"),
    })
}

async fn worker_get(cfg: &ControllerConfig, path: &str) -> anyhow::Result<serde_json::Value> {
    let client = worker_client(cfg)?;
    let url = format!("{}{}", cfg.guestkit_worker_url.trim_end_matches('/'), path);
    let resp = client.get(&url).send().await?;
    let status = resp.status();
    let out: serde_json::Value = resp.json().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("worker GET HTTP {status}: {out}");
    }
    Ok(out)
}

fn worker_client(cfg: &ControllerConfig) -> anyhow::Result<reqwest::Client> {
    let mut b = reqwest::Client::builder().timeout(std::time::Duration::from_secs(120));
    if cfg.guestkit_insecure_tls {
        b = b.danger_accept_invalid_certs(true);
    }
    Ok(b.build()?)
}

fn probe_worker_health(base_url: &str, insecure_tls: bool) -> bool {
    let mut b = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(3));
    if insecure_tls {
        b = b.danger_accept_invalid_certs(true);
    }
    let Ok(client) = b.build() else { return false };
    let url = format!("{}/api/v1/health", base_url.trim_end_matches('/'));
    client
        .get(&url)
        .send()
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}
