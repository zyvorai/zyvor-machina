// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct TemporaryRuleRequest {
    pub target_kind: String,
    pub target_id: Uuid,
    pub source_cidr: String,
    pub dest_port: i32,
    #[serde(default = "default_proto")]
    pub protocol: String,
    pub reason: String,
    pub duration_hours: i32,
    pub owner: Option<String>,
}

fn default_proto() -> String {
    "tcp".into()
}

#[derive(Debug, Clone, Serialize)]
pub struct TemporaryRule {
    pub id: Uuid,
    pub source_cidr: String,
    pub dest_port: i32,
    pub protocol: String,
    pub reason: String,
    pub expires_at: String,
    pub owner: Option<String>,
    // Honest-state fields (bug-hunt fix): no code path pushes this rule to the
    // host's real firewall (core::compile_profile_plan / agent's
    // apply_firewall_plan only compile named catalog profiles — neither reads
    // firewall_temporary_rules at all). Prior to this fix the row was written
    // with `applied = true` and the timeline said "Temporary rule created",
    // which an operator could reasonably read as "a time-limited allow rule is
    // now live and will auto-expire" when nothing on the host had changed.
    // `enforced` must stay `false` until real enforcement plumbing (an
    // agent_client call to push the rule, plus expiry/retraction tracking in
    // worker.rs) is built; `note` carries that caveat to API/UI callers.
    pub enforced: bool,
    pub note: String,
}

pub async fn create_temporary_rule(
    pool: &DbPool,
    req: TemporaryRuleRequest,
) -> anyhow::Result<TemporaryRule> {
    let expires = Utc::now() + Duration::hours(req.duration_hours as i64);
    let id = Uuid::new_v4();
    // `applied` must be false: this INSERT only records intent in the
    // database. Nothing here (or anywhere else in the codebase) actually
    // pushes an allow rule to the host firewall, so claiming `applied = true`
    // was a false-success bug — see the `enforced`/`note` fields on
    // `TemporaryRule` for the honest, API-visible version of this caveat.
    // Leaving `applied = false` also makes `list_temporary_rules` (which
    // selects `WHERE applied = true`) correctly report zero "active" rules,
    // and makes `expire_temporary_rules` a correct no-op for these rows
    // (there is nothing enforced to expire).
    crate::db::query(
        "INSERT INTO firewall_temporary_rules
         (id, target_kind, target_id, source_cidr, dest_port, protocol, reason, owner, expires_at, applied)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, false)",
    )
    .bind(id)
    .bind(&req.target_kind)
    .bind(req.target_id)
    .bind(&req.source_cidr)
    .bind(req.dest_port)
    .bind(&req.protocol)
    .bind(&req.reason)
    .bind(&req.owner)
    .bind(expires)
    .execute(pool)
    .await?;

    let _ = crate::db::query(
        "INSERT INTO firewall_timeline (id, target_kind, target_id, kind, summary, detail_json, actor) VALUES (?, ?, ?, 'temporary_rule', ?, ?, ?)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(&req.target_kind)
    .bind(req.target_id)
    .bind(format!(
        "Temporary rule RECORDED (not enforced on host): {} {}:{} for {}h",
        req.protocol, req.source_cidr, req.dest_port, req.duration_hours
    ))
    .bind(serde_json::json!({
        "reason": req.reason,
        "expires_at": expires.to_rfc3339(),
        "enforced": false,
    }))
    .bind(req.owner.as_deref().unwrap_or("system"))
    .execute(pool)
    .await;

    Ok(TemporaryRule {
        id,
        source_cidr: req.source_cidr,
        dest_port: req.dest_port,
        protocol: req.protocol,
        reason: req.reason,
        expires_at: expires.to_rfc3339(),
        owner: req.owner,
        enforced: false,
        note: "Recorded for audit only — no host firewall change was made. This rule is NOT \
               live and will NOT auto-expire; enforcement plumbing is not yet implemented."
            .to_string(),
    })
}

pub async fn list_temporary_rules(
    pool: &DbPool,
    target_id: Uuid,
) -> anyhow::Result<Vec<TemporaryRule>> {
    let rows: Vec<(
        Uuid,
        String,
        i32,
        String,
        String,
        chrono::DateTime<Utc>,
        Option<String>,
    )> = crate::db::query_as(
        // `expires_at` is stored as an RFC3339 string (chrono's sqlite encoding, e.g.
        // "2026-07-24T18:00:00+00:00"), while `datetime('now')` yields SQLite's own
        // "YYYY-MM-DD HH:MM:SS" format. Comparing the two directly as TEXT is wrong:
        // the 'T' separator (0x54) sorts after the space (0x20), so any same-day
        // expiry would always compare as "not yet expired" regardless of the actual
        // time. Wrapping both sides in datetime() normalizes them before comparing.
        // NOTE: `applied = true` never happens post-honesty-fix (see
        // create_temporary_rule), so this currently always returns an empty
        // list — which is correct, since no temporary rule is actually
        // enforced on any host. Kept as-is so it starts reporting real
        // "active" rules for free once enforcement plumbing sets `applied`.
        "SELECT id, source_cidr, dest_port, protocol, reason, expires_at, owner
             FROM firewall_temporary_rules
             WHERE target_id = ? AND applied = true AND datetime(expires_at) > datetime('now')
             ORDER BY expires_at",
    )
    .bind(target_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(id, source_cidr, dest_port, protocol, reason, expires_at, owner)| TemporaryRule {
                id,
                source_cidr,
                dest_port,
                protocol,
                reason,
                expires_at: expires_at.to_rfc3339(),
                owner,
                // Even for a row where `applied = true` in the DB, nothing in
                // this codebase pushes the rule to a host firewall (see
                // create_temporary_rule), so this can never honestly claim
                // `enforced: true`.
                enforced: false,
                note: "Recorded for audit only — no host firewall change was made.".to_string(),
            },
        )
        .collect())
}

pub async fn expire_temporary_rules(pool: &DbPool) -> anyhow::Result<u64> {
    // Same format mismatch as list_temporary_rules: normalize expires_at through
    // datetime() so a same-day expiry is actually detected instead of the raw
    // 'T'-separated string always sorting "in the future" against datetime('now').
    //
    // Note: create_temporary_rule now inserts rows with `applied = false`
    // (honesty fix — nothing is ever actually pushed to the host firewall),
    // so this UPDATE currently matches zero rows and is effectively a no-op.
    // That is correct, not a regression: there is nothing enforced on a host
    // for this loop to "expire". Leaving the query in place (rather than
    // deleting it) means it starts doing real work again for free the day
    // real enforcement plumbing sets `applied = true` on rules it actually
    // pushed to a host.
    let rows = crate::db::query(
        "UPDATE firewall_temporary_rules SET applied = false
         WHERE applied = true AND datetime(expires_at) <= datetime('now')",
    )
    .execute(pool)
    .await?;
    Ok(rows.rows_affected())
}

pub async fn timeline(
    pool: &DbPool,
    target_kind: &str,
    target_id: Uuid,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let rows: Vec<(
        String,
        String,
        serde_json::Value,
        Option<String>,
        chrono::DateTime<Utc>,
    )> = crate::db::query_as(
        "SELECT kind, summary, detail_json, actor,
                strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
             FROM firewall_timeline
             WHERE target_kind = ? AND target_id = ?
             ORDER BY created_at DESC LIMIT 100",
    )
    .bind(target_kind)
    .bind(target_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(kind, summary, detail, actor, created_at)| {
            serde_json::json!({
                "kind": kind,
                "summary": summary,
                "detail": detail,
                "actor": actor,
                "created_at": created_at.to_rfc3339(),
            })
        })
        .collect())
}
