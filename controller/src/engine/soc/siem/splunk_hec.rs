// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde_json::json;
use crate::db::DbPool;

use super::{
    fetch_unexported_alerts, fetch_unexported_events, integration_err, integration_ok,
    mark_exported, AlertRow, EventRow, IntegrationRow,
};

pub async fn forward(
    pool: &DbPool,
    integ: &IntegrationRow,
    controller_id: &str,
) -> anyhow::Result<usize> {
    let url = integ
        .config_json
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .trim_end_matches('/');
    let token = integ
        .config_json
        .get("token")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if url.is_empty() || token.is_empty() {
        return Ok(0);
    }
    let index = integ
        .config_json
        .get("index")
        .and_then(|v| v.as_str())
        .unwrap_or("machina");
    let st_events = integ
        .config_json
        .get("sourcetype_events")
        .and_then(|v| v.as_str())
        .unwrap_or("machina:soc:ecs");
    let st_alerts = integ
        .config_json
        .get("sourcetype_alerts")
        .and_then(|v| v.as_str())
        .unwrap_or("machina:soc:alert");
    let host = integ
        .config_json
        .get("host")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(controller_id);

    let hec_url = if url.contains("/services/collector") {
        url.to_string()
    } else {
        format!("{url}/services/collector/event")
    };

    let insecure_tls = integ
        .config_json
        .get("insecure_tls")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .danger_accept_invalid_certs(insecure_tls)
        .build()?;

    let events = fetch_unexported_events(pool, integ.id, 200).await?;
    let mut event_exported = Vec::new();
    for ev in &events {
        let body = hec_event(ev, host, index, st_events);
        if post_hec(&client, &hec_url, token, &body).await.is_err() {
            integration_err(pool, integ.id, "hec event post failed").await?;
            if !event_exported.is_empty() {
                mark_exported(pool, integ.id, "event", &event_exported).await?;
            }
            return Ok(event_exported.len());
        }
        event_exported.push(ev.id);
    }

    let alerts = fetch_unexported_alerts(pool, integ.id, 50).await?;
    let mut alert_exported = Vec::new();
    for al in &alerts {
        let body = hec_alert(al, host, index, st_alerts);
        if post_hec(&client, &hec_url, token, &body).await.is_err() {
            integration_err(pool, integ.id, "hec alert post failed").await?;
            break;
        }
        alert_exported.push(al.id);
    }

    if !event_exported.is_empty() {
        mark_exported(pool, integ.id, "event", &event_exported).await?;
    }
    if !alert_exported.is_empty() {
        mark_exported(pool, integ.id, "alert", &alert_exported).await?;
    }
    if !event_exported.is_empty() || !alert_exported.is_empty() {
        integration_ok(pool, integ.id).await?;
    }
    Ok(event_exported.len() + alert_exported.len())
}

fn hec_event(ev: &EventRow, host: &str, index: &str, sourcetype: &str) -> serde_json::Value {
    let mut event = ev.ecs_json.clone();
    if let Some(obj) = event.as_object_mut() {
        obj.entry("message".to_string())
            .or_insert(json!(ev.summary));
        obj.entry("machina.severity".to_string())
            .or_insert(json!(ev.severity));
        obj.entry("machina.source".to_string())
            .or_insert(json!(ev.source));
    }
    json!({
        "time": ev.occurred_at.timestamp(),
        "host": host,
        "index": index,
        "sourcetype": sourcetype,
        "source": "machina:soc",
        "event": event,
    })
}

fn hec_alert(al: &AlertRow, host: &str, index: &str, sourcetype: &str) -> serde_json::Value {
    json!({
        "time": al.last_seen.timestamp(),
        "host": host,
        "index": index,
        "sourcetype": sourcetype,
        "source": "machina:soc",
        "event": {
            "@timestamp": al.last_seen.to_rfc3339(),
            "event.dataset": "machina.soc.alert",
            "event.kind": "alert",
            "message": al.title,
            "machina.alert.id": al.id.to_string(),
            "machina.alert.status": al.status,
            "machina.severity": al.severity,
            "detail": al.detail_json,
        },
    })
}

async fn post_hec(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    body: &serde_json::Value,
) -> anyhow::Result<()> {
    let auth = format!("Splunk {token}");
    let res = client
        .post(url)
        .header("Authorization", auth)
        .json(body)
        .send()
        .await?;
    let status = res.status();
    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!(
            "hec status {}: {}",
            status,
            text.chars().take(200).collect::<String>()
        );
    }
    Ok(())
}

pub async fn test_connection(
    config: &serde_json::Value,
    controller_id: &str,
) -> anyhow::Result<String> {
    let url = config.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let token = config.get("token").and_then(|v| v.as_str()).unwrap_or("");
    if url.is_empty() || token.is_empty() {
        anyhow::bail!("url and token required");
    }
    let hec_url = if url.contains("/services/collector") {
        url.to_string()
    } else {
        format!("{}/services/collector/event", url.trim_end_matches('/'))
    };
    let insecure_tls = config
        .get("insecure_tls")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .danger_accept_invalid_certs(insecure_tls)
        .build()?;
    let body = json!({
        "time": chrono::Utc::now().timestamp(),
        "host": controller_id,
        "sourcetype": "machina:soc:health",
        "event": { "message": "Machina SOC Splunk HEC connectivity test", "event.dataset": "machina.soc.health" },
    });
    post_hec(&client, &hec_url, token, &body).await?;
    Ok("Splunk HEC accepted test event".into())
}
