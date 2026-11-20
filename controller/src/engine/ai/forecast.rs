// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Real forecasting. A leader-only recorder samples VM memory/CPU and storage-pool fill every five minutes into
//! `metric_samples`; `time_to_threshold` fits a straight line to the recent samples and says when the metric
//! crosses a limit, with a confidence derived from how well the line fits and how much history there is.
//! Too little history means "no forecast", never a made-up number.

use std::time::Duration;

use std::collections::BTreeMap;

use serde::Serialize;
use crate::db::DbPool;

use crate::state::AppState;

const SAMPLE_EVERY: Duration = Duration::from_secs(300);
const KEEP_SECS: i64 = 14 * 24 * 3600;
/// Fewest samples, and shortest time span, before a trend is worth showing.
pub const MIN_SAMPLES: usize = 12;
pub const MIN_SPAN_HOURS: f64 = 1.0;
/// Forecasts further out than this are not actionable.
pub const MAX_HORIZON_HOURS: f64 = 24.0 * 30.0;

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(SAMPLE_EVERY).await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = record_samples(&state.pool).await {
                tracing::warn!("forecast sample recording failed: {e:#}");
            }
        }
    });
}

pub async fn record_samples(pool: &DbPool) -> anyhow::Result<()> {
    let now = chrono::Utc::now().timestamp();
    crate::db::query(
        "INSERT OR IGNORE INTO metric_samples (subject, metric, ts, value)
         SELECT v.id, 'mem_ratio', ?, m.memory_used_mib * 1.0 / v.memory_mib
         FROM vms v JOIN vm_metrics m ON m.vm_id = v.id
         WHERE v.observed_state = 'running' AND v.memory_mib > 0",
    )
    .bind(now)
    .execute(pool)
    .await?;
    crate::db::query(
        "INSERT OR IGNORE INTO metric_samples (subject, metric, ts, value)
         SELECT v.id, 'cpu_percent', ?, m.cpu_percent
         FROM vms v JOIN vm_metrics m ON m.vm_id = v.id
         WHERE v.observed_state = 'running'",
    )
    .bind(now)
    .execute(pool)
    .await?;
    crate::db::query(
        "INSERT OR IGNORE INTO metric_samples (subject, metric, ts, value)
         SELECT 'pool:' || id, 'pool_used_ratio', ?, used_gib * 1.0 / capacity_gib
         FROM storage_pools WHERE capacity_gib > 0",
    )
    .bind(now)
    .execute(pool)
    .await?;
    crate::db::query(
        "INSERT OR IGNORE INTO metric_samples (subject, metric, ts, value)
         SELECT v.id, 'disk_iops', ?, m.disk_read_iops + m.disk_write_iops
         FROM vms v JOIN vm_metrics m ON m.vm_id = v.id
         WHERE v.observed_state = 'running'",
    )
    .bind(now)
    .execute(pool)
    .await?;
    crate::db::query(
        "INSERT OR IGNORE INTO metric_samples (subject, metric, ts, value)
         SELECT v.id, 'net_bytes', ?, m.net_bytes
         FROM vms v JOIN vm_metrics m ON m.vm_id = v.id
         WHERE v.observed_state = 'running'",
    )
    .bind(now)
    .execute(pool)
    .await?;
    // A group's demand: CPU percent summed over its running members, in
    // "instances' worth" once divided by the target.
    crate::db::query(
        "INSERT OR IGNORE INTO metric_samples (subject, metric, ts, value)
         SELECT 'group:' || lower(hex(gm.group_id)), 'cpu_sum', ?, SUM(m.cpu_percent)
         FROM cloud_group_members gm
         JOIN vms v ON v.id = gm.vm_id
         JOIN vm_metrics m ON m.vm_id = v.id
         WHERE v.observed_state = 'running' AND m.updated_at > datetime('now', '-5 minutes')
         GROUP BY gm.group_id",
    )
    .bind(now)
    .execute(pool)
    .await?;
    crate::db::query("DELETE FROM metric_samples WHERE ts < ?")
        .bind(now - KEEP_SECS)
        .execute(pool)
        .await?;
    rollup_hourly(pool, now).await?;
    Ok(())
}

/// Hourly history is kept this long, enough for four weekly periods.
const KEEP_HOURLY_SECS: i64 = 35 * 24 * 3600;
const HOUR: i64 = 3600;
const DAY: i64 = 24 * HOUR;
const WEEK: i64 = 7 * DAY;

/// The subject a group's demand is recorded under.
pub fn group_subject(id: uuid::Uuid) -> String {
    format!("group:{}", id.simple())
}

/// Rolls the last two days of raw samples up into `metric_hourly` (complete
/// hours only), turning the cumulative `net_bytes` counter into `net_bps`.
pub async fn rollup_hourly(pool: &DbPool, now: i64) -> anyhow::Result<()> {
    let end = now / HOUR * HOUR;
    let start = end - 2 * DAY;
    crate::db::query(
        "INSERT INTO metric_hourly (subject, metric, hour, avg, max, n)
         SELECT subject, metric, ts / 3600 * 3600 AS h, AVG(value), MAX(value), COUNT(*)
         FROM metric_samples
         WHERE ts >= ? AND ts < ? AND metric <> 'net_bytes'
         GROUP BY subject, metric, h
         ON CONFLICT(subject, metric, hour) DO UPDATE SET avg = excluded.avg, max = excluded.max, n = excluded.n",
    )
    .bind(start)
    .bind(end)
    .execute(pool)
    .await?;
    crate::db::query(
        "INSERT INTO metric_hourly (subject, metric, hour, avg, max, n)
         SELECT subject, 'net_bps', ts / 3600 * 3600 AS h,
                max(0.0, (MAX(value) - MIN(value)) * 1.0 / max(1, MAX(ts) - MIN(ts))),
                max(0.0, (MAX(value) - MIN(value)) * 1.0 / max(1, MAX(ts) - MIN(ts))),
                COUNT(*)
         FROM metric_samples
         WHERE ts >= ? AND ts < ? AND metric = 'net_bytes'
         GROUP BY subject, h
         HAVING COUNT(*) >= 2
         ON CONFLICT(subject, metric, hour) DO UPDATE SET avg = excluded.avg, max = excluded.max, n = excluded.n",
    )
    .bind(start)
    .bind(end)
    .execute(pool)
    .await?;
    crate::db::query("DELETE FROM metric_hourly WHERE hour < ?")
        .bind(now - KEEP_HOURLY_SECS)
        .execute(pool)
        .await?;
    Ok(())
}

/// Hourly history for one subject+metric: hour start -> average (or peak).
pub async fn hourly(
    pool: &DbPool,
    subject: &str,
    metric: &str,
    days: i64,
    peak: bool,
) -> anyhow::Result<BTreeMap<i64, f64>> {
    let since = chrono::Utc::now().timestamp() - days * DAY;
    let rows: Vec<(i64, f64, f64)> = crate::db::query_as(
        "SELECT hour, avg, max FROM metric_hourly WHERE subject = ? AND metric = ? AND hour >= ?",
    )
    .bind(subject)
    .bind(metric)
    .bind(since)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(h, a, m)| (h, if peak { m } else { a }))
        .collect())
}

/// [`hourly`] for a VM, whose samples are keyed by its id as stored in `vms`.
pub async fn hourly_vm(
    pool: &DbPool,
    vm: uuid::Uuid,
    metric: &str,
    days: i64,
    peak: bool,
) -> anyhow::Result<BTreeMap<i64, f64>> {
    let since = chrono::Utc::now().timestamp() - days * DAY;
    let rows: Vec<(i64, f64, f64)> = crate::db::query_as(
        "SELECT hour, avg, max FROM metric_hourly WHERE subject = ? AND metric = ? AND hour >= ?",
    )
    .bind(crate::db::subject_id(vm))
    .bind(metric)
    .bind(since)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(h, a, m)| (h, if peak { m } else { a }))
        .collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Seasonal {
    pub value: f64,
    /// "weekly": the same hour in earlier weeks; "daily": the same hour on earlier days.
    pub basis: &'static str,
    /// How many earlier periods the value is the mean of.
    pub periods: usize,
}

/// The value expected in the hour starting at `at`: the mean of the same hour
/// of the week over the last four weeks when at least two are known, else the
/// same hour of the day over the last week when at least three are known.
pub fn seasonal_at(hourly: &BTreeMap<i64, f64>, at: i64) -> Option<Seasonal> {
    let at = at / HOUR * HOUR;
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let weekly: Vec<f64> = (1..=4)
        .filter_map(|k| hourly.get(&(at - k * WEEK)).copied())
        .collect();
    if weekly.len() >= 2 {
        return Some(Seasonal {
            value: mean(&weekly),
            basis: "weekly",
            periods: weekly.len(),
        });
    }
    let daily: Vec<f64> = (1..=7)
        .filter_map(|k| hourly.get(&(at - k * DAY)).copied())
        .collect();
    (daily.len() >= 3).then(|| Seasonal {
        value: mean(&daily),
        basis: "daily",
        periods: daily.len(),
    })
}

/// The highest expected value over the hour containing `now` and the next
/// `hours` hours, with the hour it falls in.
pub fn seasonal_peak(hourly: &BTreeMap<i64, f64>, now: i64, hours: i64) -> Option<(i64, Seasonal)> {
    (0..=hours)
        .filter_map(|k| {
            let h = now / HOUR * HOUR + k * HOUR;
            seasonal_at(hourly, h).map(|s| (h, s))
        })
        .max_by(|a, b| a.1.value.total_cmp(&b.1.value))
}

/// Expected peak demand (summed CPU percent) for a scaling group over the
/// next hour, from the hourly peaks of its history.
pub async fn group_demand_peak(pool: &DbPool, group: uuid::Uuid) -> Option<f64> {
    let h = hourly(pool, &group_subject(group), "cpu_sum", 35, true)
        .await
        .ok()?;
    seasonal_peak(&h, chrono::Utc::now().timestamp(), 1).map(|(_, s)| s.value)
}

/// Recent samples for one subject+metric as (epoch seconds, value), oldest first.
pub async fn series(
    pool: &DbPool,
    subject: &str,
    metric: &str,
    window_hours: i64,
) -> anyhow::Result<Vec<(i64, f64)>> {
    let since = chrono::Utc::now().timestamp() - window_hours * 3600;
    Ok(crate::db::query_as(
        "SELECT ts, value FROM metric_samples WHERE subject = ? AND metric = ? AND ts >= ? ORDER BY ts",
    )
    .bind(subject)
    .bind(metric)
    .bind(since)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trend {
    /// Change per hour.
    pub slope: f64,
    pub intercept: f64,
    pub r2: f64,
}

/// Least-squares line through (hours, value) points. None for fewer than 2 points or no spread in time.
pub fn linreg(points: &[(f64, f64)]) -> Option<Trend> {
    let n = points.len() as f64;
    if points.len() < 2 {
        return None;
    }
    let (sx, sy) = points
        .iter()
        .fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (mx, my) = (sx / n, sy / n);
    let sxx: f64 = points.iter().map(|p| (p.0 - mx).powi(2)).sum();
    if sxx < 1e-12 {
        return None;
    }
    let sxy: f64 = points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let slope = sxy / sxx;
    let intercept = my - slope * mx;
    let ss_tot: f64 = points.iter().map(|p| (p.1 - my).powi(2)).sum();
    let ss_res: f64 = points
        .iter()
        .map(|p| (p.1 - (slope * p.0 + intercept)).powi(2))
        .sum();
    let r2 = if ss_tot < 1e-12 {
        0.0
    } else {
        (1.0 - ss_res / ss_tot).clamp(0.0, 1.0)
    };
    Some(Trend {
        slope,
        intercept,
        r2,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing {
    pub hours: f64,
    pub confidence: f64,
    /// Fitted rise per day, in the metric's own unit.
    pub per_day: f64,
}

/// When will the metric reach `threshold`? `samples` are (epoch seconds, value). None when there is too little
/// history, the trend is flat or falling, the fit is poor, or the crossing is beyond the useful horizon.
pub fn time_to_threshold(samples: &[(i64, f64)], threshold: f64) -> Option<Crossing> {
    if samples.len() < MIN_SAMPLES {
        return None;
    }
    let t0 = samples.first()?.0;
    let pts: Vec<(f64, f64)> = samples
        .iter()
        .map(|(t, v)| ((*t - t0) as f64 / 3600.0, *v))
        .collect();
    let span = pts.last()?.0;
    if span < MIN_SPAN_HOURS {
        return None;
    }
    let tr = linreg(&pts)?;
    if tr.slope <= 1e-9 || tr.r2 < 0.5 {
        return None;
    }
    let now_fit = tr.slope * span + tr.intercept;
    if now_fit >= threshold {
        return Some(Crossing {
            hours: 0.0,
            confidence: tr.r2,
            per_day: tr.slope * 24.0,
        });
    }
    let hours = (threshold - now_fit) / tr.slope;
    if hours > MAX_HORIZON_HOURS {
        return None;
    }
    // Fit quality, discounted when there is little history to fit.
    let history = (samples.len() as f64 / 48.0).min(1.0);
    let confidence = (tr.r2 * (0.6 + 0.4 * history) * 100.0).round() / 100.0;
    Some(Crossing {
        hours,
        confidence,
        per_day: tr.slope * 24.0,
    })
}

/// "~6 days" / "~14 hours" / "under an hour".
pub fn humanize_hours(h: f64) -> String {
    if h < 1.0 {
        "under an hour".into()
    } else if h < 48.0 {
        format!("~{:.0} hours", h)
    } else {
        format!("~{:.0} days", h / 24.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(n: usize, step_secs: i64, start: f64, per_hour: f64) -> Vec<(i64, f64)> {
        (0..n)
            .map(|i| {
                let t = i as i64 * step_secs;
                (t, start + per_hour * (t as f64 / 3600.0))
            })
            .collect()
    }

    #[test]
    fn linreg_recovers_slope_and_perfect_fit() {
        let pts: Vec<(f64, f64)> = (0..10).map(|i| (i as f64, 2.0 * i as f64 + 1.0)).collect();
        let t = linreg(&pts).unwrap();
        assert!((t.slope - 2.0).abs() < 1e-9 && (t.intercept - 1.0).abs() < 1e-9 && t.r2 > 0.999);
    }

    #[test]
    fn rising_series_gives_hours_to_the_limit() {
        // 0.50 now, +0.01/hour, over 24h => 0.74 at the end; 0.95 is 21 hours later.
        let s = line(48, 1800, 0.50, 0.01);
        let c = time_to_threshold(&s, 0.95).unwrap();
        assert!((c.hours - 21.0).abs() < 1.0, "{}", c.hours);
        assert!(c.confidence > 0.9);
    }

    #[test]
    fn too_little_history_gives_no_forecast() {
        assert!(time_to_threshold(&line(5, 300, 0.5, 0.01), 0.95).is_none());
        // plenty of samples but only 55 minutes of history
        assert!(time_to_threshold(&line(12, 300, 0.5, 0.01), 0.95).is_none());
    }

    #[test]
    fn flat_or_falling_never_crosses() {
        assert!(time_to_threshold(&line(48, 1800, 0.5, 0.0), 0.95).is_none());
        assert!(time_to_threshold(&line(48, 1800, 0.8, -0.01), 0.95).is_none());
    }

    #[test]
    fn noisy_data_is_rejected_not_extrapolated() {
        let s: Vec<(i64, f64)> = (0..48)
            .map(|i| (i * 1800, if i % 2 == 0 { 0.2 } else { 0.9 }))
            .collect();
        assert!(time_to_threshold(&s, 0.95).is_none());
    }

    #[test]
    fn already_past_the_limit_is_zero_hours() {
        let s = line(48, 1800, 0.90, 0.01);
        assert_eq!(time_to_threshold(&s, 0.95).unwrap().hours, 0.0);
    }

    #[test]
    fn far_future_crossings_are_dropped() {
        // +0.0001/hour needs ~months to reach the limit
        assert!(time_to_threshold(&line(48, 1800, 0.10, 0.0001), 0.95).is_none());
    }

    #[test]
    fn seasonal_prefers_weekly_then_daily() {
        let now = 100 * WEEK + 10 * HOUR + 600;
        let at = now / HOUR * HOUR;
        let mut h = BTreeMap::new();
        h.insert(at - DAY, 30.0);
        h.insert(at - 2 * DAY, 40.0);
        assert_eq!(seasonal_at(&h, at), None);
        h.insert(at - 3 * DAY, 50.0);
        let d = seasonal_at(&h, at).unwrap();
        assert_eq!((d.basis, d.periods, d.value), ("daily", 3, 40.0));
        h.insert(at - WEEK, 200.0);
        h.insert(at - 2 * WEEK, 100.0);
        let w = seasonal_at(&h, at).unwrap();
        assert_eq!((w.basis, w.periods, w.value), ("weekly", 2, 150.0));
    }

    #[test]
    fn seasonal_peak_finds_the_coming_rush() {
        let now = 50 * WEEK + 8 * HOUR + 1800;
        let mut h = BTreeMap::new();
        for d in 1..=5 {
            let day = now / HOUR * HOUR - d * DAY;
            h.insert(day, 20.0);
            h.insert(day + HOUR, 180.0);
            h.insert(day + 2 * HOUR, 30.0);
        }
        let (hour, s) = seasonal_peak(&h, now, 1).unwrap();
        assert_eq!(hour, now / HOUR * HOUR + HOUR);
        assert_eq!(s.value, 180.0);
        assert_eq!(seasonal_peak(&BTreeMap::new(), now, 1), None);
    }

    #[test]
    fn humanize() {
        assert_eq!(humanize_hours(0.2), "under an hour");
        assert_eq!(humanize_hours(14.0), "~14 hours");
        assert_eq!(humanize_hours(150.0), "~6 days");
    }
}
