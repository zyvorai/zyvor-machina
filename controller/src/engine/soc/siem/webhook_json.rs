// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Generic JSON webhook forwarder: POSTs batches of ECS events as NDJSON to any
//! endpoint you run (Loki push proxy, Vector, Fluent Bit, Wazuh, Graylog, a
//! script). Nothing leaves the site unless the operator points it somewhere.

use crate::db::DbPool;
use serde_json::json;

use super::{
    fetch_unexported_events, integration_err, integration_ok, mark_exported, EventRow,
    IntegrationRow,
};

const BATCH: i64 = 200;

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
        .trim();
    if url.is_empty() {
        return Ok(0);
    }
    let client = build_client(&integ.config_json, 30)?;
    let events = fetch_unexported_events(pool, integ.id, BATCH).await?;
    if events.is_empty() {
        return Ok(0);
    }

    let mut body = String::new();
    let mut ids = Vec::with_capacity(events.len());
    for ev in &events {
        body.push_str(&serde_json::to_string(&event_doc(ev, controller_id))?);
        body.push('\n');
        ids.push(ev.id);
    }

    let mut req = client
        .post(url)
        .header("Content-Type", "application/x-ndjson")
        .body(body);
    if let Some(token) = integ
        .config_json
        .get("bearer_token")
        .and_then(|v| v.as_str())
        .filter(|t| !t.is_empty())
    {
        req = req.bearer_auth(token);
    }
    let res = req.send().await?;
    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        integration_err(pool, integ.id, &format!("webhook {status}: {text}")).await?;
        anyhow::bail!("webhook forward failed: {status}");
    }
    mark_exported(pool, integ.id, "event", &ids).await?;
    integration_ok(pool, integ.id).await?;
    Ok(ids.len())
}

fn build_client(config: &serde_json::Value, timeout_secs: u64) -> anyhow::Result<reqwest::Client> {
    let insecure_tls = config
        .get("insecure_tls")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Ok(reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .danger_accept_invalid_certs(insecure_tls)
        .build()?)
}

fn event_doc(ev: &EventRow, controller_id: &str) -> serde_json::Value {
    let mut doc = ev.ecs_json.clone();
    if let Some(obj) = doc.as_object_mut() {
        obj.insert("message".into(), json!(ev.summary));
        obj.insert("@timestamp".into(), json!(ev.occurred_at.to_rfc3339()));
        obj.insert("machina_controller".into(), json!(controller_id));
        obj.insert("event_source".into(), json!(ev.source));
        obj.insert("severity".into(), json!(ev.severity));
    }
    doc
}

/// Sends one empty-batch probe (`{"test":true}`) so a bad URL or token shows up
/// before any real event is forwarded.
pub async fn test_connection(config: &serde_json::Value) -> anyhow::Result<String> {
    let url = config
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if url.is_empty() {
        anyhow::bail!("url required");
    }
    let client = build_client(config, 10)?;
    let mut req = client
        .post(url)
        .header("Content-Type", "application/x-ndjson")
        .body("{\"test\":true,\"source\":\"machina\"}\n");
    if let Some(token) = config
        .get("bearer_token")
        .and_then(|v| v.as_str())
        .filter(|t| !t.is_empty())
    {
        req = req.bearer_auth(token);
    }
    let res = req.send().await?;
    if res.status().is_success() {
        Ok(format!("webhook accepted ({})", res.status()))
    } else {
        anyhow::bail!("webhook returned {}", res.status())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn doc_carries_summary_and_controller() {
        let ev = EventRow {
            id: Uuid::nil(),
            occurred_at: Utc::now(),
            source: "audit".into(),
            severity: "high".into(),
            summary: "login failed".into(),
            ecs_json: json!({"event": {"kind": "alert"}}),
        };
        let d = event_doc(&ev, "ctl-1");
        assert_eq!(d["message"], "login failed");
        assert_eq!(d["machina_controller"], "ctl-1");
        assert_eq!(d["event"]["kind"], "alert");
    }
}
