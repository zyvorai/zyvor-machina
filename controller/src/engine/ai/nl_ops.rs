// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use super::actions::{self, CreateActionBody};
use super::environment_intent;
use super::infra_graph::{self, GraphQueryRequest};
use super::predictions;
use super::troubleshoot;

#[derive(Debug, Deserialize)]
pub struct NlOpsRequest {
    pub query: String,
    #[serde(default = "default_dry_run")]
    pub dry_run: bool,
}

fn default_dry_run() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct NlOpsStep {
    pub label: String,
    pub action_type: String,
    pub review: String,
    pub risk: String,
}

#[derive(Debug, Serialize)]
pub struct NlOpsPlan {
    pub intent: String,
    pub summary: String,
    pub steps: Vec<NlOpsStep>,
    pub risk_score: u8,
    pub dry_run: bool,
    pub approval_required: bool,
    pub action_ids: Vec<Uuid>,
    pub reply: String,
}

pub async fn execute(
    pool: &DbPool,
    req: &NlOpsRequest,
    actor: &str,
) -> anyhow::Result<NlOpsPlan> {
    let q = req.query.trim();
    let ql = q.to_lowercase();

    // Create a volume / create a security group are checked BEFORE the generic "Create
    // VMs" block below, even though both "create a volume" and "create a security
    // group allowing ssh to my vm" can legitimately contain the substring "vm" —
    // ordering these more-specific intents first (rather than piling exclusion
    // conditions onto the VM check) means a phrase mentioning "vm" incidentally still
    // routes to the intent it's actually asking for.

    // Create a volume — unlike "create VMs" below (queued but no executor wired, see
    // actions::approve_and_execute), this genuinely creates the volume when approved:
    // "create_volume" has a real executor calling api::volumes::create_volume.
    if ql.contains("create") && (ql.contains("volume") || ql.contains("disk")) {
        let size_gib = extract_size_gib(&ql).unwrap_or(10);
        let name = extract_named(&ql)
            .unwrap_or_else(|| format!("vol-{}", &Uuid::new_v4().to_string()[..8]));
        let object_ref =
            serde_json::json!({ "name": name, "size_gib": size_gib, "volume_class": "silver" });
        let step = NlOpsStep {
            label: format!("Create {size_gib}GiB volume '{name}'"),
            action_type: "create_volume".into(),
            review: format!(
                "Provision a {size_gib}GiB volume named '{name}' (Atlas-backed if enabled)"
            ),
            risk: "low".into(),
        };
        let mut action_ids = Vec::new();
        if !req.dry_run {
            let row = actions::create_action(
                pool,
                &CreateActionBody {
                    action_type: step.action_type.clone(),
                    label: step.label.clone(),
                    review: step.review.clone(),
                    risk: step.risk.clone(),
                    object_ref,
                    source: "nl_ops".into(),
                },
                actor,
            )
            .await?;
            action_ids.push(row.id);
        }
        return Ok(NlOpsPlan {
            intent: "create_volume".into(),
            summary: format!("Create a {size_gib}GiB volume"),
            steps: vec![step],
            risk_score: 3,
            dry_run: req.dry_run,
            approval_required: true,
            action_ids,
            reply: format!(
                "Prepared a {size_gib}GiB volume named '{name}' — approve in Zyra queue."
            ),
        });
    }

    // Create a security group — "create_security_group_allow" also has a real
    // executor (api::networking::{create_security_group, create_security_group_rule}).
    if ql.contains("security group")
        || (ql.contains("create") && ql.contains("firewall") && ql.contains("allow"))
    {
        let name = extract_named(&ql)
            .unwrap_or_else(|| format!("sg-{}", &Uuid::new_v4().to_string()[..8]));
        let (protocol, port) = extract_service(&ql).unwrap_or(("tcp", 22));
        let object_ref = serde_json::json!({ "name": name, "protocol": protocol, "port": port });
        let step = NlOpsStep {
            label: format!("Create security group '{name}' allowing {protocol}/{port}"),
            action_type: "create_security_group_allow".into(),
            review: format!(
                "New security group '{name}' with one ingress rule: {protocol}/{port} from 0.0.0.0/0. \
                 Note: rule enforcement isn't wired to the firewall yet — this record is advisory-only \
                 and does not actually restrict traffic."
            ),
            risk: "medium".into(),
        };
        let mut action_ids = Vec::new();
        if !req.dry_run {
            let row = actions::create_action(
                pool,
                &CreateActionBody {
                    action_type: step.action_type.clone(),
                    label: step.label.clone(),
                    review: step.review.clone(),
                    risk: step.risk.clone(),
                    object_ref,
                    source: "nl_ops".into(),
                },
                actor,
            )
            .await?;
            action_ids.push(row.id);
        }
        return Ok(NlOpsPlan {
            intent: "create_security_group".into(),
            summary: format!("Create security group '{name}'"),
            steps: vec![step],
            risk_score: 4,
            dry_run: req.dry_run,
            approval_required: true,
            action_ids,
            reply: format!(
                "Prepared security group '{name}' allowing {protocol}/{port} — approve in Zyra queue. \
                 Note: this is advisory only for now; it does not yet enforce traffic."
            ),
        });
    }

    // Create VMs — checked after the more specific volume/security-group intents above.
    if ql.contains("create") && (ql.contains("vm") || ql.contains("ubuntu")) {
        let count = extract_count(&ql).unwrap_or(1);
        let rates: (f64, f64) = crate::db::query_as(
            "SELECT finops_vcpu_hour_usd, finops_gib_hour_usd FROM clusters ORDER BY created_at LIMIT 1",
        )
        .fetch_one(pool)
        .await
        .unwrap_or((0.02, 0.005));
        let plan = environment_intent::plan_environment(q, rates.0, rates.1);
        let mut steps = Vec::new();
        for i in 0..count.min(plan.vm_count) {
            steps.push(NlOpsStep {
                label: format!("Create VM {i}"),
                action_type: "create_vm".into(),
                review: plan.label.clone(),
                risk: "medium".into(),
            });
        }
        let mut action_ids = Vec::new();
        if !req.dry_run {
            for step in &steps {
                let row = actions::create_action(
                    pool,
                    &CreateActionBody {
                        action_type: step.action_type.clone(),
                        label: step.label.clone(),
                        review: step.review.clone(),
                        risk: step.risk.clone(),
                        object_ref: serde_json::json!({}),
                        source: "nl_ops".into(),
                    },
                    actor,
                )
                .await?;
                action_ids.push(row.id);
            }
        }
        return Ok(NlOpsPlan {
            intent: "create_vms".into(),
            summary: format!("Create {count} VM(s) from environment plan"),
            steps,
            risk_score: 6,
            dry_run: req.dry_run,
            approval_required: true,
            action_ids,
            reply: plan.label.clone(),
        });
    }

    // Migrate all VMs from host
    if ql.contains("migrate") && ql.contains("from") {
        let host_hint = extract_host_hint(&ql);
        let vms: Vec<(Uuid, String)> = if let Some(h) = &host_hint {
            let escaped = h
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            crate::db::query_as(
                "SELECT v.id, v.name FROM vms v JOIN hosts h ON h.id = v.host_id
                 WHERE h.hostname LIKE ? ESCAPE '\\'",
            )
            .bind(format!("%{escaped}%"))
            .fetch_all(pool)
            .await?
        } else {
            crate::db::query_as("SELECT id, name FROM vms WHERE observed_state = 'running' LIMIT 10")
                .fetch_all(pool)
                .await?
        };
        let mut steps = Vec::new();
        for (id, name) in &vms {
            steps.push(NlOpsStep {
                label: format!("Migrate {name}"),
                action_type: "migrate_vm".into(),
                review: format!("Live migrate VM {name} ({id}) off source host"),
                risk: "high".into(),
            });
        }
        let mut action_ids = Vec::new();
        if !req.dry_run {
            for step in &steps {
                let row = actions::create_action(
                    pool,
                    &CreateActionBody {
                        action_type: step.action_type.clone(),
                        label: step.label.clone(),
                        review: step.review.clone(),
                        risk: step.risk.clone(),
                        object_ref: serde_json::json!({}),
                        source: "nl_ops".into(),
                    },
                    actor,
                )
                .await?;
                action_ids.push(row.id);
            }
        }
        return Ok(NlOpsPlan {
            intent: "migrate_vms_from_host".into(),
            summary: format!(
                "Migrate {} VM(s) from {}",
                vms.len(),
                host_hint.unwrap_or_else(|| "fleet".into())
            ),
            steps,
            risk_score: 8,
            dry_run: req.dry_run,
            approval_required: true,
            action_ids,
            reply: format!(
                "Prepared {} migration(s) — approve in Zyra queue.",
                vms.len()
            ),
        });
    }

    // Why is storage slow
    if ql.contains("storage") && (ql.contains("slow") || ql.contains("latency")) {
        let cap = super::capacity::plan(pool).await?;
        let preds = predictions::unified(pool).await?;
        let storage_msg = preds
            .predictions
            .iter()
            .find(|p| p.kind.contains("storage") || p.resource_kind == "storage")
            .map(|p| p.message.clone());
        return Ok(NlOpsPlan {
            intent: "explain_slow_storage".into(),
            summary: "Storage performance analysis".into(),
            steps: vec![NlOpsStep {
                label: "Review storage pools".into(),
                action_type: "navigate".into(),
                review: format!(
                    "Used {:.0} GiB / {:.0} GiB — runway {:?} days",
                    cap.storage_used_gib, cap.storage_capacity_gib, cap.storage_runway_days
                ),
                risk: "low".into(),
            }],
            risk_score: if storage_msg.is_some() { 7 } else { 3 },
            dry_run: true,
            approval_required: false,
            action_ids: vec![],
            reply: storage_msg.unwrap_or_else(|| {
                format!(
                    "Storage: {:.0}/{:.0} GiB used. Check pool latency and SMART on hosts.",
                    cap.storage_used_gib, cap.storage_capacity_gib
                )
            }),
        });
    }

    // Show risky infrastructure
    if ql.contains("risky") || ql.contains("risk") {
        let preds = predictions::unified(pool).await?;
        let cost = super::cost::analyze(pool).await?;
        return Ok(NlOpsPlan {
            intent: "show_risky_infra".into(),
            summary: "Risk rollup".into(),
            steps: preds
                .predictions
                .iter()
                .take(5)
                .map(|p| NlOpsStep {
                    label: p.message.clone(),
                    action_type: "alert".into(),
                    review: p.evidence.clone(),
                    risk: p.severity.clone(),
                })
                .collect(),
            risk_score: 5,
            dry_run: true,
            approval_required: false,
            action_ids: vec![],
            reply: format!(
                "{} prediction(s); {} idle VMs; {} oversized.",
                preds.predictions.len(),
                cost.idle_vm_count,
                cost.oversized_vm_count
            ),
        });
    }

    // VM is slow — troubleshoot
    if ql.contains("slow") {
        if let Some(name) = extract_vm_name(&ql) {
            let report = troubleshoot::diagnose(
                pool,
                &troubleshoot::TroubleshootRequest {
                    vm_id: None,
                    vm_name: Some(name.clone()),
                    symptom: "slow".into(),
                },
            )
            .await?;
            return Ok(NlOpsPlan {
                intent: "troubleshoot_vm".into(),
                summary: format!("Diagnosis for {name}"),
                steps: report
                    .findings
                    .iter()
                    .map(|f| NlOpsStep {
                        label: f.message.clone(),
                        action_type: "finding".into(),
                        review: f.domain.clone(),
                        risk: f.severity.clone(),
                    })
                    .collect(),
                risk_score: match report.severity.as_str() {
                    "critical" => 9,
                    "high" => 7,
                    _ => 4,
                },
                dry_run: true,
                approval_required: false,
                action_ids: vec![],
                reply: report
                    .findings
                    .first()
                    .map(|f| f.message.clone())
                    .unwrap_or_else(|| "No dominant finding.".into()),
            });
        }
    }

    // Infrastructure search fallback
    let hits = infra_graph::query(pool, &GraphQueryRequest { query: q.into() }).await?;
    Ok(NlOpsPlan {
        intent: "search".into(),
        summary: format!("{} result(s)", hits.hits.len()),
        steps: hits
            .hits
            .iter()
            .take(8)
            .map(|h| NlOpsStep {
                label: h.name.clone(),
                action_type: "navigate".into(),
                review: h.detail.clone(),
                risk: "low".into(),
            })
            .collect(),
        risk_score: 1,
        dry_run: true,
        approval_required: false,
        action_ids: vec![],
        reply: format!("Found {} matching resources.", hits.hits.len()),
    })
}

fn extract_count(ql: &str) -> Option<i32> {
    for word in ql.split_whitespace() {
        if let Ok(n) = word.parse::<i32>() {
            if (1..=50).contains(&n) {
                return Some(n);
            }
        }
    }
    if ql.contains("ten") || ql.contains("10") {
        Some(10)
    } else if ql.contains("three") || ql.contains("3") {
        Some(3)
    } else {
        None
    }
}

fn extract_host_hint(ql: &str) -> Option<String> {
    if let Some(idx) = ql.find("host") {
        let rest = &ql[idx..];
        for word in rest.split_whitespace() {
            if word.starts_with("host-") || word.contains('-') {
                return Some(
                    word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-')
                        .into(),
                );
            }
        }
    }
    None
}

/// Parses a size like "20gb", "20 gb", "20gib" out of a query. Returns the number
/// immediately preceding a gb/gib/g unit token — deliberately not just "any number in
/// the query" (extract_count already grabs the first bare number for VM counts, which
/// would misfire here for e.g. "create a 20gb volume" reading "20" as a VM count).
fn extract_size_gib(ql: &str) -> Option<i64> {
    let words: Vec<&str> = ql.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        let trimmed = word.trim_end_matches(|c: char| !c.is_ascii_digit());
        if trimmed.is_empty() || !trimmed.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let suffix = &word[trimmed.len()..];
        let is_size_unit = matches!(suffix, "gb" | "gib" | "g")
            || words
                .get(i + 1)
                .is_some_and(|next| matches!(*next, "gb" | "gib" | "g" | "gigs" | "gigabytes"));
        if is_size_unit {
            if let Ok(n) = trimmed.parse::<i64>() {
                if n > 0 {
                    return Some(n);
                }
            }
        }
    }
    None
}

/// Pulls a resource name out of "named X" / "called X" — X is the next word, with
/// surrounding punctuation trimmed.
fn extract_named(ql: &str) -> Option<String> {
    for marker in ["named ", "called "] {
        if let Some(idx) = ql.find(marker) {
            let rest = &ql[idx + marker.len()..];
            if let Some(word) = rest.split_whitespace().next() {
                let cleaned = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
                if !cleaned.is_empty() {
                    return Some(cleaned.to_string());
                }
            }
        }
    }
    None
}

/// Maps a common service word, or an explicit "port N", to (protocol, port).
fn extract_service(ql: &str) -> Option<(&'static str, i32)> {
    if let Some(idx) = ql.find("port ") {
        let rest = &ql[idx + 5..];
        if let Some(word) = rest.split_whitespace().next() {
            if let Ok(n) = word
                .trim_matches(|c: char| !c.is_ascii_digit())
                .parse::<i32>()
            {
                return Some(("tcp", n));
            }
        }
    }
    if ql.contains("ssh") {
        Some(("tcp", 22))
    } else if ql.contains("https") {
        Some(("tcp", 443))
    } else if ql.contains("http") {
        Some(("tcp", 80))
    } else if ql.contains("rdp") {
        Some(("tcp", 3389))
    } else if ql.contains("icmp") || ql.contains("ping") {
        Some(("icmp", 0))
    } else {
        None
    }
}

fn extract_vm_name(ql: &str) -> Option<String> {
    for word in ql.split_whitespace() {
        if word.contains('-') && !word.starts_with("host") {
            return Some(word.into());
        }
    }
    None
}
