// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Fleet Digest: one briefing of what happened and what needs a person, for the last N hours.
//!
//! Counts come straight from the database; the wording is fixed, so the digest is identical with or
//! without an LLM provider. Read-only. (Scheduled delivery is not built: the digest is fetched on
//! demand and can be polled by whatever sends your notifications.)

use serde::Serialize;

use crate::db::DbPool;

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Digest {
    pub hours: i64,
    pub hosts_total: i64,
    pub hosts_not_online: i64,
    pub vms_total: i64,
    pub vms_running: i64,
    pub open_incidents: i64,
    pub pending_approvals: i64,
    pub actions_executed: i64,
    pub actions_failed: i64,
    pub actions_rejected: i64,
    pub actions_undone: i64,
    pub budget_status: Option<String>,
    pub budget_alerts: Vec<String>,
    /// `ok` | `attention` | `critical`
    pub headline: String,
    pub markdown: String,
}

fn sum_where(rows: &[(String, i64)], pred: impl Fn(&str) -> bool) -> i64 {
    rows.iter().filter(|(k, _)| pred(k)).map(|(_, n)| *n).sum()
}

/// Headline and Markdown from the counted facts. Pure, so it is unit-tested.
pub fn compose(d: &mut Digest) {
    d.headline = if d.hosts_not_online > 0 && d.hosts_not_online * 2 >= d.hosts_total.max(1) {
        "critical"
    } else if d.hosts_not_online > 0
        || d.open_incidents > 0
        || d.pending_approvals > 0
        || d.actions_failed > 0
        || !d.budget_alerts.is_empty()
    {
        "attention"
    } else {
        "ok"
    }
    .to_string();

    let mut md = format!("# Fleet digest — last {} h\n\n", d.hours);
    md.push_str(&match d.headline.as_str() {
        "ok" => "**All quiet.** Nothing needs a person right now.\n\n".to_string(),
        "critical" => {
            "**Needs attention now.** Half or more of your hosts are not online.\n\n".to_string()
        }
        _ => "**A few things need a look.**\n\n".to_string(),
    });
    md.push_str("## Fleet\n\n");
    md.push_str(&format!(
        "- Hosts: {} total, {} not online\n- Machines: {} total, {} running\n",
        d.hosts_total, d.hosts_not_online, d.vms_total, d.vms_running
    ));
    md.push_str("\n## Needs a person\n\n");
    md.push_str(&format!(
        "- Open incidents: {}\n- Pending approvals: {}\n",
        d.open_incidents, d.pending_approvals
    ));
    md.push_str("\n## What Zyra did\n\n");
    md.push_str(&format!(
        "- Executed: {}\n- Failed: {}\n- Rejected by a person: {}\n- Undone: {}\n",
        d.actions_executed, d.actions_failed, d.actions_rejected, d.actions_undone
    ));
    if let Some(st) = &d.budget_status {
        md.push_str(&format!("\n## Cost\n\n- Budget status: {st}\n"));
        for a in &d.budget_alerts {
            md.push_str(&format!("- {}\n", a.replace('\n', " ")));
        }
    }
    d.markdown = md;
}

pub async fn build(pool: &DbPool, hours: i64) -> anyhow::Result<Digest> {
    let hours = hours.clamp(1, 24 * 30);
    let since = (chrono::Utc::now() - chrono::Duration::hours(hours))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    let hosts: Vec<(String, i64)> =
        crate::db::query_as("SELECT state, COUNT(*) FROM hosts GROUP BY state")
            .fetch_all(pool)
            .await?;
    let vms: Vec<(String, i64)> =
        crate::db::query_as("SELECT observed_state, COUNT(*) FROM vms GROUP BY observed_state")
            .fetch_all(pool)
            .await?;
    let acts: Vec<(String, i64)> = crate::db::query_as(
        "SELECT status, COUNT(*) FROM ai_actions
         WHERE strftime('%Y-%m-%dT%H:%M:%SZ', created_at) >= ? GROUP BY status",
    )
    .bind(&since)
    .fetch_all(pool)
    .await?;
    let undone: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM ai_actions
         WHERE undone_at IS NOT NULL AND strftime('%Y-%m-%dT%H:%M:%SZ', undone_at) >= ?",
    )
    .bind(&since)
    .fetch_one(pool)
    .await?;
    let open_incidents: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM ai_incidents WHERE status IN ('open', 'investigating')",
    )
    .fetch_one(pool)
    .await?;
    let pending: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM ai_actions WHERE status = 'pending'")
            .fetch_one(pool)
            .await?;

    // Cost is a nice-to-have: a failure there must not block the rest of the digest.
    let budget = super::cost_budget::analyze(pool).await.ok();

    let mut d = Digest {
        hours,
        hosts_total: sum_where(&hosts, |_| true),
        hosts_not_online: sum_where(&hosts, |s| s != "online"),
        vms_total: sum_where(&vms, |_| true),
        vms_running: sum_where(&vms, |s| s == "running"),
        open_incidents,
        pending_approvals: pending,
        actions_executed: sum_where(&acts, |s| s == "executed"),
        actions_failed: sum_where(&acts, |s| s == "failed"),
        actions_rejected: sum_where(&acts, |s| s == "rejected"),
        actions_undone: undone,
        budget_status: budget.as_ref().map(|b| b.status.clone()),
        budget_alerts: budget
            .map(|b| b.alerts.into_iter().map(|a| a.message).collect())
            .unwrap_or_default(),
        ..Default::default()
    };
    compose(&mut d);
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Digest {
        Digest {
            hours: 24,
            hosts_total: 4,
            vms_total: 20,
            vms_running: 18,
            ..Default::default()
        }
    }

    #[test]
    fn quiet_fleet_is_ok() {
        let mut d = base();
        compose(&mut d);
        assert_eq!(d.headline, "ok");
        assert!(d.markdown.contains("All quiet"));
    }

    #[test]
    fn pending_work_asks_for_attention() {
        let mut d = base();
        d.pending_approvals = 2;
        compose(&mut d);
        assert_eq!(d.headline, "attention");
        assert!(d.markdown.contains("Pending approvals: 2"));
    }

    #[test]
    fn half_the_hosts_down_is_critical() {
        let mut d = base();
        d.hosts_not_online = 2;
        compose(&mut d);
        assert_eq!(d.headline, "critical");
    }

    #[test]
    fn a_budget_alert_shows_in_the_cost_section() {
        let mut d = base();
        d.budget_status = Some("warning".into());
        d.budget_alerts = vec!["Spend is 92% of budget".into()];
        compose(&mut d);
        assert_eq!(d.headline, "attention");
        assert!(d.markdown.contains("Spend is 92% of budget"));
    }

    #[test]
    fn sums_filter_by_state() {
        let rows = vec![("running".to_string(), 3), ("shutoff".to_string(), 2)];
        assert_eq!(sum_where(&rows, |s| s == "running"), 3);
        assert_eq!(sum_where(&rows, |_| true), 5);
    }

    #[tokio::test]
    async fn build_runs_against_an_empty_schema() {
        let pool = crate::db::testing::pool().await;
        let d = build(&pool, 24).await.unwrap();
        assert_eq!((d.hosts_total, d.vms_total, d.open_incidents), (0, 0, 0));
        assert!(d.markdown.starts_with("# Fleet digest"));
    }
}
