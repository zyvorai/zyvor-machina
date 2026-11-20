// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Load balancer health checks. Every 5 s the leader looks for load balancers whose check is due, has the owning host's agent
//! probe each member (`lb.probe`), moves each member's health through `step`, and re-applies the rule set when a member
//! changed state. A member is left out of rotation only while `unhealthy`; `unknown` (never probed) stays in.

use std::time::Duration;

use serde_json::{json, Value};
use crate::db::DbPool;
use uuid::Uuid;

use crate::state::AppState;

/// One probe result applied to a member's counters: (new state, ok streak, fail streak).
pub fn step(state: &str, ok: i64, fail: i64, probe_ok: bool, healthy_threshold: i64, unhealthy_threshold: i64) -> (String, i64, i64) {
    if probe_ok {
        let ok = ok + 1;
        let state = if ok >= healthy_threshold.max(1) { "healthy" } else { state };
        (state.to_string(), ok, 0)
    } else {
        let fail = fail + 1;
        let state = if fail >= unhealthy_threshold.max(1) { "unhealthy" } else { state };
        (state.to_string(), 0, fail)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Check {
    pub protocol: String,
    pub port: Option<i64>,
    pub path: String,
    pub interval_secs: i64,
    pub timeout_secs: i64,
    pub healthy_threshold: i64,
    pub unhealthy_threshold: i64,
}

pub fn validate(c: &Check) -> Result<(), String> {
    if !matches!(c.protocol.as_str(), "none" | "tcp" | "http") {
        return Err("protocol must be none, tcp or http".into());
    }
    if c.protocol == "none" {
        return Ok(());
    }
    if let Some(p) = c.port {
        if !(1..=65535).contains(&p) {
            return Err("port must be 1-65535".into());
        }
    }
    if c.protocol == "http" && (!c.path.starts_with('/') || c.path.len() > 256 || !c.path.bytes().all(|b| b.is_ascii_graphic())) {
        return Err("path must start with / (printable characters, at most 256)".into());
    }
    if !(5..=300).contains(&c.interval_secs) {
        return Err("interval_secs must be 5-300".into());
    }
    if !(1..=10).contains(&c.timeout_secs) || c.timeout_secs >= c.interval_secs {
        return Err("timeout_secs must be 1-10 and shorter than the interval".into());
    }
    if !(1..=10).contains(&c.healthy_threshold) || !(1..=10).contains(&c.unhealthy_threshold) {
        return Err("thresholds must be 1-10".into());
    }
    Ok(())
}

type LbRow = (Uuid, Uuid, String, Option<i64>, String, i64, i64, i64, i64, i64);

async fn due(pool: &DbPool) -> anyhow::Result<Vec<LbRow>> {
    Ok(crate::db::query_as(
        "SELECT id, host_id, hc_protocol, hc_port, hc_path, hc_interval_secs, hc_timeout_secs, hc_healthy_threshold, hc_unhealthy_threshold, listener_port \
         FROM load_balancers WHERE hc_protocol <> 'none' \
         AND (hc_last_run IS NULL OR CAST(strftime('%s','now') AS INTEGER) - CAST(strftime('%s', hc_last_run) AS INTEGER) >= hc_interval_secs)",
    )
    .fetch_all(pool)
    .await?)
}

async fn run_one(state: &AppState, lb: LbRow) -> anyhow::Result<()> {
    let (id, host, protocol, port, path, _interval, timeout, healthy_t, unhealthy_t, _listener) = lb;
    crate::db::query("UPDATE load_balancers SET hc_last_run = CURRENT_TIMESTAMP WHERE id = ?").bind(id).execute(&state.pool).await?;
    type M = (Uuid, String, i64, String, i64, i64);
    let members: Vec<M> = crate::db::query_as(
        "SELECT m.id, v.guest_ip, m.port, m.health, m.health_ok, m.health_fail FROM lb_members m JOIN vms v ON v.id = m.vm_id \
         WHERE m.load_balancer_id = ? AND m.enabled = TRUE AND COALESCE(v.guest_ip, '') <> ''",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    if members.is_empty() {
        return Ok(());
    }
    let addr: String = crate::db::query_scalar("SELECT agent_grpc_addr FROM hosts WHERE id = ? AND state = 'online'").bind(host).fetch_one(&state.pool).await?;
    let targets: Vec<Value> = members
        .iter()
        .map(|(mid, ip, mport, ..)| {
            json!({ "id": mid.to_string(), "ip": ip, "port": port.unwrap_or(*mport), "kind": protocol, "path": path, "timeout_ms": timeout * 1000 })
        })
        .collect();
    let mut client = crate::agent_client::connect(&addr).await?;
    let res = crate::agent_client::host_libvirt_invoke(&mut client, "lb.probe", &json!({ "targets": targets })).await?;
    let mut changed = false;
    for r in res["results"].as_array().cloned().unwrap_or_default() {
        let Some(mid) = r["id"].as_str().and_then(|s| Uuid::parse_str(s).ok()) else { continue };
        let Some((_, _, _, prev, ok, fail)) = members.iter().find(|m| m.0 == mid) else { continue };
        let (state_now, ok2, fail2) = step(prev, *ok, *fail, r["ok"].as_bool().unwrap_or(false), healthy_t, unhealthy_t);
        if &state_now != prev {
            changed = true;
            state.emit_event("lb.health", format!("Load balancer member {mid}: {prev} -> {state_now} ({})", r["detail"].as_str().unwrap_or("")));
        }
        record(&state.pool, mid, &state_now, ok2, fail2, r["detail"].as_str().unwrap_or("")).await?;
    }
    if changed {
        crate::engine::load_balancer::apply(&state.pool, &state.config, id).await?;
    }
    Ok(())
}

/// Save one member's probe outcome; `health_changed_at` moves only when the state does.
async fn record(pool: &DbPool, member: Uuid, state_now: &str, ok: i64, fail: i64, detail: &str) -> Result<(), sqlx::Error> {
    crate::db::query(
        "UPDATE lb_members SET health = ?, health_ok = ?, health_fail = ?, health_detail = ?, \
         health_changed_at = CASE WHEN health <> ? THEN CURRENT_TIMESTAMP ELSE health_changed_at END WHERE id = ?",
    )
    .bind(state_now)
    .bind(ok)
    .bind(fail)
    .bind(detail)
    .bind(state_now)
    .bind(member)
    .execute(pool)
    .await?;
    Ok(())
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            for lb in due(&state.pool).await.unwrap_or_default() {
                if let Err(e) = run_one(&state, lb).await {
                    tracing::warn!("lb health check: {e:#}");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The statement the checker runs against the real schema: 058 once missed a column and every result failed to save,
    /// invisibly, because the error was only logged at debug level.
    #[tokio::test]
    async fn a_probe_result_can_be_saved_against_the_migrated_schema() {
        let pool = crate::db::testing::pool().await;
        record(&pool, Uuid::new_v4(), "healthy", 2, 0, "ok").await.unwrap();
    }

    #[test]
    fn a_member_turns_unhealthy_only_after_the_failure_threshold() {
        let (s, ok, fail) = step("healthy", 5, 0, false, 2, 3);
        assert_eq!((s.as_str(), ok, fail), ("healthy", 0, 1));
        let (s, _, fail) = step(&s, ok, fail, false, 2, 3);
        assert_eq!((s.as_str(), fail), ("healthy", 2));
        let (s, _, fail) = step(&s, 0, fail, false, 2, 3);
        assert_eq!((s.as_str(), fail), ("unhealthy", 3));
    }

    #[test]
    fn it_recovers_after_the_healthy_threshold_and_a_failure_resets_the_streak() {
        let (s, ok, _) = step("unhealthy", 0, 7, true, 2, 3);
        assert_eq!((s.as_str(), ok), ("unhealthy", 1));
        let (s, ok, fail) = step(&s, ok, 0, false, 2, 3);
        assert_eq!((s.as_str(), ok, fail), ("unhealthy", 0, 1), "a failure restarts the recovery");
        let (s, ok, _) = step(&s, 0, 0, true, 2, 3);
        let (s, _, _) = step(&s, ok, 0, true, 2, 3);
        assert_eq!(s, "healthy");
    }

    #[test]
    fn unknown_stays_in_rotation_until_a_threshold_is_crossed() {
        let (s, _, _) = step("unknown", 0, 0, false, 2, 3);
        assert_eq!(s, "unknown");
        let (s, _, _) = step("unknown", 1, 0, true, 2, 3);
        assert_eq!(s, "healthy");
    }

    fn c() -> Check {
        Check { protocol: "http".into(), port: Some(8080), path: "/health".into(), interval_secs: 10, timeout_secs: 3, healthy_threshold: 2, unhealthy_threshold: 3 }
    }

    #[test]
    fn check_validation() {
        assert!(validate(&c()).is_ok());
        assert!(validate(&Check { protocol: "none".into(), ..c() }).is_ok());
        assert!(validate(&Check { protocol: "udp".into(), ..c() }).is_err());
        assert!(validate(&Check { path: "health".into(), ..c() }).is_err());
        assert!(validate(&Check { interval_secs: 2, ..c() }).is_err());
        assert!(validate(&Check { timeout_secs: 10, ..c() }).is_err(), "timeout must be shorter than the interval");
        assert!(validate(&Check { port: Some(0), ..c() }).is_err());
        assert!(validate(&Check { healthy_threshold: 0, ..c() }).is_err());
        assert!(validate(&Check { protocol: "tcp".into(), path: "".into(), ..c() }).is_ok(), "tcp ignores the path");
    }
}
