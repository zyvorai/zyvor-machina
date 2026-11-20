// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Operations — runbook catalog, execution history, compliance showback (Phase 29).

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use crate::config::ControllerConfig;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RunbookCatalogRow {
    pub id: Uuid,
    pub incident: String,
    pub title: String,
    pub category: String,
    pub severity: String,
    pub auto_trigger: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RunbookExecutionRow {
    pub id: Uuid,
    pub incident: String,
    pub status: String,
    pub steps_json: serde_json::Value,
    pub actor: Option<String>,
    pub summary: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OperationsOverview {
    pub runbook_count: usize,
    pub executions_24h: usize,
    pub showback_projects: usize,
    pub compliance_grade: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunbookExecuteResult {
    pub execution_id: String,
    pub incident: String,
    pub title: String,
    pub steps: Vec<String>,
    pub commands: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShowbackLine {
    pub project_name: String,
    pub cost_usd: f64,
    pub compliance_grade: String,
    pub vm_count: usize,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShowbackOverview {
    pub lines: Vec<ShowbackLine>,
    pub total_cost_usd: f64,
    pub fleet_grade: String,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
pub struct ExecuteRunbookRequest {
    #[serde(default)]
    pub context: serde_json::Value,
}

pub async fn overview(pool: &DbPool) -> anyhow::Result<OperationsOverview> {
    ensure_showback_snapshots(pool).await?;

    let runbook_count: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM ops_runbook_catalog WHERE enabled = true")
            .fetch_one(pool)
            .await?;

    let executions_24h: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM ops_runbook_executions WHERE created_at > datetime('now', '-24 hours')",
    )
    .fetch_one(pool)
    .await?;

    let showback_projects: i64 =
        crate::db::query_scalar("SELECT COUNT(DISTINCT project_name) FROM ops_showback_snapshots")
            .fetch_one(pool)
            .await?;

    let compliance = crate::engine::ai::compliance::generate(pool).await?;
    let grade = compliance.grade.clone();

    Ok(OperationsOverview {
        runbook_count: runbook_count as usize,
        executions_24h: executions_24h as usize,
        showback_projects: showback_projects as usize,
        compliance_grade: grade.clone(),
        summary: format!(
            "{} runbook(s) · {} execution(s) in 24h · fleet grade {}",
            runbook_count, executions_24h, grade
        ),
    })
}

pub async fn list_catalog(pool: &DbPool) -> anyhow::Result<Vec<RunbookCatalogRow>> {
    crate::db::query_as(
        "SELECT id, incident, title, category, severity, auto_trigger, enabled
         FROM ops_runbook_catalog WHERE enabled = true ORDER BY category, title",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.into())
}

pub async fn list_executions(
    pool: &DbPool,
    limit: i64,
) -> anyhow::Result<Vec<RunbookExecutionRow>> {
    crate::db::query_as(
        "SELECT id, incident, status, steps_json, actor, summary, created_at
         FROM ops_runbook_executions ORDER BY created_at DESC LIMIT ?",
    )
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await
    .map_err(|e| e.into())
}

pub async fn execute_runbook(
    pool: &DbPool,
    incident: &str,
    actor: &str,
    context: &serde_json::Value,
) -> anyhow::Result<RunbookExecuteResult> {
    let catalog: Option<(String,)> = crate::db::query_as(
        "SELECT title FROM ops_runbook_catalog WHERE incident = ? AND enabled = true",
    )
    .bind(incident)
    .fetch_optional(pool)
    .await?;

    let rb = crate::engine::ai::runbook::generate(pool, incident, context).await?;
    let execution_id = Uuid::new_v4();
    let steps_json = serde_json::to_value(&rb.steps)?;

    crate::db::query(
        "INSERT INTO ops_runbook_executions (id, incident, status, steps_json, actor, summary)
         VALUES (?, ?, 'completed', ?, ?, ?)",
    )
    .bind(execution_id)
    .bind(incident)
    .bind(steps_json)
    .bind(actor)
    .bind(rb.summary.as_deref().unwrap_or(&rb.title))
    .execute(pool)
    .await?;

    let title = catalog.map(|(t,)| t).unwrap_or(rb.title.clone());

    Ok(RunbookExecuteResult {
        execution_id: execution_id.to_string(),
        incident: incident.into(),
        title,
        steps: rb.steps,
        commands: rb.commands,
        summary: rb
            .summary
            .unwrap_or_else(|| format!("Runbook generated for {incident}")),
    })
}

pub async fn showback_overview(
    pool: &DbPool,
    _cfg: &ControllerConfig,
) -> anyhow::Result<ShowbackOverview> {
    ensure_showback_snapshots(pool).await?;

    let rows: Vec<(String, f64, String, i32)> = crate::db::query_as(
        "SELECT s.project_name, s.cost_usd, s.compliance_grade, s.vm_count
         FROM ops_showback_snapshots s
         WHERE s.captured_at = (
             SELECT MAX(s2.captured_at) FROM ops_showback_snapshots s2
             WHERE s2.project_name = s.project_name
         )
         ORDER BY s.project_name",
    )
    .fetch_all(pool)
    .await?;

    let mut lines = Vec::new();
    let mut total = 0.0f64;
    for (project_name, cost_usd, compliance_grade, vm_count) in rows {
        total += cost_usd;
        lines.push(ShowbackLine {
            project_name: project_name.clone(),
            cost_usd,
            compliance_grade,
            vm_count: vm_count as usize,
            notes: format!("{vm_count} VM(s) · showback snapshot"),
        });
    }

    if let Ok(attr) = crate::engine::ai::cost_attribution::attribute(pool).await {
        for team in attr.teams {
            if lines.iter().any(|l| l.project_name == team.team) {
                continue;
            }
            lines.push(ShowbackLine {
                project_name: team.team.clone(),
                cost_usd: team.estimated_monthly_usd,
                compliance_grade: "B".into(),
                vm_count: team.vm_count as usize,
                notes: "Live cost attribution".into(),
            });
            total += team.estimated_monthly_usd;
        }
    }

    let compliance = crate::engine::ai::compliance::generate(pool).await?;

    Ok(ShowbackOverview {
        summary: format!(
            "{} project(s) · ${:.0}/mo estimated · fleet grade {}",
            lines.len(),
            total,
            compliance.grade
        ),
        total_cost_usd: total,
        fleet_grade: compliance.grade,
        lines,
    })
}

async fn ensure_showback_snapshots(pool: &DbPool) -> anyhow::Result<()> {
    let count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM ops_showback_snapshots")
        .fetch_one(pool)
        .await?;
    if count > 0 {
        return Ok(());
    }

    let projects: Vec<(String,)> =
        crate::db::query_as("SELECT DISTINCT COALESCE(NULLIF(TRIM(project), ''), 'default') FROM vms ORDER BY 1 LIMIT 20")
            .fetch_all(pool)
            .await
            .unwrap_or_default();

    if projects.is_empty() {
        let vm_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vms")
            .fetch_one(pool)
            .await
            .unwrap_or(0);
        crate::db::query(
            "INSERT INTO ops_showback_snapshots (id, project_name, cost_usd, compliance_grade, vm_count)
             VALUES (?, 'default', ?, 'B', ?)",
        )
        .bind(Uuid::new_v4())
        .bind((vm_count as f64) * 12.0)
        .bind(vm_count)
        .execute(pool)
        .await?;
        return Ok(());
    }

    let compliance = crate::engine::ai::compliance::generate(pool).await.ok();
    let grade = compliance
        .as_ref()
        .map(|c| c.grade.clone())
        .unwrap_or_else(|| "B".into());

    let mut tx = pool.begin().await?;
    for (name,) in projects {
        let vm_count: i64 = crate::db::query_scalar(
            "SELECT COUNT(*) FROM vms WHERE COALESCE(NULLIF(TRIM(project), ''), 'default') = ?",
        )
        .bind(&name)
        .fetch_one(&mut *tx)
        .await
        .unwrap_or(0);

        let cost = (vm_count as f64) * 18.5 + 25.0;
        crate::db::query(
            "INSERT INTO ops_showback_snapshots (id, project_name, cost_usd, compliance_grade, vm_count)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(&name)
        .bind(cost)
        .bind(if vm_count > 5 { "C" } else { grade.as_str() })
        .bind(vm_count)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
