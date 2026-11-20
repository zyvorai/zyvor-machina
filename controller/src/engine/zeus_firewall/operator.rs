// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// AI operator — guardrailed autonomous secure-machine (Phase 25, hardened apply path).

use serde::{Deserialize, Serialize};
use crate::db::DbPool;

use crate::config::ControllerConfig;

use super::approvals::{self, ApprovalRequest};
use super::inventory::{apply_profile, overview};

#[derive(Debug, Clone, Serialize)]
pub struct OperatorThresholds {
    pub max_risk_score: u32,
    pub auto_apply_enabled: bool,
    pub require_approval_above: u32,
    pub budget_guard_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SecureMachinePreview {
    pub host_id: String,
    pub hostname: String,
    pub current_score: u32,
    pub target_profile: String,
    pub predicted_score: u32,
    pub risk: String,
    pub monthly_exposure_usd: f64,
    pub requires_approval: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetSecurePlan {
    pub previews: Vec<SecureMachinePreview>,
    pub auto_eligible: usize,
    pub approval_required: usize,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
pub struct OperatorExecuteRequest {
    pub host_id: String,
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize)]
pub struct OperatorBatchExecuteRequest {
    #[serde(default)]
    pub host_ids: Vec<String>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub force: bool,
    #[serde(default = "default_auto_only")]
    pub auto_only: bool,
}

fn default_auto_only() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct OperatorExecuteResult {
    pub dry_run: bool,
    pub host_id: String,
    pub applied: bool,
    pub enqueued: bool,
    pub task_id: Option<String>,
    pub approval_id: Option<String>,
    pub operations: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OperatorBatchExecuteResult {
    pub dry_run: bool,
    pub results: Vec<OperatorExecuteResult>,
    pub applied_count: usize,
    pub approval_count: usize,
    pub summary: String,
}

pub fn thresholds() -> OperatorThresholds {
    OperatorThresholds {
        max_risk_score: 65,
        auto_apply_enabled: false,
        require_approval_above: 45,
        budget_guard_usd: 500.0,
    }
}

pub async fn fleet_secure_preview(
    pool: &DbPool,
    cfg: &ControllerConfig,
) -> anyhow::Result<FleetSecurePlan> {
    let ov = overview(pool, cfg).await?;
    let thresh = thresholds();
    let cfg_finops = crate::config::ControllerConfig::default();
    let exp = super::finops::exposure_rollup(pool, &cfg_finops).await.ok();

    let mut previews = Vec::new();
    for t in ov.targets.iter().filter(|t| t.kind == "host") {
        if t.score >= thresh.max_risk_score {
            continue;
        }
        let exposure = exp
            .as_ref()
            .and_then(|r| r.targets.iter().find(|x| x.target_id == t.id))
            .map(|x| x.exposure_monthly_usd)
            .unwrap_or(0.0);
        let target_profile: String = if t.score < 50 {
            "Emergency".into()
        } else {
            "ProductionServer".into()
        };
        let predicted_score = (t.score + 25).min(95);
        let requires_approval =
            t.score < thresh.require_approval_above || exposure > thresh.budget_guard_usd;
        previews.push(SecureMachinePreview {
            host_id: t.id.clone(),
            hostname: t.hostname.clone(),
            current_score: t.score,
            target_profile: target_profile.clone(),
            predicted_score,
            risk: t.risk.clone(),
            monthly_exposure_usd: exposure,
            requires_approval,
            summary: format!(
                "{} → {} (score {} → ~{})",
                t.hostname, target_profile, t.score, predicted_score
            ),
        });
    }

    let auto_eligible = previews.iter().filter(|p| !p.requires_approval).count();
    let approval_required = previews.len().saturating_sub(auto_eligible);

    Ok(FleetSecurePlan {
        summary: format!(
            "{} host(s) · {} auto-eligible · {} need approval",
            previews.len(),
            auto_eligible,
            approval_required
        ),
        previews,
        auto_eligible,
        approval_required,
    })
}

pub async fn execute_secure(
    pool: &DbPool,
    cfg: &ControllerConfig,
    req: &OperatorExecuteRequest,
    actor: &str,
) -> anyhow::Result<OperatorExecuteResult> {
    let plan = fleet_secure_preview(pool, cfg).await?;
    let preview = plan
        .previews
        .iter()
        .find(|p| p.host_id == req.host_id)
        .ok_or_else(|| anyhow::anyhow!("host not in operator plan"))?;

    let profile = if req.profile.is_empty() {
        preview.target_profile.clone()
    } else {
        req.profile.clone()
    };

    if preview.requires_approval && !req.force {
        if req.dry_run {
            return Ok(OperatorExecuteResult {
                dry_run: true,
                host_id: req.host_id.clone(),
                applied: false,
                enqueued: false,
                task_id: None,
                approval_id: None,
                operations: 0,
                message: format!(
                    "Dry-run: would queue approval for {} → {}",
                    preview.hostname, profile
                ),
            });
        }
        let approval = approvals::request_approval(
            pool,
            ApprovalRequest {
                target_id: req.host_id.clone(),
                profile: Some(profile.clone()),
                plan_json: Some(serde_json::json!({
                    "operator": true,
                    "hostname": preview.hostname,
                    "current_score": preview.current_score,
                    "predicted_score": preview.predicted_score,
                    "monthly_exposure_usd": preview.monthly_exposure_usd,
                })),
            },
            actor,
        )
        .await?;
        return Ok(OperatorExecuteResult {
            dry_run: false,
            host_id: req.host_id.clone(),
            applied: false,
            enqueued: true,
            task_id: None,
            approval_id: Some(approval.id.to_string()),
            operations: 0,
            message: format!(
                "Approval required for {} → {} — review in Compliance",
                preview.hostname, profile
            ),
        });
    }

    if req.dry_run {
        let preview_result = apply_profile(pool, cfg, &req.host_id, &profile, actor, true).await?;
        return Ok(OperatorExecuteResult {
            dry_run: true,
            host_id: req.host_id.clone(),
            applied: false,
            enqueued: false,
            task_id: None,
            approval_id: None,
            operations: preview_result.operations.len(),
            message: format!(
                "Dry-run: {} operation(s) to apply {} to {}",
                preview_result.operations.len(),
                profile,
                preview.hostname
            ),
        });
    }

    let result = apply_profile(pool, cfg, &req.host_id, &profile, actor, false).await?;
    let host_id = uuid::Uuid::parse_str(&req.host_id).unwrap_or_else(|_| uuid::Uuid::nil());
    let _ = crate::db::query(
        "INSERT INTO firewall_timeline (id, target_kind, target_id, kind, summary, detail_json, actor) VALUES (?, 'host', ?, 'operator_secure', ?, ?, ?)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(host_id)
    .bind(format!(
        "Operator applied {} → {}",
        preview.hostname, profile
    ))
    .bind(serde_json::json!({
        "profile": profile,
        "hostname": preview.hostname,
        "operations": result.operations,
        "forced": req.force,
    }))
    .bind(actor)
    .execute(pool)
    .await;

    Ok(OperatorExecuteResult {
        dry_run: false,
        host_id: req.host_id.clone(),
        applied: true,
        enqueued: false,
        task_id: None,
        approval_id: None,
        operations: result.operations.len(),
        message: format!(
            "Applied {} to {} ({} operation(s))",
            profile,
            preview.hostname,
            result.operations.len()
        ),
    })
}

pub async fn execute_secure_batch(
    pool: &DbPool,
    cfg: &ControllerConfig,
    req: &OperatorBatchExecuteRequest,
    actor: &str,
) -> anyhow::Result<OperatorBatchExecuteResult> {
    let plan = fleet_secure_preview(pool, cfg).await?;
    let targets: Vec<&SecureMachinePreview> = if req.host_ids.is_empty() {
        plan.previews
            .iter()
            .filter(|p| !req.auto_only || !p.requires_approval)
            .collect()
    } else {
        plan.previews
            .iter()
            .filter(|p| req.host_ids.iter().any(|id| id == &p.host_id))
            .filter(|p| !req.auto_only || !p.requires_approval || req.force)
            .collect()
    };

    let mut results = Vec::new();
    let mut applied_count = 0usize;
    let mut approval_count = 0usize;

    for preview in targets {
        let single = OperatorExecuteRequest {
            host_id: preview.host_id.clone(),
            profile: preview.target_profile.clone(),
            dry_run: req.dry_run,
            force: req.force,
        };
        let result = execute_secure(pool, cfg, &single, actor).await?;
        if result.applied {
            applied_count += 1;
        }
        if result.approval_id.is_some() {
            approval_count += 1;
        }
        results.push(result);
    }

    Ok(OperatorBatchExecuteResult {
        dry_run: req.dry_run,
        applied_count,
        approval_count,
        summary: format!(
            "{} host(s) processed · {} applied · {} approval(s) queued",
            results.len(),
            applied_count,
            approval_count
        ),
        results,
    })
}
