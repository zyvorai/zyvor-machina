// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Alarms over `metric_samples` (OK / ALARM / INSUFFICIENT_DATA) with a step-scaling action on an instance group.
//! The evaluator runs on the leader only and stops writing as soon as its leadership epoch moves.

use std::time::Duration;

use machina_spec::ScalingPolicy;
use uuid::Uuid;

use crate::api::metric_stats::{aggregate, Datapoint};
use crate::state::AppState;

pub const STATES: [&str; 3] = ["OK", "ALARM", "INSUFFICIENT_DATA"];
const TICK_SECS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    Gt,
    Gte,
    Lt,
    Lte,
}

impl Cmp {
    pub fn parse(s: &str) -> Option<Cmp> {
        Some(match s {
            "gt" => Cmp::Gt,
            "gte" => Cmp::Gte,
            "lt" => Cmp::Lt,
            "lte" => Cmp::Lte,
            _ => return None,
        })
    }
    fn breaches(self, v: f64, threshold: f64) -> bool {
        match self {
            Cmp::Gt => v > threshold,
            Cmp::Gte => v >= threshold,
            Cmp::Lt => v < threshold,
            Cmp::Lte => v <= threshold,
        }
    }
}

fn pick(d: &Datapoint, statistic: &str) -> Option<f64> {
    match statistic {
        "Average" => d.average,
        "Minimum" => d.minimum,
        "Maximum" => d.maximum,
        "Sum" => d.sum,
        "SampleCount" => d.sample_count.map(|c| c as f64),
        _ => None,
    }
}

/// State from the buckets of the evaluation window: the newest `periods` buckets must all exist, and all breach for ALARM.
pub fn evaluate(points: &[Datapoint], statistic: &str, periods: usize, cmp: Cmp, threshold: f64) -> (&'static str, String) {
    if periods == 0 || points.len() < periods {
        return ("INSUFFICIENT_DATA", format!("{} of {periods} periods have data", points.len()));
    }
    let recent = &points[points.len() - periods..];
    let vals: Vec<f64> = recent.iter().filter_map(|d| pick(d, statistic)).collect();
    if vals.len() < periods {
        return ("INSUFFICIENT_DATA", "statistic not available".into());
    }
    let shown = vals.iter().map(|v| format!("{v:.2}")).collect::<Vec<_>>().join(", ");
    if vals.iter().all(|v| cmp.breaches(*v, threshold)) {
        ("ALARM", format!("{statistic} [{shown}] breached {threshold} in {periods} period(s)"))
    } else {
        ("OK", format!("{statistic} [{shown}] within {threshold}"))
    }
}

/// Fire on entering ALARM, and again while it stays in ALARM once the cooldown has passed.
pub fn should_fire(prev: &str, now: &str, secs_since_last_action: Option<i64>, cooldown: i64) -> bool {
    if now != "ALARM" {
        return false;
    }
    if prev != "ALARM" {
        return secs_since_last_action.is_none_or(|s| s >= cooldown);
    }
    secs_since_last_action.is_none_or(|s| s >= cooldown)
}

/// The group's new desired size after a step, clamped to its limits; None when nothing would change.
pub fn next_desired(policy: &ScalingPolicy, step: i32) -> Option<u32> {
    let want = (i64::from(policy.desired) + i64::from(step)).clamp(i64::from(policy.min), i64::from(policy.max)) as u32;
    (want != policy.desired).then_some(want)
}

type AlarmRow = (Uuid, String, String, String, String, i64, i64, String, f64, String, String, Option<Uuid>, i64, i64, Option<i64>);

async fn evaluate_one(state: &AppState, a: AlarmRow, now: i64, epoch: i64) -> anyhow::Result<()> {
    let (id, name, subject, metric, statistic, period, periods, comparator, threshold, prev, action, group, step, cooldown, since) = a;
    let start = now - period * periods;
    let keys = crate::api::metric_stats::subject_keys(&state.pool, &subject).await;
    let samples = crate::api::metric_stats::fetch_samples(&state.pool, &keys, &metric, start, now).await?;
    let cmp = Cmp::parse(&comparator).unwrap_or(Cmp::Gt);
    let points = aggregate(&samples, period, &[statistic.as_str()]);
    let (new_state, reason) = evaluate(&points, &statistic, periods as usize, cmp, threshold);
    if !crate::engine::cloud::fence_ok(state, Some(epoch)) {
        return Ok(());
    }
    if new_state != prev {
        crate::db::query("UPDATE cloud_alarms SET state = ?, state_reason = ?, state_updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(new_state)
            .bind(&reason)
            .bind(id)
            .execute(&state.pool)
            .await?;
        state.emit_event("alarm.state", format!("Alarm {name}: {prev} -> {new_state} ({reason})"));
    }
    if action == "scale_group" && should_fire(&prev, new_state, since, cooldown) {
        if let Some(g) = group {
            scale_group(state, id, &name, g, step as i32).await?;
        }
    }
    Ok(())
}

async fn scale_group(state: &AppState, alarm: Uuid, name: &str, group: Uuid, step: i32) -> anyhow::Result<()> {
    let raw: Option<String> = crate::db::query_scalar("SELECT policy_json FROM cloud_instance_groups WHERE id = ? AND paused = FALSE")
        .bind(group)
        .fetch_optional(&state.pool)
        .await?;
    let Some(raw) = raw else { return Ok(()) };
    let mut policy: ScalingPolicy = serde_json::from_str(&raw)?;
    let Some(want) = next_desired(&policy, step) else {
        return Ok(());
    };
    let before = policy.desired;
    policy.desired = want;
    // Same compare-and-swap the group reconciler uses: a concurrent policy edit wins and we try again next tick.
    let changed = crate::db::query("UPDATE cloud_instance_groups SET policy_json = ?, last_scaled_at = CURRENT_TIMESTAMP WHERE id = ? AND paused = FALSE AND policy_json = ?")
        .bind(serde_json::to_string(&policy)?)
        .bind(group)
        .bind(&raw)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if changed == 1 {
        crate::db::query("UPDATE cloud_alarms SET last_action_at = CURRENT_TIMESTAMP WHERE id = ?").bind(alarm).execute(&state.pool).await?;
        state.emit_event("alarm.action", format!("Alarm {name} scaled a group from {before} to {want}"));
    }
    Ok(())
}

pub async fn tick(state: &AppState, epoch: i64) -> anyhow::Result<usize> {
    let rows: Vec<AlarmRow> = crate::db::query_as(
        "SELECT id, name, subject, metric, statistic, period_secs, evaluation_periods, comparator, threshold, state, action, group_id, step, cooldown_secs, \
         CASE WHEN last_action_at IS NULL THEN NULL ELSE CAST(strftime('%s','now') AS INTEGER) - CAST(strftime('%s', last_action_at) AS INTEGER) END \
         FROM cloud_alarms WHERE enabled = TRUE",
    )
    .fetch_all(&state.pool)
    .await?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() as i64;
    let n = rows.len();
    for r in rows {
        if let Err(e) = evaluate_one(state, r, now, epoch).await {
            tracing::warn!("alarm evaluation: {e:#}");
        }
    }
    Ok(n)
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(TICK_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = tick(&state, crate::leader::current_epoch()).await {
                tracing::warn!("alarms: {e:#}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dp(avg: f64) -> Datapoint {
        aggregate(&[(0, avg)], 60, &["Average"]).remove(0)
    }

    #[test]
    fn needs_enough_periods_and_all_must_breach() {
        let pts = [dp(10.0), dp(95.0), dp(97.0)];
        assert_eq!(evaluate(&pts, "Average", 2, Cmp::Gt, 90.0).0, "ALARM");
        assert_eq!(evaluate(&pts, "Average", 3, Cmp::Gt, 90.0).0, "OK");
        assert_eq!(evaluate(&pts, "Average", 4, Cmp::Gt, 90.0).0, "INSUFFICIENT_DATA");
        assert_eq!(evaluate(&[], "Average", 1, Cmp::Gt, 1.0).0, "INSUFFICIENT_DATA");
    }

    #[test]
    fn comparators() {
        let p = [dp(50.0)];
        assert_eq!(evaluate(&p, "Average", 1, Cmp::Gte, 50.0).0, "ALARM");
        assert_eq!(evaluate(&p, "Average", 1, Cmp::Gt, 50.0).0, "OK");
        assert_eq!(evaluate(&p, "Average", 1, Cmp::Lt, 60.0).0, "ALARM");
        assert_eq!(evaluate(&p, "Average", 1, Cmp::Lte, 40.0).0, "OK");
        assert!(Cmp::parse("eq").is_none());
    }

    #[test]
    fn a_missing_statistic_is_insufficient_data() {
        assert_eq!(evaluate(&[dp(1.0)], "Maximum", 1, Cmp::Gt, 0.0).0, "INSUFFICIENT_DATA");
    }

    #[test]
    fn firing_respects_the_cooldown() {
        assert!(should_fire("OK", "ALARM", None, 300));
        assert!(!should_fire("OK", "ALARM", Some(100), 300));
        assert!(should_fire("ALARM", "ALARM", Some(301), 300));
        assert!(!should_fire("ALARM", "ALARM", Some(10), 300));
        assert!(!should_fire("ALARM", "OK", None, 0));
    }

    #[test]
    fn steps_are_clamped_to_the_group_limits() {
        let p = ScalingPolicy { min: 1, max: 4, desired: 3, cooldown_secs: 60, ..Default::default() };
        assert_eq!(next_desired(&p, 1), Some(4));
        assert_eq!(next_desired(&p, 5), Some(4));
        assert_eq!(next_desired(&p, -9), Some(1));
        let at_max = ScalingPolicy { desired: 4, ..p.clone() };
        assert_eq!(next_desired(&at_max, 1), None);
        assert_eq!(next_desired(&p, 0), None);
    }
}
