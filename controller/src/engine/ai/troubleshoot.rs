// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct TroubleshootRequest {
    pub vm_id: Option<Uuid>,
    pub vm_name: Option<String>,
    #[serde(default = "default_symptom")]
    pub symptom: String,
}

fn default_symptom() -> String {
    "slow".into()
}

#[derive(Debug, Serialize)]
pub struct CheckResult {
    pub domain: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct Finding {
    pub severity: String,
    pub message: String,
    pub domain: String,
}

#[derive(Debug, Serialize)]
pub struct DiagnosisReport {
    pub vm_id: String,
    pub vm_name: String,
    pub symptom: String,
    pub severity: String,
    pub checks: Vec<CheckResult>,
    pub findings: Vec<Finding>,
    pub recommended_actions: Vec<String>,
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

async fn resolve_vm(
    pool: &DbPool,
    vm_id: Option<Uuid>,
    vm_name: Option<&str>,
) -> anyhow::Result<(Uuid, String, Option<Uuid>, i64, i32, String)> {
    if let Some(id) = vm_id {
        let row: (String, Option<Uuid>, i64, i32, String) = crate::db::query_as(
            "SELECT name, host_id, memory_mib, vcpus, observed_state FROM vms WHERE id = ?",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;
        return Ok((id, row.0, row.1, row.2, row.3, row.4));
    }
    if let Some(name) = vm_name.filter(|n| !n.is_empty()) {
        let row: (Uuid, String, Option<Uuid>, i64, i32, String) = crate::db::query_as(
            "SELECT id, name, host_id, memory_mib, vcpus, observed_state FROM vms WHERE name LIKE ? ESCAPE '\\' LIMIT 1",
        )
        // No wildcards added — this LIKE is used for case-insensitive exact
        // match against a caller-supplied name, so only escape the input's
        // own `%`/`_` (common in real VM names) to stop it being
        // misinterpreted as a wildcard rather than a literal character.
        .bind(escape_like(name))
        .fetch_one(pool)
        .await?;
        return Ok(row);
    }
    anyhow::bail!("vm_id or vm_name required")
}

pub async fn diagnose(
    pool: &DbPool,
    req: &TroubleshootRequest,
) -> anyhow::Result<DiagnosisReport> {
    let (vid, vname, host_id, mem_alloc, vcpus, state) =
        resolve_vm(pool, req.vm_id, req.vm_name.as_deref()).await?;
    let symptom = req.symptom.to_lowercase();
    let mut checks = Vec::new();
    let mut findings = Vec::new();
    let mut actions = Vec::new();

    // CPU
    let cpu: Option<f64> = crate::db::query_scalar("SELECT cpu_percent FROM vm_metrics WHERE vm_id = ?")
        .bind(vid)
        .fetch_optional(pool)
        .await?;
    let cpu_status = match cpu {
        Some(c) if c >= 90.0 => "critical",
        Some(c) if c >= 70.0 => "warn",
        Some(_) => "ok",
        None => "unknown",
    };
    checks.push(CheckResult {
        domain: "cpu".into(),
        status: cpu_status.into(),
        detail: cpu
            .map(|c| format!("CPU {c:.1}% of {vcpus} vCPUs"))
            .unwrap_or_else(|| "No recent CPU metrics".into()),
    });
    if cpu_status == "critical" {
        findings.push(Finding {
            severity: "high".into(),
            message: format!("VM {vname} CPU saturated"),
            domain: "cpu".into(),
        });
        actions.push("Right-size vCPUs or investigate guest process load.".into());
    }

    // Memory / balloon
    let mem_used: Option<i64> =
        crate::db::query_scalar("SELECT memory_used_mib FROM vm_metrics WHERE vm_id = ?")
            .bind(vid)
            .fetch_optional(pool)
            .await?;
    let mem_ratio = mem_used.map(|u| u as f64 / mem_alloc.max(1) as f64);
    let mem_status = match mem_ratio {
        Some(r) if r >= 0.92 => "critical",
        Some(r) if r >= 0.75 => "warn",
        Some(_) => "ok",
        None => "unknown",
    };
    checks.push(CheckResult {
        domain: "memory".into(),
        status: mem_status.into(),
        detail: mem_used
            .map(|u| {
                format!(
                    "{u}/{mem_alloc} MiB ({:.0}%)",
                    (u as f64 / mem_alloc as f64) * 100.0
                )
            })
            .unwrap_or_else(|| format!("Allocated {mem_alloc} MiB — no guest metrics")),
    });
    if mem_status == "critical" || mem_status == "warn" {
        findings.push(Finding {
            severity: if mem_status == "critical" {
                "high"
            } else {
                "medium"
            }
            .into(),
            message: "Memory pressure — check balloon driver and host overcommit".into(),
            domain: "memory".into(),
        });
        actions.push("Increase memory allocation or migrate to less loaded host.".into());
    }
    checks.push(CheckResult {
        domain: "numa_balloon".into(),
        status: "info".into(),
        detail:
            "NUMA topology follows host layout; verify guest NUMA awareness if latency-sensitive."
                .into(),
    });

    // Disk
    let disk_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vm_disks WHERE vm_id = ?")
        .bind(vid)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let disk_io: Option<(i64, i64)> =
        crate::db::query_as("SELECT disk_read_iops, disk_write_iops FROM vm_metrics WHERE vm_id = ?")
            .bind(vid)
            .fetch_optional(pool)
            .await?;
    let disk_detail = match disk_io {
        Some((r, w)) if r + w > 5000 => format!(
            "{disk_count} disk(s); high I/O ({r} read / {w} write IOPS) — check storage pool latency"
        ),
        Some((r, w)) => format!("{disk_count} disk(s); {r} read / {w} write IOPS"),
        None => format!("{disk_count} attached disk(s) — no recent disk metrics"),
    };
    checks.push(CheckResult {
        domain: "disk".into(),
        status: if disk_count > 0 { "ok" } else { "warn" }.into(),
        detail: disk_detail,
    });
    if let Some((r, w)) = disk_io {
        if r + w > 8000 {
            findings.push(Finding {
                severity: "medium".into(),
                message: "Disk I/O saturation — correlate with native eBPF flows and pool backend"
                    .into(),
                domain: "disk".into(),
            });
        }
    }

    // Host pressure
    if let Some(hid) = host_id {
        let host: Option<(String, f64, i64, i64)> = crate::db::query_as(
            "SELECT hostname, cpu_percent, memory_used_mib, memory_total_mib FROM hosts WHERE id = ?",
        )
        .bind(hid)
        .fetch_optional(pool)
        .await?;
        if let Some((hname, hcpu, hmem_u, hmem_t)) = host {
            let hmem_ratio = if hmem_t > 0 {
                hmem_u as f64 / hmem_t as f64
            } else {
                0.0
            };
            let h_status = if hcpu >= 85.0 || hmem_ratio >= 0.9 {
                "critical"
            } else if hcpu >= 70.0 {
                "warn"
            } else {
                "ok"
            };
            checks.push(CheckResult {
                domain: "host_pressure".into(),
                status: h_status.into(),
                detail: format!(
                    "Host {hname}: CPU {hcpu:.0}%, memory {:.0}%",
                    hmem_ratio * 100.0
                ),
            });
            if h_status != "ok" {
                findings.push(Finding {
                    severity: "high".into(),
                    message: format!("Host {hname} under pressure — noisy neighbor risk"),
                    domain: "host".into(),
                });
                actions.push("Migrate VM or rebalance fleet.".into());
            }
        }
    } else {
        checks.push(CheckResult {
            domain: "host_pressure".into(),
            status: "unknown".into(),
            detail: "VM has no host assignment".into(),
        });
    }

    // Network (if unreachable symptom)
    if symptom.contains("network") || symptom.contains("reach") || symptom.contains("unreachable") {
        checks.push(CheckResult {
            domain: "network".into(),
            status: "info".into(),
            detail: "Run graph path analysis for peer connectivity".into(),
        });
    } else {
        checks.push(CheckResult {
            domain: "network".into(),
            status: "ok".into(),
            detail: "No connectivity symptom reported".into(),
        });
    }

    // VM Doctor score
    if let Ok(doctor) = crate::engine::vm_health::run_vm_health_check(pool, vid).await {
        checks.push(CheckResult {
            domain: "health_doctor".into(),
            status: if doctor.healthy { "ok" } else { "warn" }.into(),
            detail: format!(
                "Doctor score {}/100 — {} issues",
                doctor.score_numeric,
                doctor.checks_total - doctor.checks_passed
            ),
        });
        if !doctor.healthy {
            for issue in doctor.issues.iter().take(2) {
                findings.push(Finding {
                    severity: issue.severity.clone(),
                    message: issue.message.clone(),
                    domain: "doctor".into(),
                });
            }
        }
    }

    if let Some(stale) = crate::engine::vm_inventory::stale_libvirt_vm(pool, vid).await? {
        checks.push(CheckResult {
            domain: "inventory".into(),
            status: "critical".into(),
            detail: format!("Host has not listed this VM since {}", stale.last_seen),
        });
        findings.push(Finding {
            severity: "critical".into(),
            message: format!(
                "VM is recorded as {state} but its host no longer lists it — the domain is gone \
                 and consoles, keys and power actions will fail"
            ),
            domain: "inventory".into(),
        });
        actions.push(if stale.managed {
            "Recreate the VM or delete it from the VM page.".into()
        } else {
            "Approve the 'Remove stale VM' autopilot action.".into()
        });
    }

    if state != "running" && (symptom.contains("slow") || symptom.contains("unreachable")) {
        findings.push(Finding {
            severity: "critical".into(),
            message: format!("VM is {state} — performance checks may be stale"),
            domain: "lifecycle".into(),
        });
        actions.push("Start VM before performance diagnosis.".into());
    }

    if findings.is_empty() {
        findings.push(Finding {
            severity: "low".into(),
            message: "No dominant bottleneck detected — review guest OS metrics".into(),
            domain: "general".into(),
        });
        actions.push("Enable guest tools for richer metrics.".into());
    }

    let severity = findings
        .iter()
        .map(|f| f.severity.as_str())
        .max_by_key(|s| match *s {
            "critical" => 4,
            "high" => 3,
            "medium" => 2,
            _ => 1,
        })
        .unwrap_or("low")
        .to_string();

    Ok(DiagnosisReport {
        vm_id: vid.to_string(),
        vm_name: vname,
        symptom: req.symptom.clone(),
        severity,
        checks,
        findings,
        recommended_actions: actions,
    })
}

pub async fn verify_after_action(pool: &DbPool, vm_id: Uuid) -> anyhow::Result<String> {
    let req = TroubleshootRequest {
        vm_id: Some(vm_id),
        vm_name: None,
        symptom: "slow".into(),
    };
    let report = diagnose(pool, &req).await?;
    Ok(format!(
        "Post-action verify: {} checks, severity={}",
        report.checks.len(),
        report.severity
    ))
}
