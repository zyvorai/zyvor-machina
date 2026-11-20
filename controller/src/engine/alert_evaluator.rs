// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//
// Day-2: threshold alert-rule evaluator. Periodically checks user-defined alert_rules
// against current vm_metrics and, when a rule is violated, emits a notification through
// the existing notification_outbox (delivered by webhook_worker). A per-rule cooldown
// prevents alert storms.

use uuid::Uuid;

use crate::state::AppState;

struct Rule {
    id: Uuid,
    name: String,
    metric: String,
    comparator: String,
    threshold: f64,
    severity: String,
    scope_project: String,
    scope_tag: String,
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = tick(&state).await {
                tracing::warn!("alert evaluator: {e:#}");
            }
        }
    });
}

async fn tick(state: &AppState) -> anyhow::Result<()> {
    // Only rules whose cooldown has elapsed (or never fired) are eligible this tick.
    let rows: Vec<(Uuid, String, String, String, f64, String, String, String)> = crate::db::query_as(
        "SELECT id, name, metric, comparator, threshold, severity, scope_project, scope_tag
         FROM alert_rules
         WHERE enabled = TRUE
           AND (last_fired_at IS NULL
                OR last_fired_at < datetime('now', printf('-%d minutes', cooldown_minutes)))",
    )
    .fetch_all(&state.pool)
    .await?;

    for (id, name, metric, comparator, threshold, severity, scope_project, scope_tag) in rows {
        let rule = Rule {
            id,
            name,
            metric,
            comparator,
            threshold,
            severity,
            scope_project,
            scope_tag,
        };
        let violations = evaluate_rule(state, &rule).await?;
        if !violations.is_empty() {
            fire(state, &rule, &violations).await?;
        }
    }
    Ok(())
}

/// Return (vm_name, metric_value) for every scoped VM currently violating the rule.
async fn evaluate_rule(state: &AppState, rule: &Rule) -> anyhow::Result<Vec<(String, f64)>> {
    if is_fleet_metric(&rule.metric) {
        let mut out = fleet_values(&state.pool, &rule.metric).await?;
        out.retain(|(_, v)| breaches(&rule.comparator, *v, rule.threshold));
        return Ok(out);
    }
    // mem_percent is derived from used vs configured memory; cpu_percent is direct.
    let base = "SELECT v.name,
                       m.cpu_percent AS cpu_percent,
                       CASE WHEN v.memory_mib > 0
                            THEN (m.memory_used_mib * 100.0 / v.memory_mib) ELSE 0 END AS mem_percent
                FROM vms v JOIN vm_metrics m ON m.vm_id = v.id
                WHERE v.lifecycle_phase NOT IN ('retired', 'deleting')";
    let rows: Vec<(String, f64, f64)> = if !rule.scope_project.is_empty()
        && !rule.scope_tag.is_empty()
    {
        crate::db::query_as(&format!(
                "{base} AND v.project = ? AND EXISTS (SELECT 1 FROM json_each(COALESCE(v.tags,'[]')) WHERE value = ?)"
            ))
            .bind(&rule.scope_project)
            .bind(&rule.scope_tag)
            .fetch_all(&state.pool)
            .await?
    } else if !rule.scope_project.is_empty() {
        crate::db::query_as(&format!("{base} AND v.project = ?"))
            .bind(&rule.scope_project)
            .fetch_all(&state.pool)
            .await?
    } else {
        crate::db::query_as(base).fetch_all(&state.pool).await?
    };

    let mut out = Vec::new();
    for (name, cpu_percent, mem_percent) in rows {
        let value = match rule.metric.as_str() {
            "mem_percent" => mem_percent,
            _ => cpu_percent, // default cpu_percent
        };
        if breaches(&rule.comparator, value, rule.threshold) {
            out.push((name, value));
        }
    }
    Ok(out)
}

async fn fire(state: &AppState, rule: &Rule, violations: &[(String, f64)]) -> anyhow::Result<()> {
    let payload = serde_json::json!({
        "rule_id": rule.id.to_string(),
        "rule": rule.name,
        "metric": rule.metric,
        "comparator": rule.comparator,
        "threshold": rule.threshold,
        "severity": rule.severity,
        "violations": violations.iter()
            .map(|(vm, v)| serde_json::json!({ "vm": vm, "value": v }))
            .collect::<Vec<_>>(),
        "count": violations.len(),
    });
    let kind = format!("alert.{}", rule.severity);
    // Route through the central dispatcher: in-app notification_outbox + signed webhooks +
    // notification channels (Slack/email/webhook), each filtered by its event list.
    crate::engine::webhooks::dispatch_webhooks(&state.pool, &kind, payload).await;
    crate::db::query("UPDATE alert_rules SET last_fired_at = datetime('now') WHERE id = ?")
        .bind(rule.id)
        .execute(&state.pool)
        .await?;
    state.emit_event(
        &kind,
        format!(
            "Alert '{}' fired: {} VM(s) {} {} {}",
            rule.name,
            violations.len(),
            rule.metric,
            rule.comparator,
            rule.threshold
        ),
    );
    Ok(())
}

/// Metrics about the fleet itself rather than about one VM (the default rules use these; scope is ignored).
pub const FLEET_METRICS: [&str; 4] = [
    "host_offline",
    "storage_pool_percent",
    "backup_failed_24h",
    "failed_task_burst",
];

pub fn is_fleet_metric(metric: &str) -> bool {
    FLEET_METRICS.contains(&metric)
}

fn breaches(comparator: &str, value: f64, threshold: f64) -> bool {
    match comparator {
        "lt" => value < threshold,
        _ => value > threshold, // default gt
    }
}

fn since(minutes: i64) -> String {
    (chrono::Utc::now() - chrono::Duration::minutes(minutes))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// (subject, value) for a fleet metric: one row per host / pool, or one row for a count.
async fn fleet_values(pool: &crate::db::DbPool, metric: &str) -> anyhow::Result<Vec<(String, f64)>> {
    Ok(match metric {
        // A host the controller marked offline that is not in maintenance (planned downtime is not an alert).
        "host_offline" => crate::db::query_as::<_, (String,)>(
            "SELECT hostname FROM hosts WHERE state = 'offline' AND maintenance_mode = FALSE ORDER BY hostname",
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|(h,)| (h, 1.0))
        .collect(),
        // Storage pool usage as the controller records it (used / capacity), not the host root filesystem.
        "storage_pool_percent" => crate::db::query_as::<_, (String, f64)>(
            "SELECT name, CAST(used_gib AS REAL) * 100.0 / CAST(capacity_gib AS REAL)
             FROM storage_pools WHERE capacity_gib > 0 ORDER BY name",
        )
        .fetch_all(pool)
        .await?,
        "backup_failed_24h" => {
            let n: i64 = crate::db::query_scalar(
                "SELECT COUNT(*) FROM backup_records WHERE status = 'failed' AND created_at >= ?",
            )
            .bind(since(24 * 60))
            .fetch_one(pool)
            .await?;
            vec![("backups failed in the last 24 h".into(), n as f64)]
        }
        // Failures an operator has not acknowledged (see api/task_failures.rs).
        "failed_task_burst" => {
            let n: i64 = crate::db::query_scalar(
                "SELECT COUNT(*) FROM tasks WHERE status = 'failed' AND acknowledged_at IS NULL AND created_at >= ?",
            )
            .bind(since(15))
            .fetch_one(pool)
            .await?;
            vec![("failed tasks in the last 15 min".into(), n as f64)]
        }
        _ => Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::test_support::test_pool;

    #[tokio::test]
    async fn default_rules_are_seeded_by_the_migration() {
        let pool = test_pool().await;
        let rows: Vec<(String, String)> =
            crate::db::query_as("SELECT name, metric FROM alert_rules WHERE enabled = TRUE ORDER BY name")
                .fetch_all(&pool)
                .await
                .unwrap();
        for m in FLEET_METRICS {
            assert!(rows.iter().any(|(_, metric)| metric == m), "missing default rule for {m}");
        }
        // The seeded ids must decode as UUIDs, as the list/evaluate queries read them.
        let ids: Vec<(Uuid,)> = crate::db::query_as("SELECT id FROM alert_rules").fetch_all(&pool).await.unwrap();
        assert!(ids.len() >= 4);
    }

    #[tokio::test]
    async fn fleet_metrics_read_the_fleet() {
        let pool = test_pool().await;
        crate::db::query("INSERT INTO hosts (id, hostname, state) VALUES (?, 'down1', 'offline')")
            .bind(Uuid::new_v4())
            .execute(&pool)
            .await
            .unwrap();
        crate::db::query("INSERT INTO hosts (id, hostname, state, maintenance_mode) VALUES (?, 'planned', 'offline', TRUE)")
            .bind(Uuid::new_v4())
            .execute(&pool)
            .await
            .unwrap();
        crate::db::query("INSERT INTO hosts (id, hostname, state) VALUES (?, 'up1', 'online')")
            .bind(Uuid::new_v4())
            .execute(&pool)
            .await
            .unwrap();
        let off = fleet_values(&pool, "host_offline").await.unwrap();
        assert_eq!(off, vec![("down1".to_string(), 1.0)]);

        crate::db::query("INSERT INTO storage_pools (id, name, capacity_gib, used_gib) VALUES (?, 'p-full', 100, 95)")
            .bind(Uuid::new_v4())
            .execute(&pool)
            .await
            .unwrap();
        crate::db::query("INSERT INTO storage_pools (id, name, capacity_gib, used_gib) VALUES (?, 'p-empty', 0, 0)")
            .bind(Uuid::new_v4())
            .execute(&pool)
            .await
            .unwrap();
        let pools = fleet_values(&pool, "storage_pool_percent").await.unwrap();
        assert_eq!(pools.len(), 1);
        assert!(breaches("gt", pools[0].1, 90.0));

        let tasks = fleet_values(&pool, "failed_task_burst").await.unwrap();
        assert_eq!(tasks[0].1, 0.0);
        assert!(!breaches("gt", tasks[0].1, 5.0));
        assert_eq!(fleet_values(&pool, "cpu_percent").await.unwrap().len(), 0);
    }

    #[test]
    fn comparators() {
        assert!(breaches("gt", 91.0, 90.0));
        assert!(!breaches("gt", 90.0, 90.0));
        assert!(breaches("lt", 1.0, 2.0));
    }
}
