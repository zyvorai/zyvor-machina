// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! CloudWatch-style statistics over the stored metric samples: `GET /api/v1/metrics/statistics`.

use std::collections::BTreeMap;

use crate::db::DbPool;
use uuid::Uuid;

use axum::extract::{Query, State};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

const MAX_POINTS: i64 = 1440;
const MAX_RANGE_SECS: i64 = 15 * 24 * 3600;

#[derive(Debug, Deserialize)]
pub struct StatsQuery {
    /// Sample subject, e.g. a VM name.
    pub subject: String,
    pub metric: String,
    /// Bucket width in seconds (60..=86400, a multiple of 60). Default 300.
    #[serde(default)]
    pub period: Option<i64>,
    /// Unix seconds; default: the last hour.
    #[serde(default)]
    pub start: Option<i64>,
    #[serde(default)]
    pub end: Option<i64>,
    /// Comma list of Average, Minimum, Maximum, Sum, SampleCount (default Average).
    #[serde(default)]
    pub statistics: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Datapoint {
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sum: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_count: Option<u64>,
}

/// How a sample's `subject` is stored: text (`pool:<id>`, `group:<hex>`, synthetic subjects) or, for machines, the machine's
/// 16-byte id. A caller names a machine by name, `i-…` id or UUID, so every candidate is tried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SubjectKey {
    Text(String),
    Id(Uuid),
}

/// The candidates that need no database lookup, plus the machine name / EC2 id hex / group hex still to resolve.
pub(crate) fn classify(subject: &str) -> (Vec<SubjectKey>, Option<String>, Option<String>, Option<String>) {
    let mut keys = vec![SubjectKey::Text(subject.to_string())];
    if let Ok(u) = Uuid::parse_str(subject) {
        keys.push(SubjectKey::Id(u));
        return (keys, None, None, None);
    }
    if let Some(hex) = subject.strip_prefix("group:") {
        return (keys, None, None, Some(hex.to_ascii_lowercase()));
    }
    if let Some((crate::resource_ids::Kind::Vm, hex)) = crate::resource_ids::parse(subject) {
        return (keys, None, Some(hex), None);
    }
    (keys, Some(subject.to_string()), None, None)
}

/// Every key the samples of `subject` may be stored under.
pub(crate) async fn subject_keys(pool: &DbPool, subject: &str) -> Vec<SubjectKey> {
    let (mut keys, name, ec2_hex, group_hex) = classify(subject);
    if let Some(n) = name {
        if let Ok(Some(id)) = crate::db::query_scalar::<_, Uuid>("SELECT id FROM vms WHERE name = ?").bind(n).fetch_optional(pool).await {
            keys.push(SubjectKey::Id(id));
        }
    }
    if let Some(hex) = ec2_hex {
        if let Ok(ids) = crate::db::query_scalar::<_, Uuid>("SELECT id FROM vms WHERE lower(hex(id)) LIKE ?").bind(format!("{hex}%")).fetch_all(pool).await {
            keys.extend(ids.into_iter().take(1).map(SubjectKey::Id));
        }
    }
    if let Some(hex) = group_hex {
        // a group's metric is its members' samples
        if let Ok(ids) = crate::db::query_scalar::<_, Uuid>("SELECT vm_id FROM cloud_group_members WHERE lower(hex(group_id)) = ? AND vm_id IS NOT NULL")
            .bind(hex)
            .fetch_all(pool)
            .await
        {
            keys.extend(ids.into_iter().map(SubjectKey::Id));
        }
    }
    keys
}

/// `(ts, value)` samples of `metric` for all `keys` in `[start, end)`.
pub(crate) async fn fetch_samples(pool: &DbPool, keys: &[SubjectKey], metric: &str, start: i64, end: i64) -> Result<Vec<(i64, f64)>, sqlx::Error> {
    let sql = "SELECT ts, value FROM metric_samples WHERE subject = ? AND metric = ? AND ts >= ? AND ts < ? ORDER BY ts";
    let mut out = Vec::new();
    // On PostgreSQL a machine's id and its canonical text are the same subject; query it once.
    #[cfg(feature = "postgres")]
    let mut seen = std::collections::HashSet::new();
    for k in keys {
        #[cfg(feature = "postgres")]
        {
            let text = match k {
                SubjectKey::Text(t) => t.clone(),
                SubjectKey::Id(u) => u.to_string(),
            };
            if !seen.insert(text) {
                continue;
            }
        }
        let q = crate::db::query_as::<_, (i64, f64)>(sql);
        let q = match k {
            SubjectKey::Text(t) => q.bind(t.clone()),
            SubjectKey::Id(u) => q.bind(crate::db::subject_id(*u)),
        };
        out.extend(q.bind(metric).bind(start).bind(end).fetch_all(pool).await?);
    }
    out.sort_by_key(|s| s.0);
    Ok(out)
}

pub(crate) fn parse_statistics(s: Option<&str>) -> Result<Vec<&'static str>, String> {
    let mut out = Vec::new();
    for part in s.unwrap_or("Average").split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let name = match part {
            "Average" => "Average",
            "Minimum" => "Minimum",
            "Maximum" => "Maximum",
            "Sum" => "Sum",
            "SampleCount" => "SampleCount",
            other => return Err(format!("unknown statistic '{other}'")),
        };
        if !out.contains(&name) {
            out.push(name);
        }
    }
    if out.is_empty() {
        return Err("no statistics requested".into());
    }
    Ok(out)
}

/// Bucket `(ts, value)` samples into `period`-second windows aligned to the epoch, oldest first.
pub(crate) fn aggregate(samples: &[(i64, f64)], period: i64, stats: &[&str]) -> Vec<Datapoint> {
    let mut buckets: BTreeMap<i64, Vec<f64>> = BTreeMap::new();
    for (ts, v) in samples {
        buckets.entry(ts - ts.rem_euclid(period)).or_default().push(*v);
    }
    let want = |n: &str| stats.contains(&n);
    buckets
        .into_iter()
        .map(|(timestamp, vals)| {
            let sum: f64 = vals.iter().sum();
            Datapoint {
                timestamp,
                average: want("Average").then(|| sum / vals.len() as f64),
                minimum: want("Minimum").then(|| vals.iter().copied().fold(f64::INFINITY, f64::min)),
                maximum: want("Maximum").then(|| vals.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
                sum: want("Sum").then_some(sum),
                sample_count: want("SampleCount").then_some(vals.len() as u64),
            }
        })
        .collect()
}

pub(crate) fn validate_window(period: i64, start: i64, end: i64) -> Result<(), String> {
    if !(60..=86_400).contains(&period) || period % 60 != 0 {
        return Err("period must be a multiple of 60 between 60 and 86400".into());
    }
    if end <= start {
        return Err("end must be after start".into());
    }
    if end - start > MAX_RANGE_SECS {
        return Err("the range is limited to 15 days".into());
    }
    if (end - start) / period > MAX_POINTS {
        return Err(format!("that would return more than {MAX_POINTS} datapoints; use a longer period"));
    }
    Ok(())
}

pub async fn statistics(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<StatsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    if q.subject.is_empty() || q.subject.len() > 128 || q.metric.is_empty() || q.metric.len() > 128 {
        return Err(ApiError::bad_request("subject and metric are required (at most 128 characters)"));
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let end = q.end.unwrap_or(now);
    let start = q.start.unwrap_or(end - 3600);
    let period = q.period.unwrap_or(300);
    validate_window(period, start, end).map_err(ApiError::bad_request)?;
    let stats = parse_statistics(q.statistics.as_deref()).map_err(ApiError::bad_request)?;
    let keys = subject_keys(&state.pool, &q.subject).await;
    let samples = fetch_samples(&state.pool, &keys, &q.metric, start, end).await?;
    Ok(Json(serde_json::json!({
        "subject": q.subject,
        "metric": q.metric,
        "period": period,
        "datapoints": aggregate(&samples, period, &stats),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_are_epoch_aligned_and_summarised() {
        let s = [(0, 1.0), (30, 3.0), (60, 10.0), (119, 20.0), (120, 5.0)];
        let d = aggregate(&s, 60, &["Average", "Maximum", "Minimum", "Sum", "SampleCount"]);
        assert_eq!(d.len(), 3);
        assert_eq!(d[0].timestamp, 0);
        assert_eq!(d[0].average, Some(2.0));
        assert_eq!(d[0].maximum, Some(3.0));
        assert_eq!(d[1].timestamp, 60);
        assert_eq!(d[1].minimum, Some(10.0));
        assert_eq!(d[1].sum, Some(30.0));
        assert_eq!(d[1].sample_count, Some(2));
        assert_eq!(d[2].timestamp, 120);
    }

    #[test]
    fn only_requested_statistics_are_filled() {
        let d = aggregate(&[(0, 4.0)], 60, &["Maximum"]);
        assert_eq!(d[0].maximum, Some(4.0));
        assert_eq!(d[0].average, None);
        assert!(aggregate(&[], 60, &["Average"]).is_empty());
    }

    #[test]
    fn statistics_names_are_validated_and_deduplicated() {
        assert_eq!(parse_statistics(None).unwrap(), ["Average"]);
        assert_eq!(parse_statistics(Some("Sum, Sum,Maximum")).unwrap(), ["Sum", "Maximum"]);
        assert!(parse_statistics(Some("P99")).is_err());
        assert!(parse_statistics(Some(" , ")).is_err());
    }

    #[test]
    fn classify_names_uuids_ec2_ids_and_groups() {
        let (k, name, ec2, group) = classify("web-1");
        assert_eq!(k, vec![SubjectKey::Text("web-1".into())]);
        assert_eq!((name.as_deref(), ec2, group), (Some("web-1"), None, None));
        let u = "6f9619ff-8b86-d011-b42d-00cf4fc964ff";
        let (k, name, _, _) = classify(u);
        assert!(k.contains(&SubjectKey::Id(Uuid::parse_str(u).unwrap())) && name.is_none());
        let (_, name, ec2, _) = classify("i-0123456789abcdef0");
        assert_eq!((name, ec2.as_deref()), (None, Some("0123456789abcdef0")));
        let (_, _, _, g) = classify("group:ABCDEF");
        assert_eq!(g.as_deref(), Some("abcdef"));
    }

    #[tokio::test]
    async fn samples_stored_under_the_machine_id_are_found_by_name_ec2_id_and_uuid() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        let id = Uuid::parse_str("0123456789abcdef0123456789abcdef").unwrap();
        crate::db::query("INSERT INTO vms (id, name) VALUES (?, 'web-1')").bind(id).execute(&state.pool).await.unwrap();
        // the sampler stores a machine's samples under its 16-byte id
        crate::db::query("INSERT INTO metric_samples (subject, metric, ts, value) VALUES (?, 'cpu_percent', 100, 42.0)")
            .bind(crate::db::subject_id(id))
            .execute(&state.pool)
            .await
            .unwrap();
        for subject in ["web-1", "i-0123456789abcdef0", "01234567-89ab-cdef-0123-456789abcdef"] {
            let keys = subject_keys(&state.pool, subject).await;
            let s = fetch_samples(&state.pool, &keys, "cpu_percent", 0, 1000).await.unwrap();
            assert_eq!(s, vec![(100, 42.0)], "subject {subject}");
        }
        let none = subject_keys(&state.pool, "other").await;
        assert!(fetch_samples(&state.pool, &none, "cpu_percent", 0, 1000).await.unwrap().is_empty());
    }

    #[test]
    fn window_limits() {
        assert!(validate_window(300, 0, 3600).is_ok());
        assert!(validate_window(30, 0, 3600).is_err());
        assert!(validate_window(90, 0, 3600).is_err());
        assert!(validate_window(300, 100, 100).is_err());
        assert!(validate_window(60, 0, 60 * 1441).is_err());
        assert!(validate_window(86_400, 0, MAX_RANGE_SECS + 1).is_err());
    }
}
