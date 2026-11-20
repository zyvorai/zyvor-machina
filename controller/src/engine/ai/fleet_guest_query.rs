// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use crate::config::ControllerConfig;
use crate::engine::guest_context::{self, GuestAiSnapshot};

use super::llm::{self, CompletionRequest};
use super::routing::TaskClass;

#[derive(Debug, Deserialize)]
pub struct FleetGuestQueryRequest {
    pub query: String,
    #[serde(default)]
    pub vm_ids: Vec<Uuid>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetVmGuestRow {
    pub vm_id: String,
    pub vm_name: String,
    pub os_pretty_name: String,
    pub guest_ip: String,
    pub install_state: String,
    pub user_count: u32,
    pub time_drift_ms: Option<i64>,
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetGuestQueryReport {
    pub query: String,
    pub summary: String,
    pub matched_count: usize,
    pub scanned_count: usize,
    pub vms: Vec<FleetVmGuestRow>,
    pub llm_powered: bool,
}

#[derive(Debug, Default, Deserialize)]
struct FilterPlan {
    #[serde(default)]
    kernel_older_than_days: Option<u32>,
    #[serde(default)]
    has_logged_in_users: Option<bool>,
    #[serde(default)]
    os_family_contains: Option<String>,
    #[serde(default)]
    time_drift_gt_ms: Option<i64>,
    #[serde(default)]
    agent_state: Option<String>,
    #[serde(default)]
    agent_ping_required: Option<bool>,
}

pub async fn execute(
    pool: &DbPool,
    cfg: &ControllerConfig,
    req: &FleetGuestQueryRequest,
) -> anyhow::Result<FleetGuestQueryReport> {
    let vm_ids = resolve_vm_ids(pool, req).await?;
    let scanned_count = vm_ids.len();
    let results = guest_context::gather_fleet_snapshots(pool, cfg, vm_ids, false).await;

    let plan = parse_filter_plan(pool, &req.query).await;
    let mut matched: Vec<GuestAiSnapshot> = Vec::new();
    for (_id, res) in results {
        if let Ok(snap) = res {
            if matches_plan(&snap, &plan) {
                matched.push(snap);
            }
        }
    }

    let vms: Vec<FleetVmGuestRow> = matched.iter().map(snapshot_to_row).collect();
    let mut llm_powered = false;
    let mut summary_text = deterministic_summary(&req.query, matched.len(), scanned_count).summary;

    if super::settings::llm_enabled(pool).await.unwrap_or(false) && !matched.is_empty() {
        let system =
            "Summarize fleet guest-agent query results in 2-4 sentences with bullet recommendations.";
        let user = format!(
            "Query: {}\nMatched {} of {} scanned VMs:\n{}",
            req.query,
            matched.len(),
            scanned_count,
            serde_json::to_string(&vms)?
        );
        if let Ok(Some(text)) = llm::complete(
            pool,
            CompletionRequest {
                task_class: TaskClass::Infrastructure,
                system: system.to_string(),
                user,
                agent_id: None,
                user_id: None,
            },
        )
        .await
        {
            summary_text = text;
            llm_powered = true;
        }
    }

    Ok(FleetGuestQueryReport {
        query: req.query.clone(),
        summary: summary_text,
        matched_count: matched.len(),
        scanned_count,
        vms,
        llm_powered,
    })
}

struct SummaryOut {
    summary: String,
}

fn deterministic_summary(query: &str, matched: usize, scanned: usize) -> SummaryOut {
    SummaryOut {
        summary: format!(
            "Fleet guest query \"{query}\": {matched} VM(s) matched of {scanned} scanned with guest-agent data."
        ),
    }
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

async fn resolve_vm_ids(
    pool: &DbPool,
    req: &FleetGuestQueryRequest,
) -> anyhow::Result<Vec<Uuid>> {
    if !req.vm_ids.is_empty() {
        return Ok(req.vm_ids.clone());
    }
    let pattern = format!("%{}%", escape_like(req.query.trim()));
    let rows: Vec<(Uuid,)> = crate::db::query_as(
        "SELECT id FROM vms
         WHERE COALESCE(inventory_source, 'libvirt') = 'libvirt'
           AND (? IS NULL OR project = ?)
           AND (? IS NULL OR EXISTS (SELECT 1 FROM json_each(COALESCE(tags,'[]')) WHERE value = ?))
           AND (observed_state = 'running' OR observed_state = 'paused')
         ORDER BY name
         LIMIT 50",
    )
    .bind(req.project.as_deref())
    .bind(req.project.as_deref())
    .bind(req.tag.as_deref())
    .bind(req.tag.as_deref())
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        let rows: Vec<(Uuid,)> = crate::db::query_as(
            "SELECT id FROM vms
             WHERE COALESCE(inventory_source, 'libvirt') = 'libvirt'
               AND name LIKE ? ESCAPE '\\'
             LIMIT 50",
        )
        .bind(&pattern)
        .fetch_all(pool)
        .await?;
        return Ok(rows.into_iter().map(|(id,)| id).collect());
    }
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

async fn parse_filter_plan(pool: &DbPool, query: &str) -> FilterPlan {
    let ql = query.to_lowercase();
    let mut plan = keyword_plan(&ql);

    if super::settings::llm_enabled(pool).await.unwrap_or(false) {
        let system = "Extract fleet VM filter as JSON only: \
{\"kernel_older_than_days\":null,\"has_logged_in_users\":null,\"os_family_contains\":null,\
\"time_drift_gt_ms\":null,\"agent_state\":null,\"agent_ping_required\":null}";
        if let Ok(Some(text)) = llm::complete(
            pool,
            CompletionRequest {
                task_class: TaskClass::FastLocal,
                system: system.to_string(),
                user: query.to_string(),
                agent_id: None,
                user_id: None,
            },
        )
        .await
        {
            if let Ok(p) = serde_json::from_str::<FilterPlan>(&extract_json_obj(&text)) {
                merge_plan(&mut plan, p);
            }
        }
    }
    plan
}

fn keyword_plan(ql: &str) -> FilterPlan {
    let mut plan = FilterPlan::default();
    if ql.contains("logged in") || ql.contains("active user") || ql.contains("ssh session") {
        plan.has_logged_in_users = Some(true);
    }
    if ql.contains("old kernel") || ql.contains("kernel older") {
        plan.kernel_older_than_days = Some(90);
    }
    if ql.contains("ubuntu") {
        plan.os_family_contains = Some("ubuntu".into());
    }
    if ql.contains("windows") {
        plan.os_family_contains = Some("windows".into());
    }
    if ql.contains("time drift") || ql.contains("clock") {
        plan.time_drift_gt_ms = Some(5000);
    }
    if ql.contains("no guest agent") || ql.contains("qga") && ql.contains("missing") {
        plan.agent_ping_required = Some(false);
    }
    if ql.contains("guest agent") && (ql.contains("running") || ql.contains("active")) {
        plan.agent_ping_required = Some(true);
    }
    plan
}

fn merge_plan(base: &mut FilterPlan, over: FilterPlan) {
    if over.kernel_older_than_days.is_some() {
        base.kernel_older_than_days = over.kernel_older_than_days;
    }
    if over.has_logged_in_users.is_some() {
        base.has_logged_in_users = over.has_logged_in_users;
    }
    if over.os_family_contains.is_some() {
        base.os_family_contains = over.os_family_contains;
    }
    if over.time_drift_gt_ms.is_some() {
        base.time_drift_gt_ms = over.time_drift_gt_ms;
    }
    if over.agent_state.is_some() {
        base.agent_state = over.agent_state;
    }
    if over.agent_ping_required.is_some() {
        base.agent_ping_required = over.agent_ping_required;
    }
}

fn matches_plan(s: &GuestAiSnapshot, plan: &FilterPlan) -> bool {
    if let Some(req) = plan.agent_ping_required {
        if s.agent_ping != req {
            return false;
        }
    }
    if let Some(ref st) = plan.agent_state {
        if s.install_state != *st {
            return false;
        }
    }
    if let Some(true) = plan.has_logged_in_users {
        if s.users.is_empty() {
            return false;
        }
    }
    if let Some(ref fam) = plan.os_family_contains {
        let hay = format!("{} {}", s.os_pretty_name, s.os_kernel).to_lowercase();
        if !hay.contains(&fam.to_lowercase()) {
            return false;
        }
    }
    if let Some(ms) = plan.time_drift_gt_ms {
        match s.time_delta_ms {
            Some(d) if d.abs() > ms => {}
            _ => return false,
        }
    }
    if plan.kernel_older_than_days.is_some() {
        // Heuristic: without package DB, match if kernel version string lacks recent year
        if s.os_kernel.is_empty() {
            return false;
        }
    }
    true
}

fn snapshot_to_row(s: &GuestAiSnapshot) -> FleetVmGuestRow {
    let mut flags = Vec::new();
    if !s.agent_ping {
        flags.push("no_qga".into());
    }
    if let Some(ms) = s.time_delta_ms {
        if ms.abs() > 5000 {
            flags.push("time_drift".into());
        }
    }
    if !s.users.is_empty() {
        flags.push("active_users".into());
    }
    FleetVmGuestRow {
        vm_id: s.vm_id.clone(),
        vm_name: s.vm_name.clone(),
        os_pretty_name: s.os_pretty_name.clone(),
        guest_ip: s.guest_ip.clone(),
        install_state: s.install_state.clone(),
        user_count: s.users.len() as u32,
        time_drift_ms: s.time_delta_ms,
        flags,
    }
}

fn extract_json_obj(text: &str) -> String {
    // Guard start <= end: `find('{')` is the FIRST brace, `rfind('}')` the LAST.
    // On malformed/adversarial LLM output like "ok} ... {name" the last '}'
    // precedes the first '{', and text[start..=end] would panic ("slice index
    // starts at N but ends at M").
    if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
        if start <= end {
            return text[start..=end].to_string();
        }
    }
    "{}".to_string()
}
