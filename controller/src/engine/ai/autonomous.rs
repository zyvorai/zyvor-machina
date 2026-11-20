// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;

#[derive(Debug, Deserialize)]
pub struct AutonomousPlanBody {
    pub goal: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub simulate: bool,
}

#[derive(Debug, Serialize)]
pub struct AutonomousPlanStep {
    pub order: u32,
    pub agent_id: String,
    pub title: String,
    pub detail: String,
    pub action_type: Option<String>,
    pub requires_approval: bool,
}

#[derive(Debug, Serialize)]
pub struct AutonomousPlanResult {
    pub goal: String,
    pub agent_id: String,
    pub steps: Vec<AutonomousPlanStep>,
    pub simulation: Option<serde_json::Value>,
    pub deterministic: bool,
}

pub async fn plan(
    pool: &DbPool,
    cfg: &crate::config::ControllerConfig,
    body: &AutonomousPlanBody,
) -> anyhow::Result<AutonomousPlanResult> {
    let agent_id = super::agents::resolve_agent_id(body.agent.as_deref());
    if body.agent.as_deref() == Some("auto") || body.agent.is_none() {
        let picked = super::agents::pick_agent(&body.goal, None);
        return plan_with_agent(pool, cfg, body, &picked).await;
    }
    plan_with_agent(pool, cfg, body, &agent_id).await
}

async fn plan_with_agent(
    pool: &DbPool,
    cfg: &crate::config::ControllerConfig,
    body: &AutonomousPlanBody,
    agent_id: &str,
) -> anyhow::Result<AutonomousPlanResult> {
    let ctx = super::context::assemble(pool, cfg, None, None).await?;
    let mut steps = vec![
        AutonomousPlanStep {
            order: 1,
            agent_id: "observability".into(),
            title: "Analyze current fleet state".into(),
            detail: format!(
                "{} VMs, {} hosts online, {} recommendations",
                ctx.cluster_vms, ctx.cluster_hosts_online, ctx.recommendations_count
            ),
            action_type: None,
            requires_approval: false,
        },
        AutonomousPlanStep {
            order: 2,
            agent_id: agent_id.into(),
            title: "Draft execution plan".into(),
            detail: body.goal.clone(),
            action_type: None,
            requires_approval: false,
        },
        AutonomousPlanStep {
            order: 3,
            agent_id: "security".into(),
            title: "Security review".into(),
            detail: "Validate blast radius and firewall posture before changes.".into(),
            action_type: Some("security_review".into()),
            requires_approval: true,
        },
        AutonomousPlanStep {
            order: 4,
            agent_id: agent_id.into(),
            title: "Execute approved actions".into(),
            detail: "Queue tasks only after operator approval.".into(),
            action_type: Some("execute_plan".into()),
            requires_approval: true,
        },
    ];

    let simulation = if body.simulate {
        Some(
            super::digital_twin::analyze_impact(
                pool,
                &super::digital_twin::ImpactRequest {
                    target_kind: "host".into(),
                    target_id: String::new(),
                    action: "shutdown".into(),
                },
            )
            .await
            .map(|v| serde_json::to_value(v).unwrap_or(serde_json::json!({})))
            .unwrap_or(serde_json::json!({"note": "simulation unavailable"})),
        )
    } else {
        None
    };

    let mut deterministic = true;
    if let Ok(Some(llm)) = super::llm::complete(
        pool,
        super::llm::CompletionRequest {
            task_class: super::routing::TaskClass::Infrastructure,
            system: super::agents::system_prompt(agent_id).to_string(),
            user: format!(
                "Goal: {}\nCluster: {} VMs / {} hosts\nReturn 3-5 numbered plan steps.",
                body.goal, ctx.cluster_vms, ctx.cluster_hosts_online
            ),
            agent_id: Some(agent_id.to_string()),
            user_id: None,
        },
    )
    .await
    {
        deterministic = false;
        let extra_text = llm.lines().take(3).collect::<Vec<_>>().join(" ");
        if !extra_text.trim().is_empty() {
            steps.push(AutonomousPlanStep {
                order: 5,
                agent_id: agent_id.into(),
                title: "Zyra recommendation".into(),
                detail: extra_text.trim().to_string(),
                action_type: None,
                requires_approval: false,
            });
        }
    }

    Ok(AutonomousPlanResult {
        goal: body.goal.clone(),
        agent_id: agent_id.into(),
        steps,
        simulation,
        deterministic,
    })
}

#[derive(Debug, Deserialize)]
pub struct AutonomousExecuteBody {
    pub goal: String,
    #[serde(default)]
    pub agent: Option<String>,
}

pub async fn execute_approved_plan(
    pool: &DbPool,
    cfg: &crate::config::ControllerConfig,
    _state: &crate::state::AppState,
    actor: &crate::auth::AuthUser,
    body: &AutonomousExecuteBody,
) -> anyhow::Result<serde_json::Value> {
    super::enterprise_zyra::require_zyra_execute(actor).map_err(|e| anyhow::anyhow!(e.message))?;
    let plan = plan(
        pool,
        cfg,
        &AutonomousPlanBody {
            goal: body.goal.clone(),
            agent: body.agent.clone(),
            simulate: false,
        },
    )
    .await?;
    let mut action_ids = Vec::new();
    for step in plan.steps.iter().filter(|s| s.requires_approval) {
        let created = super::actions::create_action(
            pool,
            &super::actions::CreateActionBody {
                action_type: step
                    .action_type
                    .clone()
                    .unwrap_or_else(|| "execute_plan".into()),
                label: step.title.clone(),
                review: step.detail.clone(),
                risk: "Review required".into(),
                object_ref: serde_json::json!({"goal": body.goal, "agent": plan.agent_id}),
                source: "zyra.autonomous".into(),
            },
            &actor.username,
        )
        .await?;
        action_ids.push(created.id);
    }
    Ok(serde_json::json!({
        "goal": body.goal,
        "agent_id": plan.agent_id,
        "queued_approvals": action_ids,
        "message": "Plan queued for human approval"
    }))
}
