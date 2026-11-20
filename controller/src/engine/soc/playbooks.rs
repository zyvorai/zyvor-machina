// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde_json::Value;
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)]
struct PlaybookRow {
    id: Uuid,
    name: String,
    trigger_json: Value,
    steps_json: Value,
}

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)]
struct AlertRow {
    id: Uuid,
    title: String,
    severity: String,
    rule_id: Option<Uuid>,
    rule_name: Option<String>,
}

pub async fn run_playbooks_for_alert(pool: &DbPool, alert_id: Uuid) -> anyhow::Result<()> {
    let alert: AlertRow = crate::db::query_as(
        "SELECT a.id, a.title, a.severity, a.rule_id, r.name AS rule_name
         FROM soc_alerts a LEFT JOIN soc_detection_rules r ON r.id = a.rule_id
         WHERE a.id = ?",
    )
    .bind(alert_id)
    .fetch_one(pool)
    .await?;

    let playbooks: Vec<PlaybookRow> = crate::db::query_as(
        "SELECT id, name, trigger_json, steps_json FROM soc_playbooks WHERE enabled = TRUE",
    )
    .fetch_all(pool)
    .await?;

    for pb in playbooks {
        if !trigger_matches(&pb.trigger_json, &alert) {
            continue;
        }
        let run_id = Uuid::new_v4();
        crate::db::query(
            "INSERT INTO soc_playbook_runs (id, playbook_id, alert_id, status) VALUES (?, ?, ?, 'running')",
        )
        .bind(run_id)
        .bind(pb.id)
        .bind(alert_id)
        .execute(pool)
        .await?;

        let mut results = Vec::new();
        let mut failed = false;
        if let Some(steps) = pb.steps_json.as_array() {
            for step in steps {
                let r = execute_step(pool, step, &alert).await;
                let ok = r.is_ok();
                results.push(serde_json::json!({
                    "step": step,
                    "ok": ok,
                    "detail": r.unwrap_or_else(|e| e.to_string()),
                }));
                if !ok {
                    failed = true;
                    break;
                }
            }
        }

        let status = if failed { "failed" } else { "completed" };
        crate::db::query(
            "UPDATE soc_playbook_runs SET status = ?, step_results = ?, finished_at = datetime('now') WHERE id = ?",
        )
        .bind(status)
        .bind(serde_json::json!(results))
        .bind(run_id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

fn trigger_matches(trigger: &Value, alert: &AlertRow) -> bool {
    let min_sev = trigger
        .get("min_severity")
        .and_then(|v| v.as_str())
        .unwrap_or("low");
    if !severity_at_least(&alert.severity, min_sev) {
        return false;
    }
    if let Some(names) = trigger.get("rule_names").and_then(|v| v.as_array()) {
        if !names.is_empty() {
            let alert_rule = alert.rule_name.as_deref().unwrap_or("");
            if !names.iter().any(|n| n.as_str() == Some(alert_rule)) {
                return false;
            }
        }
    }
    true
}

fn severity_at_least(actual: &str, min: &str) -> bool {
    let rank = |s: &str| match s.to_lowercase().as_str() {
        "critical" => 4,
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    };
    rank(actual) >= rank(min)
}

async fn execute_step(pool: &DbPool, step: &Value, alert: &AlertRow) -> anyhow::Result<String> {
    let step_type = step.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match step_type {
        "webhook" => {
            let url = resolve_webhook_url(pool, step).await?;
            let body = render_step_body(
                step.get("body").cloned().unwrap_or_else(|| {
                    serde_json::json!({
                        "alert_id": alert.id.to_string(),
                        "title": alert.title,
                        "severity": alert.severity,
                    })
                }),
                alert,
            );
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()?;
            let res = client.post(&url).json(&body).send().await?;
            if !res.status().is_success() {
                anyhow::bail!("webhook HTTP {}", res.status());
            }
            Ok("webhook delivered".into())
        }
        "notify" => {
            crate::db::query("INSERT INTO notification_outbox (id, kind, payload) VALUES (?, ?, ?)")
                .bind(Uuid::new_v4())
                .bind("soc.playbook")
                .bind(serde_json::json!({
                    "alert_id": alert.id,
                    "title": alert.title,
                    "severity": alert.severity,
                }))
                .execute(pool)
                .await?;
            Ok("notification enqueued".into())
        }
        _ => anyhow::bail!("unknown step type: {step_type}"),
    }
}

async fn resolve_webhook_url(pool: &DbPool, step: &Value) -> anyhow::Result<String> {
    if let Some(url) = step
        .get("url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Ok(url.to_string());
    }
    if step
        .get("url_from_setting")
        .and_then(|v| v.as_str())
        .is_some()
    {
        if let Ok(url) =
            crate::db::query_scalar::<_, String>("SELECT webhook_url FROM soc_settings WHERE id = 1")
                .fetch_one(pool)
                .await
        {
            let url = url.trim().to_string();
            if !url.is_empty() {
                return Ok(url);
            }
        }
        if let Ok(url) = std::env::var("MACHINA_SOC_WEBHOOK_URL") {
            let url = url.trim().to_string();
            if !url.is_empty() {
                return Ok(url);
            }
        }
    }
    anyhow::bail!("webhook url not configured")
}

fn render_step_body(body: Value, alert: &AlertRow) -> Value {
    let s = body.to_string();
    let rendered = s
        .replace("{{alert_id}}", &alert.id.to_string())
        .replace("{{title}}", &alert.title)
        .replace("{{severity}}", &alert.severity);
    serde_json::from_str(&rendered).unwrap_or(body)
}
