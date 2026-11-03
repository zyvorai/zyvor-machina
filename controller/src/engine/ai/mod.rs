// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

pub mod action_audit;
pub mod change_review;
pub mod digest;
pub mod postmortem;
pub mod wave_advisor;
pub mod actions;
pub mod agent_loop;
pub mod forecast;
pub mod idle;
pub mod trust;
pub mod agent_marketplace;
pub mod agents;
pub mod autonomous;
pub mod context;
pub mod crypto;
pub mod enterprise_zyra;
pub mod fleet_guest_query;
pub mod guest_insights;
pub mod guest_tools;
pub mod intent_router;
pub mod llm;
pub mod memory_store;
pub mod migration_readiness;
pub mod prompts;
pub mod providers;
pub mod routing;
pub mod settings;
pub mod vm_builder;

pub use intent_router::{SearchHit, SpotlightIntent, SpotlightResult};

#[derive(Debug, Serialize)]
pub struct CopilotResponse {
    pub reply: String,
    pub deterministic: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_summary: Option<String>,
}

use crate::config::ControllerConfig;

pub async fn copilot_chat(
    pool: &DbPool,
    cfg: &ControllerConfig,
    message: &str,
    vm_id: Option<Uuid>,
    host_id: Option<Uuid>,
    vm_ids: Option<Vec<Uuid>>,
) -> anyhow::Result<CopilotResponse> {
    let base = build_copilot_base(pool, cfg, message, vm_id, host_id, vm_ids).await?;
    let mut reply = base.reply;

    let system = format!(
        "You are Zyra, an autonomous infrastructure engineer and cloud architect. Be concise. Use bullet points. {}",
        guest_tools::tools_system_prompt()
    );
    if let Ok(Some(llm_text)) = llm::complete_simple(
        pool,
        &system,
        &format!("Context: {}\nUser: {}", base.ctx_json, message),
    )
    .await
    {
        reply.push_str("\n\n");
        reply.push_str(&llm_text);
        return Ok(CopilotResponse {
            reply,
            deterministic: false,
            context_summary: Some(base.context_summary),
        });
    }

    Ok(CopilotResponse {
        reply,
        deterministic: true,
        context_summary: Some(base.context_summary),
    })
}

pub struct CopilotBase {
    pub reply: String,
    pub context_summary: String,
    pub ctx_json: String,
}

pub async fn build_copilot_base(
    pool: &DbPool,
    cfg: &ControllerConfig,
    message: &str,
    vm_id: Option<Uuid>,
    host_id: Option<Uuid>,
    vm_ids: Option<Vec<Uuid>>,
) -> anyhow::Result<CopilotBase> {
    let ctx = context::assemble(pool, cfg, vm_id, host_id).await?;
    let mut guest_snapshots: Vec<crate::engine::guest_context::GuestAiSnapshot> = Vec::new();
    if let Some(ids) = vm_ids.filter(|v| !v.is_empty()) {
        let results =
            crate::engine::guest_context::gather_fleet_snapshots(pool, cfg, ids, false).await;
        for (_, r) in results {
            if let Ok(s) = r {
                guest_snapshots.push(s);
            }
        }
    } else if let Some(id) = vm_id {
        if let Ok(s) = crate::engine::guest_context::snapshot_for_vm(pool, cfg, id, false).await {
            guest_snapshots.push(s);
        }
    }
    let mut reply = String::new();

    let ml = message.to_lowercase();
    if let Some(ref host) = ctx.host {
        if ml.contains("pressure")
            || ml.contains("linux")
            || ml.contains("smart")
            || ml.contains("thermal")
        {
            if let Some(io) = host.io_pressure_pct {
                reply.push_str(&format!(
                    "**{}** Linux IO pressure: **{:.0}%**.\n",
                    host.hostname, io
                ));
            }
            if let Some(ref s) = host.linux_summary {
                reply.push_str(&format!("{s}\n"));
            }
            reply.push_str(&format!("Host state: **{}**.\n\n", host.state));
        }
    }
    if ml.contains("unhealthy") || ml.contains("slow") || ml.contains("why") && ml.contains("vm") {
        if let Some(ref vm) = ctx.vm {
            if let Ok(id) = Uuid::parse_str(&vm.id) {
                let health = crate::engine::vm_health::run_vm_health_check(pool, id).await?;
                reply.push_str(&format!(
                    "**{}** health score: **{}/100** ({}).\n\n",
                    health.vm_name, health.score_numeric, health.score_label
                ));
                for issue in &health.issues {
                    reply.push_str(&format!("- {}: {}\n", issue.severity, issue.message));
                    if let Some(r) = &issue.remediation {
                        reply.push_str(&format!("  _{r}_\n"));
                    }
                }
            }
        } else if let Ok(Some((id, name))) = crate::db::query_as::<_, (Uuid, String)>(
            "SELECT id, name FROM vms WHERE name LIKE ? ESCAPE '\\' LIMIT 1",
        )
        .bind(format!(
            "%{}%",
            escape_like(
                message
                    .split_whitespace()
                    .find(|w| w.len() > 2)
                    .unwrap_or("")
            )
        ))
        .fetch_optional(pool)
        .await
        {
            let health = crate::engine::vm_health::run_vm_health_check(pool, id).await?;
            reply.push_str(&format!(
                "**{name}** health: **{}/100**.\n",
                health.score_numeric
            ));
            for issue in health.issues.iter().take(5) {
                reply.push_str(&format!("- {}\n", issue.message));
            }
        } else {
            reply.push_str(&format!(
                "Cluster has {} VMs and {} hosts online. Open a VM detail page or name a VM for Doctor analysis.\n",
                ctx.cluster_vms, ctx.cluster_hosts_online
            ));
        }
    } else if ml.contains("capacity") || ml.contains("onboard") {
        let cap = crate::engine::ai::capacity::plan(pool).await?;
        reply.push_str(&format!(
            "Cluster memory headroom: **{} MiB**. CPU avg: **{:.0}%**. Estimated VMs you can add (small): **{}**.\n",
            cap.memory_headroom_mib, cap.avg_cpu_percent, cap.estimated_small_vms_addable
        ));
    } else if ml.contains("cost") || ml.contains("save") {
        let cost = crate::engine::ai::cost::analyze(pool).await?;
        reply.push_str(&format!(
            "Estimated monthly cost: **${:.2}**. Findings: {} idle, {} oversized VMs.\n",
            cost.estimated_monthly_usd, cost.idle_vm_count, cost.oversized_vm_count
        ));
    } else if ml.contains("security") || ml.contains("risk") || ml.contains("sentinel") {
        let sec = crate::engine::ai::security::scan(pool).await?;
        reply.push_str(&format!(
            "Security risk level: **{}**. **{}** finding(s).\n",
            sec.risk_level,
            sec.findings.len()
        ));
        for f in sec.findings.iter().take(5) {
            reply.push_str(&format!("- [{}] {}: {}\n", f.severity, f.title, f.detail));
        }
    } else if ml.contains("migrat") {
        let vm_hint = message
            .split_whitespace()
            .find(|w| {
                w.len() > 2
                    && !["why", "the", "vm", "can", "how"].contains(&w.to_lowercase().as_str())
            })
            .unwrap_or("web-01");
        let adv = crate::engine::ai::migration::advise_vmware_vm(vm_hint, "linux", false);
        reply.push_str(&format!(
            "**{}** migration readiness: **{}%**.\n",
            adv.vm_name, adv.readiness_percent
        ));
        for r in &adv.risks {
            reply.push_str(&format!("- Risk: {r}\n"));
        }
        for s in adv.safe.iter().take(3) {
            reply.push_str(&format!("- OK: {s}\n"));
        }
    } else if ml.contains("reach")
        || ml.contains("connect")
        || (ml.contains("can't") && ml.contains("to"))
    {
        if let Some((a, b, port)) = parse_reach_query(message) {
            let net = crate::engine::ai::network::explain_reach(pool, cfg, &a, &b, port).await?;
            reply.push_str(&format!(
                "Network path **{} → {}**: {}\n\nHops: {}\n\nRemediation: {}",
                a,
                b,
                if net.can_reach {
                    "likely reachable"
                } else {
                    "blocked or unknown"
                },
                net.hops.join(" → "),
                net.remediation
            ));
            reply.push_str(&format!("\n\n{}", net.explanation));
        } else {
            reply.push_str("Name two VMs to analyze connectivity, e.g. \"Why can't app-01 reach db-01 on port 5432?\"\n");
        }
    } else if ml.contains("troubleshoot")
        || (ml.contains("slow") && (ml.contains("vm") || ctx.vm.is_some()))
        || ml.contains("unreachable")
        || (ml.contains("disk") && ml.contains("vm"))
    {
        let req = crate::engine::ai::troubleshoot::TroubleshootRequest {
            vm_id,
            vm_name: ctx.vm.as_ref().map(|v| v.name.clone()),
            symptom: if ml.contains("slow") {
                "slow".into()
            } else if ml.contains("unreachable") {
                "unreachable".into()
            } else if ml.contains("disk") {
                "disk".into()
            } else if ml.contains("network") {
                "network".into()
            } else {
                "slow".into()
            },
        };
        if req.vm_id.is_some() || req.vm_name.is_some() {
            let report = crate::engine::ai::troubleshoot::diagnose(pool, &req).await?;
            reply.push_str(&format!(
                "**{}** troubleshoot ({}) — severity **{}**\n\n",
                report.vm_name, report.symptom, report.severity
            ));
            for f in report.findings.iter().take(5) {
                reply.push_str(&format!("- [{}] {}: {}\n", f.domain, f.severity, f.message));
            }
            for a in report.recommended_actions.iter().take(3) {
                reply.push_str(&format!("  → {a}\n"));
            }
        } else {
            reply.push_str("Open a VM detail page or name a VM for troubleshoot analysis.\n");
        }
    } else if (ml.contains("create") && (ml.contains("vm") || ml.contains("ubuntu")))
        || (ml.contains("migrate") && ml.contains("from"))
        || ml.contains("risky infra")
        || (ml.contains("storage") && ml.contains("slow"))
    {
        let plan = crate::engine::ai::nl_ops::execute(
            pool,
            &crate::engine::ai::nl_ops::NlOpsRequest {
                query: message.to_string(),
                dry_run: true,
            },
            "copilot",
        )
        .await?;
        reply.push_str(&format!(
            "**NL Ops plan** ({}) — risk {}/10\n\n",
            plan.intent, plan.risk_score
        ));
        reply.push_str(&format!("{}\n\n", plan.summary));
        for step in &plan.steps {
            reply.push_str(&format!(
                "- {} ({}) — {}\n",
                step.label, step.action_type, step.review
            ));
        }
        reply.push_str(
            "\nUse Ask Zyra or `/api/v1/ai/nl-ops` with `dry_run: false` to queue approvals.\n",
        );
    } else {
        reply.push_str(&format!(
            "Zyra (advisor mode). Cluster: **{} VMs**, **{} hosts online**, **{} open recommendations**.\n\nAsk about VM health, capacity, cost, security, migrations, or network reachability.",
            ctx.cluster_vms, ctx.cluster_hosts_online, ctx.recommendations_count
        ));
    }

    let context_summary = if !guest_snapshots.is_empty() {
        if guest_snapshots.len() == 1 {
            crate::engine::guest_context::context_chip(&guest_snapshots[0])
        } else {
            format!(
                "{} VMs · {} with guest data",
                guest_snapshots.len(),
                guest_snapshots.iter().filter(|s| s.agent_ping).count()
            )
        }
    } else {
        format!("{} VMs", ctx.cluster_vms)
    };

    let mut ctx_payload = serde_json::json!({
        "cluster": ctx,
        "guest_snapshots": guest_snapshots,
    });
    if !message.is_empty() {
        ctx_payload["user_message"] = serde_json::Value::String(message.to_string());
    }
    if guest_snapshots.len() == 1 && message.to_lowercase().contains("guest") {
        reply.push_str(&format!(
            "**Guest context:** {}\n\n",
            crate::engine::guest_context::context_chip(&guest_snapshots[0])
        ));
    }

    Ok(CopilotBase {
        reply,
        context_summary,
        ctx_json: ctx_payload.to_string(),
    })
}

pub fn chunk_text(text: &str, chunk_size: usize) -> Vec<String> {
    let size = chunk_size.max(8);
    let mut chunks = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if rest.len() <= size {
            chunks.push(rest.to_string());
            break;
        }
        // Snap the hard cut down to a UTF-8 char boundary so we never split a multibyte char.
        let mut boundary = size;
        while boundary > 0 && !rest.is_char_boundary(boundary) {
            boundary -= 1;
        }
        if boundary == 0 {
            // A single char is larger than `size`; take at least one whole char to make progress.
            boundary = rest
                .char_indices()
                .nth(1)
                .map(|(i, _)| i)
                .unwrap_or(rest.len());
        }
        // Prefer breaking at the last word boundary within the safe slice. Space is ASCII,
        // so `sp + 1` is always a valid char boundary.
        let mut end = boundary;
        if let Some(sp) = rest[..boundary].rfind(' ') {
            end = sp + 1;
        }
        chunks.push(rest[..end].to_string());
        rest = &rest[end..];
    }
    chunks
}

pub async fn explain_screen(
    pool: &DbPool,
    screen: &str,
    object_ref: &serde_json::Value,
) -> anyhow::Result<String> {
    let mut base = match screen {
        "vm_detail" => {
            if let Some(id) = object_ref.get("vm_id").and_then(|v| v.as_str()) {
                if let Ok(uid) = Uuid::parse_str(id) {
                    let h = crate::engine::vm_health::run_vm_health_check(pool, uid).await?;
                    format!(
                        "VM **{}** — health **{}/100**. {} checks passed. {} issue(s).",
                        h.vm_name,
                        h.score_numeric,
                        h.checks_passed,
                        h.issues.len()
                    )
                } else {
                    "VM detail view.".into()
                }
            } else {
                "VM detail view.".into()
            }
        }
        "migration" => {
            "Migration Assistant imports workloads from VMware, OVF/OVA, or cloud images.".into()
        }
        "storage" => "Storage pools are imported from libvirt on online hosts.".into(),
        "templates" => "Templates deploy golden images when disk readiness passes.".into(),
        "tasks" => "Tasks queue orchestration operations with progress and retry.".into(),
        "vm_doctor" => "Zyra SRE scores VM health 0–100 with actionable fixes.".into(),
        "console_hub" => {
            "Zyra ConsoleHub Machine Cockpit — display, serial, SSH, and AI recovery lenses.".into()
        }
        "failed_task" => "Failed tasks include remediation via runbooks and retry.".into(),
        "notification" => {
            "Alerts surface operational issues with Explain and Runbook actions.".into()
        }
        _ => format!("Screen: {screen}"),
    };

    if let Ok(Some(llm)) = llm::complete_simple(
        pool,
        "Explain infrastructure UI screens in plain language for operators.",
        &format!("Screen: {screen}\nRef: {object_ref}\nBase: {base}"),
    )
    .await
    {
        base.push_str("\n\n");
        base.push_str(&llm);
    }
    Ok(base)
}

pub mod autopilot;
pub mod performance_autopilot;
pub mod blueprint;
pub mod blackbox_rca;
pub mod capacity;
pub mod compliance;
pub mod compliance_frameworks;
pub mod compliance_remediate;
pub mod cost;
pub mod cost_attribution;
pub mod cost_budget;
pub mod digital_twin;
pub mod environment_intent;
pub mod exposure_finops;
pub mod firewall;
pub mod firewall_remediate;
pub mod fleet_heatmap;
pub mod fleet_placement;
pub mod fleet_power;
pub mod fleet_rebalance;
pub mod fleet_summary;
pub mod incident_commander;
pub mod infra_graph;
pub mod infrastructure_memory;
pub mod knowledge_diagnose;
pub mod knowledge_runbook;
pub mod knowledge_search;
pub mod migration;
pub mod mission_stack;
pub mod mission_stack_status;
pub mod network;
pub mod nl_ops;
pub mod policy_export;
pub mod predictions;
pub mod remediate_hub;
pub mod root_cause;
pub mod noisy_neighbor;
pub mod runbook;
pub mod security;
pub mod security_graph;
pub mod service_graph;
pub mod service_impact;
pub mod sre_predict;
pub mod sre_remediate;
pub mod terminal;
pub mod troubleshoot;
pub mod worker;
pub mod zyra_summary;

/// Escape SQLite LIKE metacharacters in user-supplied text before it is
/// wrapped in `%...%` and bound to a `LIKE ? ESCAPE '\\'` clause — otherwise
/// a chat message containing `%` or `_` silently widens the match (e.g. `_`
/// matches any single character) and can select an unintended VM.
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn parse_reach_query(message: &str) -> Option<(String, String, Option<i32>)> {
    let ml = message.to_lowercase();
    let port = ml
        .split_whitespace()
        .find_map(|w| {
            w.strip_prefix("port")
                .or_else(|| w.strip_prefix(':'))
                .and_then(|p| p.parse::<i32>().ok())
        })
        .or_else(|| {
            ml.split_whitespace()
                .filter_map(|w| w.parse::<i32>().ok())
                .find(|p| *p > 0 && *p < 65536)
        });

    if let Some(idx) = ml.find(" reach ") {
        let rest = &ml[idx + 7..];
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 3 && parts[1].eq_ignore_ascii_case("to") {
            return Some((
                parts[0].to_string(),
                parts[2].trim_matches(|c| c == '?' || c == '.').to_string(),
                port,
            ));
        }
    }

    let tokens: Vec<&str> = message.split_whitespace().collect();
    for (i, t) in tokens.iter().enumerate() {
        if t.eq_ignore_ascii_case("reach") && i + 2 < tokens.len() {
            return Some((
                tokens[i + 1]
                    .trim_matches(|c| c == '?' || c == '.')
                    .to_string(),
                tokens[i + 2]
                    .trim_matches(|c| c == '?' || c == '.')
                    .to_string(),
                port,
            ));
        }
    }

    if let (Some(a_pos), Some(b_pos)) = (
        tokens
            .iter()
            .position(|t| t.contains('-') || t.chars().any(|c| c.is_ascii_digit())),
        tokens
            .iter()
            .rposition(|t| t.contains('-') || t.chars().any(|c| c.is_ascii_digit())),
    ) {
        if a_pos != b_pos {
            return Some((
                tokens[a_pos]
                    .trim_matches(|c| c == '?' || c == '.')
                    .to_string(),
                tokens[b_pos]
                    .trim_matches(|c| c == '?' || c == '.')
                    .to_string(),
                port,
            ));
        }
    }
    None
}
