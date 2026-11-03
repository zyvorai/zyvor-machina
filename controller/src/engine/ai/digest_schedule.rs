// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Scheduled delivery of the Zyra fleet digest.
//!
//! One row in `ai_digest_schedule`, off by default. When it is due, the digest is built (no LLM, same as the
//! on-demand one) and handed to `webhooks::dispatch_webhooks` as event `zyra.digest`, so it reaches whatever
//! webhooks and notification channels already subscribe to `zyra.digest`, `zyra.*` or `*`. Only the leader
//! controller sends, so a cluster sends once. Nothing is sent for a slot that passed before it was switched on.

use chrono::{DateTime, Datelike, Duration, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use super::digest;
use crate::db::DbPool;

const STAMP: &str = "%Y-%m-%d %H:%M:%S";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Schedule {
    pub enabled: bool,
    /// `daily` | `weekly`
    pub frequency: String,
    /// 0..=23, UTC
    pub hour_utc: i64,
    /// ISO weekday, 1 = Monday .. 7 = Sunday (weekly only)
    pub weekday: i64,
    pub last_sent_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ScheduleUpdate {
    pub enabled: bool,
    pub frequency: String,
    pub hour_utc: i64,
    #[serde(default = "monday")]
    pub weekday: i64,
}

fn monday() -> i64 {
    1
}

/// The window a digest covers: the schedule period.
pub fn window_hours(frequency: &str) -> i64 {
    if frequency == "weekly" {
        168
    } else {
        24
    }
}

pub fn validate(u: &ScheduleUpdate) -> Result<(), String> {
    if u.frequency != "daily" && u.frequency != "weekly" {
        return Err("frequency must be daily or weekly".into());
    }
    if !(0..=23).contains(&u.hour_utc) {
        return Err("hour_utc must be 0 to 23".into());
    }
    if !(1..=7).contains(&u.weekday) {
        return Err("weekday must be 1 (Monday) to 7 (Sunday)".into());
    }
    Ok(())
}

/// The most recent scheduled instant at or before `now`.
pub fn latest_slot(s: &Schedule, now: DateTime<Utc>) -> DateTime<Utc> {
    let at_hour = |d: DateTime<Utc>| {
        Utc.with_ymd_and_hms(
            d.year(),
            d.month(),
            d.day(),
            s.hour_utc.clamp(0, 23) as u32,
            0,
            0,
        )
        .single()
        .unwrap_or(d)
    };
    if s.frequency == "weekly" {
        for back in 0..=7 {
            let cand = at_hour(now - Duration::days(back));
            if i64::from(cand.weekday().number_from_monday()) == s.weekday && cand <= now {
                return cand;
            }
        }
    }
    let today = at_hour(now);
    if today <= now {
        today
    } else {
        today - Duration::days(1)
    }
}

fn parse_stamp(s: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(s, STAMP)
        .ok()
        .map(|n| Utc.from_utc_datetime(&n))
}

/// Due when it is on and nothing was sent since the latest slot. Pure, so it is unit-tested.
pub fn is_due(s: &Schedule, now: DateTime<Utc>) -> bool {
    if !s.enabled {
        return false;
    }
    let slot = latest_slot(s, now);
    match s.last_sent_at.as_deref().and_then(parse_stamp) {
        Some(last) => last < slot,
        None => true,
    }
}

pub async fn get(pool: &DbPool) -> anyhow::Result<Schedule> {
    let row: Option<(bool, String, i64, i64, Option<String>)> = crate::db::query_as(
        "SELECT enabled, frequency, hour_utc, weekday, last_sent_at FROM ai_digest_schedule WHERE id = 1",
    )
    .fetch_optional(pool)
    .await?;
    let (enabled, frequency, hour_utc, weekday, last_sent_at) =
        row.unwrap_or((false, "daily".into(), 8, 1, None));
    Ok(Schedule {
        enabled,
        frequency,
        hour_utc,
        weekday,
        last_sent_at,
    })
}

/// Save the schedule. Switching it on counts as "sent now", so the first digest goes out at the next slot and
/// not immediately for a slot that already passed today.
pub async fn save(pool: &DbPool, u: &ScheduleUpdate, actor: &str) -> anyhow::Result<Schedule> {
    validate(u).map_err(anyhow::Error::msg)?;
    let current = get(pool).await?;
    let last = if u.enabled && !current.enabled {
        Some(Utc::now().format(STAMP).to_string())
    } else {
        current.last_sent_at
    };
    crate::db::query(
        "UPDATE ai_digest_schedule
         SET enabled = ?, frequency = ?, hour_utc = ?, weekday = ?, last_sent_at = ?, updated_by = ?,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = 1",
    )
    .bind(u.enabled)
    .bind(&u.frequency)
    .bind(u.hour_utc)
    .bind(u.weekday)
    .bind(last)
    .bind(actor)
    .execute(pool)
    .await?;
    get(pool).await
}

/// Build the digest for the schedule's period, hand it to the webhook and channel dispatcher, and record the send.
pub async fn deliver(pool: &DbPool) -> anyhow::Result<digest::Digest> {
    let s = get(pool).await?;
    let d = digest::build(pool, window_hours(&s.frequency)).await?;
    crate::engine::webhooks::dispatch_webhooks(
        pool,
        "zyra.digest",
        serde_json::json!({
            "message": d.markdown,
            "headline": d.headline,
            "hours": d.hours,
        }),
    )
    .await;
    crate::db::query("UPDATE ai_digest_schedule SET last_sent_at = ? WHERE id = 1")
        .bind(Utc::now().format(STAMP).to_string())
        .execute(pool)
        .await?;
    Ok(d)
}

/// Check every five minutes; only the leader controller sends.
pub fn spawn(state: crate::state::AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            match get(&state.pool).await {
                Ok(s) if is_due(&s, Utc::now()) => match deliver(&state.pool).await {
                    Ok(d) => tracing::info!(headline = %d.headline, "zyra fleet digest sent"),
                    Err(e) => tracing::warn!("zyra fleet digest: {e:#}"),
                },
                Ok(_) => {}
                Err(e) => tracing::warn!("zyra fleet digest schedule: {e:#}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, mi, 0).unwrap()
    }

    fn sched(freq: &str, hour: i64, weekday: i64, last: Option<&str>) -> Schedule {
        Schedule {
            enabled: true,
            frequency: freq.into(),
            hour_utc: hour,
            weekday,
            last_sent_at: last.map(str::to_string),
        }
    }

    #[test]
    fn daily_slot_is_today_once_the_hour_has_passed() {
        let s = sched("daily", 8, 1, None);
        assert_eq!(
            latest_slot(&s, at(2026, 11, 3, 9, 0)),
            at(2026, 11, 3, 8, 0)
        );
        assert_eq!(
            latest_slot(&s, at(2026, 11, 3, 7, 59)),
            at(2026, 11, 2, 8, 0)
        );
    }

    #[test]
    fn weekly_slot_is_the_latest_matching_weekday() {
        // 2026-11-03 is a Tuesday. Monday 08:00 was the day before.
        let s = sched("weekly", 8, 1, None);
        assert_eq!(
            latest_slot(&s, at(2026, 11, 3, 12, 0)),
            at(2026, 11, 2, 8, 0)
        );
        // On Monday before the hour, the slot is the previous Monday.
        assert_eq!(
            latest_slot(&s, at(2026, 11, 2, 7, 0)),
            at(2026, 10, 26, 8, 0)
        );
        // On Monday after the hour it is today.
        assert_eq!(
            latest_slot(&s, at(2026, 11, 2, 8, 0)),
            at(2026, 11, 2, 8, 0)
        );
    }

    #[test]
    fn due_only_when_enabled_and_not_yet_sent_for_the_slot() {
        let now = at(2026, 11, 3, 9, 0);
        assert!(is_due(&sched("daily", 8, 1, None), now));
        assert!(is_due(
            &sched("daily", 8, 1, Some("2026-11-02 08:00:10")),
            now
        ));
        assert!(!is_due(
            &sched("daily", 8, 1, Some("2026-11-03 08:00:10")),
            now
        ));
        let mut off = sched("daily", 8, 1, None);
        off.enabled = false;
        assert!(!is_due(&off, now));
    }

    #[test]
    fn a_send_after_the_slot_is_not_repeated() {
        let s = sched("daily", 8, 1, Some("2026-11-03 09:10:00"));
        assert!(!is_due(&s, at(2026, 11, 3, 23, 0)));
        assert!(is_due(&s, at(2026, 11, 4, 8, 5)));
    }

    #[test]
    fn an_unreadable_last_sent_counts_as_never_sent() {
        assert!(is_due(
            &sched("daily", 8, 1, Some("garbage")),
            at(2026, 11, 3, 9, 0)
        ));
    }

    #[test]
    fn validation_rejects_bad_values() {
        let ok = ScheduleUpdate {
            enabled: true,
            frequency: "daily".into(),
            hour_utc: 8,
            weekday: 1,
        };
        assert!(validate(&ok).is_ok());
        let bad = |f: &str, h: i64, w: i64| {
            validate(&ScheduleUpdate {
                enabled: true,
                frequency: f.into(),
                hour_utc: h,
                weekday: w,
            })
            .is_err()
        };
        assert!(bad("hourly", 8, 1));
        assert!(bad("daily", 24, 1));
        assert!(bad("daily", -1, 1));
        assert!(bad("weekly", 8, 0));
        assert!(bad("weekly", 8, 8));
    }

    #[test]
    fn window_follows_the_frequency() {
        assert_eq!(window_hours("daily"), 24);
        assert_eq!(window_hours("weekly"), 168);
    }

    #[tokio::test]
    async fn defaults_are_off() {
        let pool = crate::db::testing::pool().await;
        let s = get(&pool).await.unwrap();
        assert!(!s.enabled);
        assert_eq!(s.frequency, "daily");
        assert!(s.last_sent_at.is_none());
    }

    #[tokio::test]
    async fn enabling_marks_it_sent_so_it_does_not_fire_for_a_past_slot() {
        let pool = crate::db::testing::pool().await;
        let u = ScheduleUpdate {
            enabled: true,
            frequency: "daily".into(),
            hour_utc: 0,
            weekday: 1,
        };
        let s = save(&pool, &u, "tester").await.unwrap();
        assert!(s.enabled);
        assert!(s.last_sent_at.is_some());
        assert!(!is_due(&s, Utc::now()));
        // Changing the hour while already on keeps the last send.
        let again = save(&pool, &ScheduleUpdate { hour_utc: 5, ..u }, "tester")
            .await
            .unwrap();
        assert_eq!(again.last_sent_at, s.last_sent_at);
        assert_eq!(again.hour_utc, 5);
    }

    #[tokio::test]
    async fn save_refuses_invalid_input() {
        let pool = crate::db::testing::pool().await;
        let u = ScheduleUpdate {
            enabled: true,
            frequency: "yearly".into(),
            hour_utc: 8,
            weekday: 1,
        };
        assert!(save(&pool, &u, "tester").await.is_err());
    }

    #[tokio::test]
    async fn deliver_queues_the_digest_for_matching_channels_and_records_the_send() {
        let pool = crate::db::testing::pool().await;
        let chan = uuid::Uuid::new_v4();
        crate::db::query(
            "INSERT INTO notification_channels (id, name, kind, target, events, enabled)
             VALUES (?, 'test', 'webhook', 'http://127.0.0.1:9/x', '[\"zyra.*\"]', TRUE)",
        )
        .bind(chan)
        .execute(&pool)
        .await
        .unwrap();
        let d = deliver(&pool).await.unwrap();
        assert!(!d.markdown.is_empty());
        let n: i64 = crate::db::query_scalar(
            "SELECT COUNT(*) FROM channel_deliveries WHERE event_kind = 'zyra.digest'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(n, 1);
        assert!(get(&pool).await.unwrap().last_sent_at.is_some());
    }
}
