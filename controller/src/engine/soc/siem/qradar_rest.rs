// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde_json::json;
use crate::db::DbPool;

use super::{
    fetch_unexported_events, integration_err, integration_ok, mark_exported, IntegrationRow,
};

pub async fn forward(
    pool: &DbPool,
    integ: &IntegrationRow,
    controller_id: &str,
) -> anyhow::Result<usize> {
    let base = integ
        .config_json
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_end_matches('/');
    let token = integ
        .config_json
        .get("api_token")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let log_source = integ
        .config_json
        .get("log_source_id")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if base.is_empty() || token.is_empty() {
        return Ok(0);
    }

    let insecure_tls = integ
        .config_json
        .get("insecure_tls")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .danger_accept_invalid_certs(insecure_tls)
        .build()?;

    let events = fetch_unexported_events(pool, integ.id, 100).await?;
    if events.is_empty() {
        return Ok(0);
    }

    let url = format!("{base}/api/siem/events");
    let mut exported = Vec::new();
    for ev in &events {
        let body = json!({
            "events": [{
                "qid": 0,
                "log_source_id": log_source,
                "message": ev.summary,
                "severity": qradar_severity(&ev.severity),
                "payload": ev.ecs_json,
                "hostname": controller_id,
            }]
        });
        let res = client
            .post(&url)
            .header("SEC", token)
            .header("Version", "16.0")
            .json(&body)
            .send()
            .await?;
        if !res.status().is_success() {
            let text = res.text().await.unwrap_or_default();
            integration_err(pool, integ.id, &format!("qradar: {text}")).await?;
            break;
        }
        exported.push(ev.id);
    }

    if !exported.is_empty() {
        mark_exported(pool, integ.id, "event", &exported).await?;
        integration_ok(pool, integ.id).await?;
    }
    Ok(exported.len())
}

fn qradar_severity(sev: &str) -> i32 {
    match sev.to_lowercase().as_str() {
        "critical" => 10,
        "high" => 7,
        "medium" => 5,
        "low" => 3,
        _ => 1,
    }
}

pub async fn test_connection(config: &serde_json::Value) -> anyhow::Result<String> {
    let base = config.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let token = config
        .get("api_token")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if base.is_empty() || token.is_empty() {
        anyhow::bail!("url and api_token required");
    }
    let insecure_tls = config
        .get("insecure_tls")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(insecure_tls)
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let res = client
        .get(format!("{}/api/system/about", base.trim_end_matches('/')))
        .header("SEC", token)
        .header("Version", "16.0")
        .send()
        .await?;
    if res.status().is_success() {
        Ok("QRadar API reachable".into())
    } else {
        anyhow::bail!("QRadar about: {}", res.status())
    }
}
