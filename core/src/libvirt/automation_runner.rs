// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Periodic automation: evaluate alerts, run schedules, snapshot schedules.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use chrono::TimeZone;
use tracing::{info, warn};

use super::automation::{
    fire_webhook, load_alert_rules, load_notification_channels, load_schedules,
    load_snapshot_schedules, save_alert, save_schedules, save_snapshot_schedules,
    send_notification, should_run_now, Alert, AlertRule, ScheduledAction, SnapshotSchedule,
};
use super::domain;
use super::extras::get_host_stats;
use super::snapshot;
use crate::state::CreateSnapshotRequest;
use crate::{LibvirtManager, VmMetrics};

static LAST_VM_CPU: Mutex<Option<HashMap<String, (Instant, u64)>>> = Mutex::new(None);

fn now_ts() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn alert_id() -> String {
    format!("alert-{}", chrono::Utc::now().timestamp_millis())
}

fn recent_unacked(rule_name: &str, within_mins: i64) -> bool {
    let alerts = super::automation::load_alerts();
    let cutoff = chrono::Local::now() - chrono::Duration::minutes(within_mins);
    alerts.iter().any(|a| {
        if a.acknowledged || a.rule_name != rule_name {
            return false;
        }
        chrono::NaiveDateTime::parse_from_str(&a.timestamp, "%Y-%m-%d %H:%M:%S")
            .ok()
            .and_then(|ndt| chrono::Local.from_local_datetime(&ndt).single())
            .map(|dt| dt > cutoff)
            .unwrap_or(true)
    })
}

fn emit_alert(rule: &AlertRule, message: &str, severity: &str) {
    if recent_unacked(&rule.name, 15) {
        return;
    }
    let alert = Alert {
        id: alert_id(),
        rule_name: rule.name.clone(),
        message: message.to_string(),
        severity: severity.to_string(),
        timestamp: now_ts(),
        acknowledged: false,
    };
    if let Err(e) = save_alert(&alert) {
        warn!("save alert: {e}");
        return;
    }
    let payload = serde_json::json!({
        "rule": rule.name,
        "message": message,
        "severity": severity,
        "timestamp": alert.timestamp,
    });
    fire_webhook("alert_fired", &payload);
    for ch in load_notification_channels() {
        if let Err(e) = send_notification(&ch, &format!("Machina alert: {}", rule.name), message) {
            warn!("notification {}: {e}", ch.id);
        }
    }
}

fn evaluate_host_rules(stats: &super::extras::HostStats, rules: &[AlertRule]) {
    for rule in rules {
        if !rule.enabled {
            continue;
        }
        let value = match rule.condition.as_str() {
            "cpu_percent" => stats.cpu_percent,
            "memory_percent" => stats.memory_percent,
            "disk_percent" => stats.disk_percent,
            _ => continue,
        };
        if value >= rule.threshold {
            emit_alert(
                rule,
                &format!(
                    "{} {:.1}% (threshold {:.1}%)",
                    rule.name, value, rule.threshold
                ),
                if value >= rule.threshold + 5.0 {
                    "critical"
                } else {
                    "warning"
                },
            );
        }
    }
}

fn vm_cpu_percent(metrics: &VmMetrics) -> f64 {
    let mut guard = LAST_VM_CPU.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    let pct = if let Some((t0, cpu0)) = map.get(&metrics.name) {
        let dt = now.duration_since(*t0).as_secs_f64();
        if dt > 0.1 {
            let dcpu = metrics.cpu_time_ns.saturating_sub(*cpu0) as f64 / 1_000_000_000.0;
            (dcpu / dt / metrics.vcpus.max(1) as f64 * 100.0).min(100.0)
        } else {
            0.0
        }
    } else {
        0.0
    };
    // Prune entries for VMs not seen in 10 min so the map stays bounded to live VMs
    // rather than accumulating a slot for every distinct VM name ever evaluated.
    map.retain(|_, (t, _)| now.saturating_duration_since(*t) < std::time::Duration::from_secs(600));
    map.insert(metrics.name.clone(), (now, metrics.cpu_time_ns));
    pct
}

fn evaluate_vm_rules(all_metrics: &[VmMetrics], rules: &[AlertRule]) {
    for rule in rules {
        if !rule.enabled {
            continue;
        }
        match rule.condition.as_str() {
            "vm_memory_percent" => {
                for m in all_metrics {
                    if m.memory_pct >= rule.threshold {
                        emit_alert(
                            rule,
                            &format!(
                                "VM {} memory {:.1}% (threshold {:.1}%)",
                                m.name, m.memory_pct, rule.threshold
                            ),
                            "warning",
                        );
                    }
                }
            }
            "vm_cpu_percent" => {
                for m in all_metrics {
                    let pct = vm_cpu_percent(m);
                    if pct >= rule.threshold {
                        emit_alert(
                            rule,
                            &format!(
                                "VM {} CPU {:.1}% (threshold {:.1}%)",
                                m.name, pct, rule.threshold
                            ),
                            "warning",
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

fn evaluate_vm_down(manager: &LibvirtManager, rules: &[AlertRule]) {
    let has_rule = rules.iter().any(|r| r.enabled && r.condition == "vm_down");
    if !has_rule {
        return;
    }
    let Ok(rows) = manager.with_conn(|conn| {
        let domains = conn
            .list_all_domains(0)
            .map_err(crate::LibvirtError::map_op("list domains"))?;
        let mut out = Vec::new();
        for domain in domains {
            let name = domain.get_name().unwrap_or_default();
            let autostart = domain.get_autostart().unwrap_or(false);
            if !autostart {
                continue;
            }
            let info = domain.get_info().ok();
            let state = info
                .map(|i| super::domain::state_to_string(i.state))
                .unwrap_or_else(|| "unknown".into());
            if state.eq_ignore_ascii_case("running") {
                continue;
            }
            out.push((name, state));
        }
        Ok::<_, crate::LibvirtError>(out)
    }) else {
        return;
    };
    for (name, state) in rows {
        let pseudo = AlertRule {
            id: "vm_down".into(),
            name: format!("vm_down:{name}"),
            condition: "vm_down".into(),
            threshold: 0.0,
            enabled: true,
        };
        emit_alert(
            &pseudo,
            &format!("VM {name} has autostart enabled but state is {state}"),
            "critical",
        );
    }
}

fn run_scheduled_action(manager: &LibvirtManager, action: &ScheduledAction) -> Result<(), String> {
    let vm = action.vm_name.clone();
    let act = action.action.to_ascii_lowercase();
    manager
        .with_conn(|conn| match act.as_str() {
            "start" => domain::start_vm(conn, &vm),
            "stop" => domain::stop_vm(conn, &vm),
            "shutdown" => domain::shutdown_vm(conn, &vm),
            "reboot" => domain::reboot_vm(conn, &vm),
            "snapshot" => {
                let req = CreateSnapshotRequest {
                    name: format!("sched-{}", chrono::Local::now().format("%Y%m%d-%H%M%S")),
                    description: "Scheduled snapshot".into(),
                    disk_only: true,
                    storage_mode: String::new(),
                    memory_snapshot: String::new(),
                    memory_file: String::new(),
                    external_disk_dir: String::new(),
                    external_memory_dir: String::new(),
                    disks: Vec::new(),
                    atomic: true,
                    reuse_external: false,
                    quiesce: false,
                };
                snapshot::create_snapshot(conn, &vm, &req)
            }
            other => Err(crate::LibvirtError::Invalid(format!(
                "unknown scheduled action: {other}"
            ))),
        })
        .map_err(|e| e.to_string())
}

fn run_snapshot_schedule(manager: &LibvirtManager, sched: &SnapshotSchedule) -> Result<(), String> {
    let vm = sched.vm_name.clone();
    let snap_name = format!("auto-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    manager
        .with_conn(|conn| {
            let req = CreateSnapshotRequest {
                name: snap_name.clone(),
                description: "Automatic snapshot schedule".into(),
                disk_only: true,
                storage_mode: String::new(),
                memory_snapshot: String::new(),
                memory_file: String::new(),
                external_disk_dir: String::new(),
                external_memory_dir: String::new(),
                disks: Vec::new(),
                atomic: true,
                reuse_external: false,
                quiesce: false,
            };
            snapshot::create_snapshot(conn, &vm, &req)?;
            if sched.retain_count > 0 {
                let mut snaps = snapshot::list_snapshots(conn, &vm)?;
                snaps.retain(|s| s.name.starts_with("auto-"));
                snaps.sort_by_key(|s| s.creation_time);
                while snaps.len() > sched.retain_count as usize {
                    if let Some(old) = snaps.first() {
                        let _ = snapshot::delete_snapshot(conn, &vm, &old.name);
                        snaps.remove(0);
                    } else {
                        break;
                    }
                }
            }
            Ok(())
        })
        .map_err(|e| e.to_string())
}

fn snapshot_due(sched: &SnapshotSchedule) -> bool {
    if sched.last_run.is_empty() {
        return true;
    }
    let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(&sched.last_run, "%Y-%m-%d %H:%M:%S")
    else {
        return true;
    };
    let Some(last) = chrono::Local.from_local_datetime(&ndt).single() else {
        return true;
    };
    let hours = sched.interval_hours.max(1) as i64;
    chrono::Local::now() >= last + chrono::Duration::hours(hours)
}

fn already_ran_this_minute(last_run: &str) -> bool {
    if last_run.is_empty() {
        return false;
    }
    let now_prefix = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    last_run.starts_with(&now_prefix)
}

/// One automation tick (alerts + schedules). Called from the daemon worker.
pub fn run_automation_tick(manager: &LibvirtManager) {
    let rules = load_alert_rules();
    let host_stats = get_host_stats();
    evaluate_host_rules(&host_stats, &rules);

    if let Ok(vm_metrics) = manager.merge_all_metrics() {
        evaluate_vm_rules(&vm_metrics, &rules);
    }
    evaluate_vm_down(manager, &rules);

    let mut schedules = load_schedules();
    let mut schedules_changed = false;
    for sched in schedules.iter_mut() {
        if !sched.enabled || !should_run_now(&sched.schedule) {
            continue;
        }
        if already_ran_this_minute(&sched.last_run) {
            continue;
        }
        match run_scheduled_action(manager, sched) {
            Ok(()) => {
                info!("scheduled action {} on {}", sched.action, sched.vm_name);
                sched.last_run = now_ts();
                schedules_changed = true;
                fire_webhook(
                    &format!("schedule_{}", sched.action),
                    &serde_json::json!({ "vm": sched.vm_name, "schedule_id": sched.id }),
                );
            }
            Err(e) => warn!("schedule {} failed: {e}", sched.id),
        }
    }
    if schedules_changed {
        let _ = save_schedules(&schedules);
    }

    let mut snap_schedules = load_snapshot_schedules();
    let mut snap_changed = false;
    for sched in snap_schedules.iter_mut() {
        if !sched.enabled || !snapshot_due(sched) {
            continue;
        }
        match run_snapshot_schedule(manager, sched) {
            Ok(()) => {
                info!("snapshot schedule for VM {}", sched.vm_name);
                sched.last_run = now_ts();
                snap_changed = true;
            }
            Err(e) => warn!("snapshot schedule {} failed: {e}", sched.id),
        }
    }
    if snap_changed {
        let _ = save_snapshot_schedules(&snap_schedules);
    }
}
