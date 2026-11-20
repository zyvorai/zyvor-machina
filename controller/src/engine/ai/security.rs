// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct SecurityFinding {
    pub id: String,
    pub severity: String,
    pub title: String,
    pub detail: String,
    pub remediation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_ref: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct SecurityReport {
    pub risk_level: String,
    pub findings: Vec<SecurityFinding>,
}

pub async fn scan(pool: &DbPool) -> anyhow::Result<SecurityReport> {
    let mut findings = Vec::new();

    let no_backup: Vec<(uuid::Uuid, String)> = crate::db::query_as(
        "SELECT v.id, v.name FROM vms v
         WHERE COALESCE(v.managed, TRUE) = TRUE
           AND (EXISTS (SELECT 1 FROM json_each(COALESCE(v.tags,'[]')) WHERE value='prod') OR EXISTS (SELECT 1 FROM json_each(COALESCE(v.tags,'[]')) WHERE value='production'))
           AND NOT EXISTS (SELECT 1 FROM backup_records b WHERE b.vm_id = v.id AND b.status = 'completed')
         LIMIT 20",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for (id, name) in no_backup {
        findings.push(SecurityFinding {
            id: format!("no-backup-{id}"),
            severity: "high".into(),
            title: format!("Production VM '{name}' has no backup"),
            detail: "Backup compliance gap for production workload.".into(),
            remediation: "Queue backup from VM Doctor or Recommendations.".into(),
            object_ref: Some(serde_json::json!({ "vm_id": id.to_string() })),
        });
    }

    let no_guest: Vec<(uuid::Uuid, String)> = crate::db::query_as(
        "SELECT id, name FROM vms WHERE observed_state = 'running'
         AND guest_tools_status IN ('unknown', 'not_installed') LIMIT 15",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for (id, name) in no_guest {
        findings.push(SecurityFinding {
            id: format!("no-guest-{id}"),
            severity: "medium".into(),
            title: format!("Guest tools missing on '{name}'"),
            detail: "Monitoring and graceful shutdown may be impaired.".into(),
            remediation: "Install guest tools from VM detail.".into(),
            object_ref: Some(serde_json::json!({ "vm_id": id.to_string() })),
        });
    }

    let offline_hosts: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM hosts WHERE state = 'offline'")
            .fetch_one(pool)
            .await
            .unwrap_or(0);
    if offline_hosts > 0 {
        findings.push(SecurityFinding {
            id: "offline-hosts".into(),
            severity: "high".into(),
            title: format!("{offline_hosts} host(s) offline"),
            detail: "Cluster resilience reduced.".into(),
            remediation: "Sync hosts and restart agents.".into(),
            object_ref: None,
        });
    }

    let risk_level = if findings.iter().any(|f| f.severity == "high") {
        "high"
    } else if findings.is_empty() {
        "low"
    } else {
        "medium"
    };

    Ok(SecurityReport {
        risk_level: risk_level.into(),
        findings,
    })
}

fn explain_event_heuristic(event: &serde_json::Value, host_id: Option<&str>) -> serde_json::Value {
    let kind = event
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("security");
    let summary = event
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("Security event");
    let severity = event
        .get("severity")
        .and_then(|v| v.as_str())
        .unwrap_or("info");
    let risk = match severity {
        "critical" => "Critical — investigate immediately",
        "high" => "High — likely requires operator review",
        "medium" => "Medium — monitor and correlate",
        _ => "Low — likely routine activity",
    };
    serde_json::json!({
        "host_id": host_id,
        "kind": kind,
        "summary": summary,
        "explanation": format!("Event type '{kind}': {summary}. {risk}."),
        "risk": risk,
        "recommendation": "Review timeline for related events; use attack reconstruction if severity is high.",
        "llm_powered": false
    })
}

pub async fn explain_event(
    pool: &DbPool,
    event: &serde_json::Value,
    host_id: Option<&str>,
) -> anyhow::Result<serde_json::Value> {
    let mut out = explain_event_heuristic(event, host_id);
    if let Ok(Some(llm)) = super::llm::complete_simple(
        pool,
        "You are Zyra Security. Explain eBPF runtime security events for operators in 2-4 sentences. End with one concrete next step.",
        &format!(
            "Host: {}\nEvent:\n{}",
            host_id.unwrap_or("unknown"),
            serde_json::to_string_pretty(event).unwrap_or_else(|_| event.to_string())
        ),
    )
    .await
    {
        out["explanation"] = serde_json::Value::String(llm);
        out["llm_powered"] = serde_json::Value::Bool(true);
    }
    Ok(out)
}

pub fn attack_reconstruct_sync(timeline: &serde_json::Value) -> serde_json::Value {
    let events = timeline
        .get("events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut chain: Vec<String> = Vec::new();
    for (i, ev) in events.iter().take(8).enumerate() {
        let s = ev
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("event");
        chain.push(format!("{}. {}", i + 1, s));
    }
    serde_json::json!({
        "attack_chain": chain,
        "summary": if chain.is_empty() {
            "No events in window — unable to reconstruct chain.".into()
        } else {
            format!("Reconstructed {} step(s) from security timeline.", chain.len())
        },
        "llm_powered": false
    })
}

pub async fn attack_reconstruct(
    pool: &DbPool,
    timeline: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let mut out = attack_reconstruct_sync(timeline);
    let events = timeline
        .get("events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if events.is_empty() {
        return Ok(out);
    }
    let compact: Vec<String> = events
        .iter()
        .take(12)
        .map(|ev| {
            let ts = ev.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
            let summary = ev
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("event");
            let sev = ev
                .get("severity")
                .and_then(|v| v.as_str())
                .unwrap_or("info");
            format!("{ts} [{sev}] {summary}")
        })
        .collect();
    if let Ok(Some(llm)) = super::llm::complete_simple(
        pool,
        "You are a threat hunter. Given a security timeline, output a numbered attack chain (one step per line, max 8 steps) and a one-sentence summary on the last line prefixed with 'Summary: '.",
        &format!("Timeline events:\n{}", compact.join("\n")),
    )
    .await
    {
        let (chain, summary) = parse_attack_chain_llm(&llm);
        if !chain.is_empty() {
            out["attack_chain"] = serde_json::json!(chain);
        }
        if let Some(s) = summary {
            out["summary"] = serde_json::Value::String(s);
        }
        out["llm_powered"] = serde_json::Value::Bool(true);
    }
    Ok(out)
}

fn parse_attack_chain_llm(text: &str) -> (Vec<String>, Option<String>) {
    let mut chain = Vec::new();
    let mut summary = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Summary:") {
            summary = Some(rest.trim().to_string());
            continue;
        }
        let step = trimmed
            .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')' || c == ' ')
            .trim()
            .to_string();
        if !step.is_empty() {
            chain.push(step);
        }
    }
    if summary.is_none() && chain.len() > 1 {
        summary = chain.last().cloned();
        chain.pop();
    }
    (chain, summary)
}

pub fn translate_nl_search(query: &str) -> String {
    let q = query.to_lowercase();
    if q.contains("port 8080") || q.contains("8080") {
        return "8080 network_connect".into();
    }
    if q.contains("sudo") {
        return "sudo privilege".into();
    }
    if q.contains("ssh") {
        return "ssh sshd".into();
    }
    if q.contains("curl") {
        return "curl".into();
    }
    if q.contains("russia") || q.contains("russian") {
        return "suspicious dns .ru".into();
    }
    query.to_string()
}

pub async fn translate_nl_search_async(pool: &DbPool, query: &str) -> (String, bool) {
    if let Ok(Some(llm)) = super::llm::complete_simple(
        pool,
        "Convert natural-language security hunt questions into concise keyword search terms for eBPF process/network/DNS logs. Reply with keywords only — no punctuation or explanation.",
        query,
    )
    .await
    {
        let terms = llm.lines().next().unwrap_or(&llm).trim().to_string();
        if !terms.is_empty() {
            return (terms, true);
        }
    }
    (translate_nl_search(query), false)
}

pub fn hunt_summary_heuristic(
    correlations: &serde_json::Value,
    timeline: &serde_json::Value,
) -> serde_json::Value {
    let corr = correlations
        .get("correlations")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let events = timeline
        .get("events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let critical = corr
        .iter()
        .filter(|c| {
            c.get("severity")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s == "critical" || s == "high")
        })
        .count();
    let summary = if corr.is_empty() && events.is_empty() {
        "No correlated threats or timeline events in the current window.".into()
    } else {
        format!(
            "{} correlation finding(s), {} timeline event(s), {} high/critical.",
            corr.len(),
            events.len(),
            critical
        )
    };
    serde_json::json!({
        "summary": summary,
        "priority_actions": [
            if critical > 0 { "Review high/critical correlations in Security Center" } else { "Continue monitoring fleet timeline" },
            "Pivot to affected hosts for process graph and container context",
            "Sync critical alerts to Notifications if not already done",
        ],
        "llm_powered": false
    })
}

pub async fn hunt_summary(
    pool: &DbPool,
    correlations: &serde_json::Value,
    timeline: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let mut out = hunt_summary_heuristic(correlations, timeline);
    let corr = correlations
        .get("correlations")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let events = timeline
        .get("events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if corr.is_empty() && events.is_empty() {
        return Ok(out);
    }
    let corr_lines: Vec<String> = corr
        .iter()
        .take(8)
        .map(|c| {
            format!(
                "[{}] {}",
                c.get("severity").and_then(|v| v.as_str()).unwrap_or("?"),
                c.get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("finding")
            )
        })
        .collect();
    let event_lines: Vec<String> = events
        .iter()
        .take(8)
        .map(|e| {
            e.get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("event")
                .to_string()
        })
        .collect();
    if let Ok(Some(llm)) = super::llm::complete_simple(
        pool,
        "You are Zyra Security lead summarizing a threat hunt. Write 3-5 sentences: overall posture, top risks, and recommended next steps for an operator.",
        &format!(
            "Correlations:\n{}\n\nRecent timeline:\n{}",
            corr_lines.join("\n"),
            event_lines.join("\n")
        ),
    )
    .await
    {
        out["summary"] = serde_json::Value::String(llm);
        out["llm_powered"] = serde_json::Value::Bool(true);
    }
    Ok(out)
}
