// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};

use machina_core::{host_linux_obs, host_virt, linux_audit, LibvirtManager};
use serde_json::json;

async fn health_check(State(manager): State<LibvirtManager>) -> impl IntoResponse {
    let alive = tokio::task::spawn_blocking(move || {
        manager
            .with_conn(|conn| {
                conn.get_hostname()
                    .map_err(|e| machina_core::LibvirtError::Connection(e.to_string()))
            })
            .is_ok()
    })
    .await
    .unwrap_or(false);

    let status = if alive {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status,
        Json(serde_json::json!({
            "status": if alive { "healthy" } else { "unhealthy" },
            "libvirt": alive,
        })),
    )
}

async fn host_virtualization() -> Json<serde_json::Value> {
    Json(
        serde_json::to_value(host_virt::virtualization_status())
            .unwrap_or_else(|_| serde_json::json!({ "error": "serialization_failed" })),
    )
}

async fn libvirt_summary(State(manager): State<LibvirtManager>) -> Json<serde_json::Value> {
    Json(manager.api_connection_summary())
}

fn df_use_percent(path: &str) -> Option<u32> {
    let out = std::process::Command::new("df")
        .args(["-P", path])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .nth(1)?
        .to_string();
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 5 {
        return None;
    }
    cols[4].trim_end_matches('%').parse().ok()
}

async fn host_problems() -> Json<serde_json::Value> {
    let mut items: Vec<serde_json::Value> = Vec::new();
    let v = host_virt::virtualization_status();
    if !v.cpu_virt_supported {
        items.push(json!({
            "id": "cpu_virt",
            "severity": "critical",
            "title": "Hardware virtualization (vmx/svm) not detected",
            "detail": v.hint,
            "doc_url": "https://wiki.archlinux.org/title/KVM",
        }));
    }
    if v.cpu_virt_supported && !v.kvm_device_present {
        items.push(json!({
            "id": "kvm_device",
            "severity": "warning",
            "title": "/dev/kvm missing or not accessible",
            "detail": "Load the kvm module and check permissions for the machina-daemon user.",
            "doc_url": null,
        }));
    }
    if !v.libvirt_system_socket_present && !v.libvirt_session_socket_present {
        items.push(json!({
            "id": "libvirt_socket",
            "severity": "critical",
            "title": "No libvirt daemon socket",
            "detail": v.hint,
            "doc_url": null,
        }));
    }
    for path in ["/var/lib/libvirt", "/var/lib/machina", "/"] {
        if let Some(pct) = df_use_percent(path) {
            if pct >= 90 {
                let sev = if pct >= 98 { "critical" } else { "warning" };
                items.push(json!({
                    "id": format!("disk_pressure_{}", path.trim_matches('/').replace('/', "_")),
                    "severity": sev,
                    "title": format!("Disk space high on {path}"),
                    "detail": format!("df reports about {pct}% used — free space before provisioning."),
                    "doc_url": null,
                }));
            }
        }
    }
    if let Ok(obs) = host_linux_obs::gather_linux_observability() {
        if obs.pressure.available {
            if obs.pressure.memory.some >= 10.0 {
                items.push(json!({
                    "id": "psi_memory",
                    "severity": if obs.pressure.memory.some >= 25.0 { "critical" } else { "warning" },
                    "title": "Memory pressure (PSI)",
                    "detail": format!("some={:.1}% full={:.1}% — host is stalling on memory.", obs.pressure.memory.some, obs.pressure.memory.full),
                    "doc_url": null,
                }));
            }
            if obs.pressure.io.some >= 15.0 {
                items.push(json!({
                    "id": "psi_io",
                    "severity": "warning",
                    "title": "I/O pressure (PSI)",
                    "detail": format!("some={:.1}% — block I/O contention on the hypervisor.", obs.pressure.io.some),
                    "doc_url": null,
                }));
            }
        }
        for smart in obs.smart {
            if smart.probed && !smart.passed {
                items.push(json!({
                    "id": format!("smart_{}", smart.device),
                    "severity": "critical",
                    "title": format!("SMART health failed on {}", smart.device),
                    "detail": smart.summary,
                    "doc_url": null,
                }));
            }
        }
        if obs.cgroup.available {
            if let (Some(cur), Some(max)) =
                (obs.cgroup.memory_current_bytes, obs.cgroup.memory_max_bytes)
            {
                if max > 0 {
                    let pct = (cur as f64 / max as f64) * 100.0;
                    if pct >= 90.0 {
                        items.push(json!({
                            "id": "cgroup_memory",
                            "severity": if pct >= 98.0 { "critical" } else { "warning" },
                            "title": "Daemon cgroup memory high",
                            "detail": format!(
                                "machina-daemon cgroup ({}) at {:.0}% of memory.max",
                                obs.cgroup.unified_path, pct
                            ),
                            "doc_url": null,
                        }));
                    }
                }
            }
        }
    }
    if let Some(ts) = crate::automation_worker::automation_last_tick_unix() {
        let age = chrono::Utc::now().timestamp() - ts;
        if age > 300 {
            items.push(json!({
                "id": "automation_worker_stale",
                "severity": "warning",
                "title": "Automation worker stale",
                "detail": format!("Last tick was {age}s ago (expected every ~60s)."),
                "doc_url": "docs/guides/integrations.md",
            }));
        }
    }
    if let Ok(rep) = linux_audit::gather_linux_audit_configured() {
        if rep.available {
            let th = linux_audit::health_avc_threshold();
            if th > 0 && rep.avc_count >= th {
                items.push(json!({
                    "id": "linux_audit_avc",
                    "severity": "warning",
                    "title": "SELinux/AppArmor denials (auditd)",
                    "detail": format!(
                        "{} AVC event(s) in recent audit log (source: {}, threshold {}).",
                        rep.avc_count, rep.source, th
                    ),
                    "doc_url": null,
                }));
            }
        }
    }
    Json(json!({ "items": items }))
}

async fn host_libvirt_boot_status() -> Json<serde_json::Value> {
    let issue = host_virt::libvirt_boot_autostart_issue();
    Json(match issue {
        Some(i) => json!({
            "needs_attention": true,
            "detail": i.detail,
            "systemd_unit": i.systemd_unit,
        }),
        None => json!({
            "needs_attention": false,
            "detail": null,
            "systemd_unit": null,
        }),
    })
}

const OPENAPI_JSON: &str = include_str!("../../../docs/openapi-daemon.json");

async fn openapi_spec() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/json")], OPENAPI_JSON)
}

pub fn health_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/health", get(health_check))
        .route("/health/problems", get(host_problems))
        .route("/host/virtualization", get(host_virtualization))
        .route("/host/libvirt-boot", get(host_libvirt_boot_status))
        .route("/libvirt/summary", get(libvirt_summary))
        .route("/openapi.json", get(openapi_spec))
}
