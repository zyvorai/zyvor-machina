// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use chrono::{Duration, Utc};
use serde_json::Value;
use crate::db::DbPool;
use uuid::Uuid;

use super::playbooks;

#[derive(Debug, sqlx::FromRow)]
struct RuleRow {
    id: Uuid,
    name: String,
    severity: String,
    query_json: Value,
    throttle_minutes: i32,
}

pub async fn run_detection(pool: &DbPool) -> anyhow::Result<usize> {
    let rules: Vec<RuleRow> = crate::db::query_as(
        "SELECT id, name, severity, query_json, throttle_minutes FROM soc_detection_rules WHERE enabled = TRUE",
    )
    .fetch_all(pool)
    .await?;

    let mut fired = 0usize;
    for rule in rules {
        if evaluate_rule(pool, &rule).await? {
            fired += 1;
        }
    }
    Ok(fired)
}

async fn evaluate_rule(pool: &DbPool, rule: &RuleRow) -> anyhow::Result<bool> {
    let rule_type = rule
        .query_json
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("match");
    let window = rule
        .query_json
        .get("window_minutes")
        .and_then(|v| v.as_i64())
        .unwrap_or(60);
    let since = Utc::now() - Duration::minutes(window);

    let events: Vec<(Uuid, String, String, String, Value)> = crate::db::query_as(
        "SELECT id, source, severity, summary, ecs_json FROM soc_events
         WHERE occurred_at >= ? ORDER BY occurred_at DESC LIMIT 500",
    )
    .bind(since)
    .fetch_all(pool)
    .await?;

    let matched: Vec<(Uuid, String)> = events
        .into_iter()
        .filter(|(_, src, sev, _, ecs)| event_matches(&rule.query_json, src, sev, ecs))
        .map(|(id, _, _, summary, _)| (id, summary))
        .collect();

    let min_count = rule
        .query_json
        .get("min_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(1) as usize;

    let ok = match rule_type {
        "threshold" => matched.len() >= min_count,
        _ => !matched.is_empty(),
    };

    if !ok {
        return Ok(false);
    }

    let dedupe_key = format!("rule:{}:{}", rule.id, Utc::now().format("%Y-%m-%d-%H"));
    let title = format!("{} ({})", rule.name, matched.len());
    let event_ids: Value = serde_json::json!(matched.iter().map(|(id, _)| id).collect::<Vec<_>>());

    let alert_id = upsert_alert(
        pool,
        rule.id,
        &title,
        &rule.severity,
        &dedupe_key,
        event_ids,
        &serde_json::json!({ "rule": rule.name, "match_count": matched.len() }),
        rule.throttle_minutes,
    )
    .await?;

    if let Some(id) = alert_id {
        enqueue_notification(pool, &title, &rule.severity, id).await?;
        playbooks::run_playbooks_for_alert(pool, id).await?;
        return Ok(true);
    }
    Ok(false)
}

fn event_matches(query: &Value, source: &str, severity: &str, ecs: &Value) -> bool {
    let m = query.get("match").and_then(|v| v.as_object());
    let Some(m) = m else {
        return true;
    };

    if let Some(src) = m.get("source").and_then(|v| v.as_str()) {
        if source != src {
            return false;
        }
    }

    if let Some(sevs) = m.get("severity") {
        let list: Vec<String> = if let Some(arr) = sevs.as_array() {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_lowercase))
                .collect()
        } else if let Some(s) = sevs.as_str() {
            vec![s.to_lowercase()]
        } else {
            vec![]
        };
        if !list.is_empty() && !list.contains(&severity.to_lowercase()) {
            return false;
        }
    }

    if let Some(actions) = m.get("ecs.event.action") {
        let action = ecs
            .get("event.action")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let list: Vec<String> = if let Some(arr) = actions.as_array() {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_lowercase))
                .collect()
        } else if let Some(s) = actions.as_str() {
            vec![s.to_lowercase()]
        } else {
            vec![]
        };
        if !list.is_empty() && !list.iter().any(|a| action.to_lowercase().contains(a)) {
            return false;
        }
    }

    if let Some(cat) = m.get("category").and_then(|v| v.as_str()) {
        let ds = ecs
            .get("event.dataset")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if cat == "firewall" && !ds.contains("firewall") {
            return false;
        }
    }

    true
}

#[allow(clippy::too_many_arguments)]
async fn upsert_alert(
    pool: &DbPool,
    rule_id: Uuid,
    title: &str,
    severity: &str,
    dedupe_key: &str,
    event_ids: Value,
    detail: &Value,
    throttle_minutes: i32,
) -> anyhow::Result<Option<Uuid>> {
    let existing: Option<(Uuid,)> = crate::db::query_as(
        "SELECT id FROM soc_alerts WHERE dedupe_key = ? AND status IN ('open', 'acknowledged')
         AND last_seen > datetime('now', '-' || ? || ' minutes')",
    )
    .bind(dedupe_key)
    .bind(throttle_minutes.max(0))
    .fetch_optional(pool)
    .await?;

    if let Some((id,)) = existing {
        crate::db::query(
            "UPDATE soc_alerts SET last_seen = datetime('now'), event_count = event_count + 1, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(id)
        .execute(pool)
        .await?;
        return Ok(Some(id));
    }

    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO soc_alerts (id, rule_id, title, severity, dedupe_key, event_ids, detail_json)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(rule_id)
    .bind(title)
    .bind(severity)
    .bind(dedupe_key)
    .bind(event_ids)
    .bind(detail)
    .execute(pool)
    .await?;
    Ok(Some(id))
}

async fn enqueue_notification(
    pool: &DbPool,
    title: &str,
    severity: &str,
    alert_id: Uuid,
) -> anyhow::Result<()> {
    let payload = serde_json::json!({
        "title": title,
        "severity": severity,
        "alert_id": alert_id.to_string(),
        "source": "soc",
    });
    crate::db::query("INSERT INTO notification_outbox (id, kind, payload) VALUES (?, ?, ?)")
        .bind(Uuid::new_v4())
        .bind("soc.alert")
        .bind(payload)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn test_rule(
    pool: &DbPool,
    rule_id: Uuid,
    hours: i32,
) -> anyhow::Result<serde_json::Value> {
    let rule: RuleRow = crate::db::query_as(
        "SELECT id, name, severity, query_json, throttle_minutes FROM soc_detection_rules WHERE id = ?",
    )
    .bind(rule_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("detection rule {} not found", rule_id))?;

    let hours = hours.clamp(1, 720);
    let since = Utc::now() - Duration::hours(hours as i64);
    let events: Vec<(Uuid, String, String, Value)> = crate::db::query_as(
        "SELECT id, source, severity, ecs_json FROM soc_events WHERE occurred_at >= ? ORDER BY occurred_at DESC LIMIT 1000",
    )
    .bind(since)
    .fetch_all(pool)
    .await?;

    let matches: usize = events
        .iter()
        .filter(|(_, src, sev, ecs)| event_matches(&rule.query_json, src, sev, ecs))
        .count();

    Ok(serde_json::json!({
        "rule_id": rule_id,
        "rule_name": rule.name,
        "window_hours": hours,
        "match_count": matches,
        "would_fire": matches > 0,
    }))
}
