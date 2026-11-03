// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Change Review: "what happens if I approve this?" for a pending Zyra action.
//!
//! Everything here is deterministic and read-only. It looks at the machine the action names, the
//! application groups it belongs to, the other machines on its host and the history of the same action
//! class, and returns a verdict a person can read in a few seconds. An LLM may rephrase the summary
//! elsewhere; nothing in this module needs one, and nothing in it changes state.

use serde::Serialize;
use uuid::Uuid;

use crate::db::DbPool;

/// What the review found about the machine an action touches.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Blast {
    pub vm: Option<String>,
    pub observed_state: Option<String>,
    pub host: Option<String>,
    /// Other machines on the same host.
    pub host_peers: i64,
    pub groups: Vec<String>,
    /// Distinct machines that share an application group with this one (including itself).
    pub group_members: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct ChangeReview {
    pub action_id: Uuid,
    pub action_type: String,
    /// `low` | `medium` | `high`
    pub risk: String,
    /// `approve` | `approve_with_care` | `hold`
    pub verdict: String,
    pub summary: String,
    pub blast_radius: Vec<String>,
    pub rollback: String,
    pub reversible: bool,
    pub checks: Vec<String>,
    /// Consecutive prior approvals of this class with no rejection or failure.
    pub approved_streak: usize,
}

/// Classes whose reverse is exact when the machine was in the expected state (see `action_audit::undo_kind`).
const REVERSIBLE: &[&str] = &["enable_ha", "start_vm", "stop_vm", "create_backup"];
/// Classes that cannot be taken back.
const DESTRUCTIVE: &[&str] = &["delete_vm", "delete_volume", "firewall_change"];

pub fn is_reversible(action_type: &str) -> bool {
    REVERSIBLE.contains(&action_type)
}

fn base_risk(action_type: &str, declared: &str) -> u8 {
    let declared = match declared {
        "high" | "critical" => 2,
        "medium" => 1,
        _ => 0,
    };
    let by_class = if DESTRUCTIVE.contains(&action_type) {
        2
    } else if is_reversible(action_type) {
        0
    } else {
        1
    };
    declared.max(by_class)
}

/// Combine the action, what the blast looks like and the history into a verdict.
pub fn assess(
    action_id: Uuid,
    action_type: &str,
    declared_risk: &str,
    blast: &Blast,
    approved_streak: usize,
) -> ChangeReview {
    let mut score = base_risk(action_type, declared_risk);
    let mut radius = Vec::new();
    let mut checks = Vec::new();

    match &blast.vm {
        Some(vm) => {
            let state = blast.observed_state.as_deref().unwrap_or("unknown");
            radius.push(format!("Machine `{vm}` ({state})"));
            if let Some(h) = &blast.host {
                radius.push(format!(
                    "Host `{h}` runs {} other machine(s)",
                    blast.host_peers.saturating_sub(1).max(0)
                ));
            }
        }
        None => checks.push("The action does not name a machine; its reach is unknown.".to_string()),
    }
    if !blast.groups.is_empty() {
        radius.push(format!(
            "Application group(s) {}: {} machine(s) share them",
            blast.groups.join(", "),
            blast.group_members
        ));
        if blast.group_members >= 5 && !is_reversible(action_type) {
            score = score.max(2);
        } else if blast.group_members >= 2 {
            score = score.max(1);
        }
        checks.push("Confirm the other machines in the group can carry the load.".to_string());
    }
    if blast.vm.is_none() && !is_reversible(action_type) {
        score = score.max(1);
    }

    let reversible = is_reversible(action_type);
    let rollback = if reversible {
        "Zyra records the machine's state first, verifies the result, and offers a one-click undo.".to_string()
    } else if DESTRUCTIVE.contains(&action_type) {
        "None. This cannot be undone; take a backup first.".to_string()
    } else {
        "No automatic undo. Verify the result afterwards and revert by hand if needed.".to_string()
    };
    if !reversible {
        checks.push("Make sure a recent backup exists before approving.".to_string());
    }

    let risk = ["low", "medium", "high"][score as usize].to_string();
    let verdict = match score {
        0 => "approve",
        1 => "approve_with_care",
        _ => "hold",
    }
    .to_string();
    let summary = match score {
        0 => format!("Low risk and reversible. {} prior approval(s) of this kind in a row.", approved_streak),
        1 => "Reasonable to approve once the checks below are done.".to_string(),
        _ => "Wide or irreversible. Hold until someone has checked the blast radius and has a backup.".to_string(),
    };

    ChangeReview {
        action_id,
        action_type: action_type.to_string(),
        risk,
        verdict,
        summary,
        blast_radius: radius,
        rollback,
        reversible,
        checks,
        approved_streak,
    }
}

fn vm_name(object_ref: &serde_json::Value) -> Option<String> {
    ["vm", "vm_name", "name"]
        .iter()
        .find_map(|k| object_ref.get(*k).and_then(|v| v.as_str()))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub async fn review(pool: &DbPool, id: Uuid) -> anyhow::Result<Option<ChangeReview>> {
    let Some(action) = super::actions::get_action(pool, id).await? else {
        return Ok(None);
    };
    let mut blast = Blast::default();

    let mut named = vm_name(&action.object_ref);
    if named.is_none() {
        // Some actions (guest tools, time sync) identify the machine by id.
        if let Some(vid) = action
            .object_ref
            .get("vm_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
        {
            named = crate::db::query_scalar("SELECT name FROM vms WHERE id = ? LIMIT 1")
                .bind(vid)
                .fetch_optional(pool)
                .await?;
        }
    }

    if let Some(name) = named {
        let row: Option<(Uuid, String, Option<Uuid>, Option<String>)> = crate::db::query_as(
            "SELECT v.id, v.observed_state, v.host_id, h.hostname
             FROM vms v LEFT JOIN hosts h ON h.id = v.host_id WHERE v.name = ? LIMIT 1",
        )
        .bind(&name)
        .fetch_optional(pool)
        .await?;
        if let Some((vm_id, state, host_id, host)) = row {
            blast.vm = Some(name);
            blast.observed_state = Some(state);
            blast.host = host;
            if let Some(hid) = host_id {
                blast.host_peers = crate::db::query_scalar("SELECT COUNT(*) FROM vms WHERE host_id = ?")
                    .bind(hid)
                    .fetch_one(pool)
                    .await?;
            }
            blast.groups = crate::db::query_scalar(
                "SELECT g.name FROM application_group_vms agv
                 JOIN application_groups g ON g.id = agv.group_id WHERE agv.vm_id = ? ORDER BY g.name",
            )
            .bind(vm_id)
            .fetch_all(pool)
            .await?;
            if !blast.groups.is_empty() {
                blast.group_members = crate::db::query_scalar(
                    "SELECT COUNT(DISTINCT vm_id) FROM application_group_vms
                     WHERE group_id IN (SELECT group_id FROM application_group_vms WHERE vm_id = ?)",
                )
                .bind(vm_id)
                .fetch_one(pool)
                .await?;
            }
        }
    }

    let statuses: Vec<String> = crate::db::query_scalar(
        "SELECT status FROM ai_actions WHERE action_type = ? AND status <> 'pending' AND id <> ?
         ORDER BY created_at DESC LIMIT 20",
    )
    .bind(&action.action_type)
    .bind(id)
    .fetch_all(pool)
    .await?;
    let (streak, _, _) = super::trust::tally(&statuses);

    Ok(Some(assess(id, &action.action_type, &action.risk, &blast, streak)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blast(groups: &[&str], members: i64) -> Blast {
        Blast {
            vm: Some("web-1".into()),
            observed_state: Some("running".into()),
            host: Some("kvm-a".into()),
            host_peers: 4,
            groups: groups.iter().map(|s| s.to_string()).collect(),
            group_members: members,
        }
    }

    #[test]
    fn reversible_single_machine_is_low_risk() {
        let r = assess(Uuid::nil(), "start_vm", "low", &blast(&[], 0), 6);
        assert_eq!((r.risk.as_str(), r.verdict.as_str()), ("low", "approve"));
        assert!(r.reversible);
        assert_eq!(r.approved_streak, 6);
    }

    #[test]
    fn group_membership_raises_risk() {
        let r = assess(Uuid::nil(), "start_vm", "low", &blast(&["shop"], 3), 0);
        assert_eq!(r.risk, "medium");
        assert!(r.blast_radius.iter().any(|l| l.contains("shop")));
    }

    #[test]
    fn irreversible_action_on_a_big_group_is_held() {
        let r = assess(Uuid::nil(), "migrate_vm", "low", &blast(&["shop"], 8), 0);
        assert_eq!(r.verdict, "hold");
        assert!(!r.reversible);
    }

    #[test]
    fn destructive_class_is_high_even_with_no_machine() {
        let r = assess(Uuid::nil(), "delete_vm", "low", &Blast::default(), 0);
        assert_eq!(r.risk, "high");
        assert!(r.rollback.starts_with("None"));
        assert!(r.checks.iter().any(|c| c.contains("does not name")));
    }

    #[test]
    fn declared_risk_is_never_lowered() {
        let r = assess(Uuid::nil(), "start_vm", "high", &blast(&[], 0), 9);
        assert_eq!(r.risk, "high");
    }

    #[test]
    fn machine_name_is_read_from_common_keys() {
        assert_eq!(vm_name(&serde_json::json!({"vm_name": "a"})).as_deref(), Some("a"));
        assert_eq!(vm_name(&serde_json::json!({"vm": ""})), None);
    }

    #[tokio::test]
    async fn review_of_a_missing_action_is_none() {
        let pool = crate::db::testing::pool().await;
        assert!(review(&pool, Uuid::new_v4()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn review_of_a_real_action_without_a_machine_still_answers() {
        let pool = crate::db::testing::pool().await;
        let id = Uuid::new_v4();
        crate::db::query(
            "INSERT INTO ai_actions (id, source, action_type, label, review, risk, object_ref, status, requested_by)
             VALUES (?, 'test', 'start_vm', 'Start x', '', 'low', '{\"vm\":\"ghost\"}', 'pending', 'tester')",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
        let r = review(&pool, id).await.unwrap().unwrap();
        assert_eq!(r.action_type, "start_vm");
        assert!(r.checks.iter().any(|c| c.contains("does not name")));
    }

    #[tokio::test]
    async fn review_finds_the_machine_by_vm_id() {
        let pool = crate::db::testing::pool().await;
        let vm_id = Uuid::new_v4();
        crate::db::query(
            "INSERT INTO vms (id, name, project, spec_json, desired_state, lifecycle_phase, vcpus, memory_mib, tags)
             VALUES (?, 'by-id-vm', 'p', '{}', 'running', 'running', 1, 512, '[]')",
        )
        .bind(vm_id)
        .execute(&pool)
        .await
        .unwrap();
        let id = Uuid::new_v4();
        crate::db::query(
            "INSERT INTO ai_actions (id, source, action_type, label, review, risk, object_ref, status, requested_by)
             VALUES (?, 'test', 'start_vm', 'Start x', '', 'low', ?, 'pending', 'tester')",
        )
        .bind(id)
        .bind(serde_json::json!({ "vm_id": vm_id.to_string() }).to_string())
        .execute(&pool)
        .await
        .unwrap();
        let r = review(&pool, id).await.unwrap().unwrap();
        assert!(!r.checks.iter().any(|c| c.contains("does not name")));
    }
}
