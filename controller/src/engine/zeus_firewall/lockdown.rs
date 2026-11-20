// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use super::profiles::plan_for_profile;
use crate::config::ControllerConfig;
use crate::engine::zeus_firewall::inventory::apply_target;

#[derive(Debug, Clone, Serialize)]
pub struct LockdownPreview {
    pub summary: String,
    pub actions: Vec<String>,
    pub capture_traffic: bool,
    pub create_incident: bool,
}

pub fn lockdown_preview(capture: bool) -> LockdownPreview {
    LockdownPreview {
        summary: "Emergency Isolation will block all traffic except Zeus management, console, and backup/forensics networks".into(),
        actions: vec![
            "Apply EmergencyIsolation profile".into(),
            "Preserve current logs".into(),
            "Create rollback checkpoint".into(),
            if capture {
                "Start native eBPF packet capture".into()
            } else {
                "Skip packet capture".into()
            },
            "Create security incident event".into(),
        ],
        capture_traffic: capture,
        create_incident: true,
    }
}

pub async fn lockdown_target(
    pool: &DbPool,
    cfg: &ControllerConfig,
    target_id: &str,
    capture: bool,
    actor: &str,
) -> anyhow::Result<serde_json::Value> {
    let plan = plan_for_profile("EmergencyIsolation", false)?;
    let result = apply_target(pool, cfg, target_id, plan, actor).await?;
    if capture {
        let _ = crate::engine::bpf::telemetry::capture_target(pool, target_id).await;
    }
    if let Ok(host_id) = Uuid::parse_str(target_id) {
        let _ = crate::db::query(
            "INSERT INTO events (id, kind, message, resource_type, resource_id, payload) VALUES (?, 'security', ?, 'host', ?, '{\"severity\":\"critical\"}')",
        )
        .bind(uuid::Uuid::new_v4())
        .bind("Zeus Lockdown enabled — Emergency Isolation")
        .bind(host_id)
        .execute(pool)
        .await;
    }
    Ok(serde_json::json!({
        "ok": true,
        "preview": lockdown_preview(capture),
        "result": result
    }))
}
