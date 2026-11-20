// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Trust ladder: each action class is `ask` (a person approves every one) or `auto` (autopilot mode may approve
//! queued proposals of that class itself, capped per run). Only low-blast-radius, reversible classes can climb;
//! network policy, JIT access and anything two-person is never trustable. Zyra offers `auto` after a streak of
//! approvals but never switches it on by itself.

use serde::Serialize;
use crate::db::DbPool;

use crate::auth::AuthUser;
use crate::state::AppState;

/// Classes that may be made automatic. Each has an executor, a verify step and (mostly) an undo.
pub const TRUSTABLE: &[&str] = &[
    "create_backup",
    "enable_ha",
    "install_guest_tools",
    "start_vm",
];
/// Consecutive approvals (with no rejection in between) before Zyra offers to make a class automatic.
pub const OFFER_STREAK: usize = 5;
const DEFAULT_MAX_PER_RUN: i64 = 3;

#[derive(Debug, Serialize, PartialEq)]
pub struct TrustClass {
    pub action_type: String,
    pub level: String,
    pub max_per_run: i64,
    pub approved_streak: usize,
    pub total_approved: usize,
    pub total_rejected: usize,
    /// True when the streak is long enough that Zyra suggests making this automatic.
    pub offer: bool,
}

/// Streak and totals from statuses ordered newest first. `pending` rows are ignored; a rejection ends the streak.
pub fn tally(statuses: &[String]) -> (usize, usize, usize) {
    let mut streak = 0;
    let mut streak_open = true;
    let (mut approved, mut rejected) = (0, 0);
    for s in statuses {
        match s.as_str() {
            "pending" => {}
            "rejected" => {
                rejected += 1;
                streak_open = false;
            }
            "failed" => {
                streak_open = false;
            }
            _ => {
                approved += 1;
                if streak_open {
                    streak += 1;
                }
            }
        }
    }
    (streak, approved, rejected)
}

pub async fn classes(pool: &DbPool) -> anyhow::Result<Vec<TrustClass>> {
    let rules: Vec<(String, String, i64)> =
        crate::db::query_as("SELECT action_type, level, max_per_run FROM ai_trust_rules")
            .fetch_all(pool)
            .await?;
    let rows: Vec<(String, String)> = crate::db::query_as(
        "SELECT action_type, status FROM ai_actions ORDER BY created_at DESC LIMIT 5000",
    )
    .fetch_all(pool)
    .await?;
    Ok(TRUSTABLE
        .iter()
        .map(|t| {
            let statuses: Vec<String> = rows
                .iter()
                .filter(|(a, _)| a == t)
                .map(|(_, s)| s.clone())
                .collect();
            let (streak, approved, rejected) = tally(&statuses);
            let rule = rules.iter().find(|(a, _, _)| a == t);
            let level = rule.map(|r| r.1.clone()).unwrap_or_else(|| "ask".into());
            TrustClass {
                action_type: (*t).to_string(),
                max_per_run: rule.map(|r| r.2).unwrap_or(DEFAULT_MAX_PER_RUN),
                offer: level == "ask" && streak >= OFFER_STREAK,
                level,
                approved_streak: streak,
                total_approved: approved,
                total_rejected: rejected,
            }
        })
        .collect())
}

pub async fn set_rule(
    pool: &DbPool,
    actor: &str,
    action_type: &str,
    level: &str,
    max_per_run: Option<i64>,
) -> anyhow::Result<()> {
    if !TRUSTABLE.contains(&action_type) {
        anyhow::bail!("'{action_type}' can never be automatic; it always needs a person");
    }
    if !matches!(level, "ask" | "auto") {
        anyhow::bail!("level must be 'ask' or 'auto'");
    }
    let cap = max_per_run.unwrap_or(DEFAULT_MAX_PER_RUN).clamp(1, 10);
    crate::db::query(
        "INSERT INTO ai_trust_rules (action_type, level, max_per_run, updated_by, updated_at)
         VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(action_type) DO UPDATE SET level = excluded.level, max_per_run = excluded.max_per_run,
             updated_by = excluded.updated_by, updated_at = CURRENT_TIMESTAMP",
    )
    .bind(action_type)
    .bind(level)
    .bind(cap)
    .bind(actor)
    .execute(pool)
    .await?;
    Ok(())
}

/// Approve queued proposals of `auto` classes (autopilot mode only), at most `max_per_run` per class.
/// Returns how many ran. Every one goes through the normal approve path, so it is audited and undoable.
pub async fn run_trusted_pending(state: &AppState) -> anyhow::Result<usize> {
    let settings = super::settings::get_ai_settings(&state.pool).await?;
    if settings.mode != "autopilot" {
        return Ok(0);
    }
    let auto: Vec<(String, i64)> =
        crate::db::query_as("SELECT action_type, max_per_run FROM ai_trust_rules WHERE level = 'auto'")
            .fetch_all(&state.pool)
            .await?;
    if auto.is_empty() {
        return Ok(0);
    }
    let actor = AuthUser {
        username: "autopilot-trust".into(),
        role: "operator".into(),
        auth_source: None,
    };
    let pending = super::actions::list_pending(&state.pool).await?;
    let mut ran = 0;
    for (ty, cap) in auto {
        if !TRUSTABLE.contains(&ty.as_str()) {
            continue;
        }
        for a in pending
            .iter()
            .filter(|a| a.action_type == ty)
            .take(cap.clamp(1, 10) as usize)
        {
            match super::actions::approve_and_execute(state, a.id, &actor).await {
                Ok(_) => ran += 1,
                Err(e) => tracing::warn!("trusted auto-run of {} failed: {e:#}", a.label),
            }
        }
    }
    Ok(ran)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn streak_counts_newest_approvals_until_a_rejection() {
        assert_eq!(
            tally(&s(&["executed", "executed", "rejected", "executed"])),
            (2, 3, 1)
        );
    }

    #[test]
    fn pending_is_ignored_and_failure_breaks_the_streak() {
        assert_eq!(
            tally(&s(&["pending", "executed", "failed", "executed"])),
            (1, 2, 0)
        );
    }

    #[test]
    fn nothing_dangerous_is_trustable() {
        for t in [
            "firewall_change",
            "vm_network_policy_apply",
            "vm_network_jit",
            "delete_vm",
            "migrate_vm",
        ] {
            assert!(!TRUSTABLE.contains(&t));
        }
    }
}
