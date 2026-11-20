// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! `/api/v1/alarms`: CloudWatch-style alarms (see `engine::alarms`).

use axum::extract::{Path, State};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::cloud::access;
use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::alarms::Cmp;
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AlarmRow {
    pub id: Uuid,
    pub name: String,
    pub subject: String,
    pub metric: String,
    pub statistic: String,
    pub period_secs: i64,
    pub evaluation_periods: i64,
    pub comparator: String,
    pub threshold: f64,
    pub state: String,
    pub state_reason: String,
    pub state_updated_at: String,
    pub action: String,
    pub group_id: Option<Uuid>,
    pub step: i64,
    pub cooldown_secs: i64,
    pub last_action_at: Option<String>,
    pub enabled: bool,
}

const SELECT: &str = "SELECT id, name, subject, metric, statistic, period_secs, evaluation_periods, comparator, threshold, state, \
    state_reason, state_updated_at, action, group_id, step, cooldown_secs, last_action_at, enabled FROM cloud_alarms";

#[derive(Debug, Deserialize)]
pub struct CreateAlarm {
    pub name: String,
    pub subject: String,
    pub metric: String,
    #[serde(default = "avg")]
    pub statistic: String,
    #[serde(default = "five_min")]
    pub period_secs: i64,
    #[serde(default = "one")]
    pub evaluation_periods: i64,
    #[serde(default = "gt")]
    pub comparator: String,
    pub threshold: f64,
    /// `none` (default) or `scale_group`.
    #[serde(default = "none")]
    pub action: String,
    #[serde(default)]
    pub group_id: Option<Uuid>,
    /// Change of the group's desired size per firing (non-zero, within ±10).
    #[serde(default)]
    pub step: i64,
    #[serde(default = "five_min")]
    pub cooldown_secs: i64,
}

fn avg() -> String {
    "Average".into()
}
fn five_min() -> i64 {
    300
}
fn one() -> i64 {
    1
}
fn gt() -> String {
    "gt".into()
}
fn none() -> String {
    "none".into()
}

pub(crate) fn validate(b: &CreateAlarm) -> Result<(), String> {
    machina_spec::validate_name(&b.name).map_err(|e| e.to_string())?;
    if b.subject.is_empty() || b.subject.len() > 128 || b.metric.is_empty() || b.metric.len() > 128 {
        return Err("subject and metric are required (at most 128 characters)".into());
    }
    if !["Average", "Minimum", "Maximum", "Sum", "SampleCount"].contains(&b.statistic.as_str()) {
        return Err("statistic must be Average, Minimum, Maximum, Sum or SampleCount".into());
    }
    if !(60..=3600).contains(&b.period_secs) || b.period_secs % 60 != 0 {
        return Err("period_secs must be a multiple of 60 between 60 and 3600".into());
    }
    if !(1..=10).contains(&b.evaluation_periods) {
        return Err("evaluation_periods must be 1 to 10".into());
    }
    if Cmp::parse(&b.comparator).is_none() {
        return Err("comparator must be gt, gte, lt or lte".into());
    }
    if !b.threshold.is_finite() {
        return Err("threshold must be a number".into());
    }
    if !(0..=86_400).contains(&b.cooldown_secs) {
        return Err("cooldown_secs must be 0 to 86400".into());
    }
    match b.action.as_str() {
        "none" => {
            if b.group_id.is_some() || b.step != 0 {
                return Err("group_id and step only apply to action scale_group".into());
            }
        }
        "scale_group" => {
            if b.group_id.is_none() {
                return Err("scale_group needs a group_id".into());
            }
            if b.step == 0 || b.step.abs() > 10 {
                return Err("step must be non-zero and within ±10".into());
            }
        }
        _ => return Err("action must be none or scale_group".into()),
    }
    Ok(())
}

pub async fn list_alarms(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<AlarmRow>>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(crate::db::query_as(&format!("{SELECT} ORDER BY name")).fetch_all(&state.pool).await?))
}

pub async fn get_alarm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<AlarmRow>, ApiError> {
    require_operator(&actor)?;
    let row = crate::db::query_as(&format!("{SELECT} WHERE id = ?")).bind(id).fetch_optional(&state.pool).await?;
    row.map(Json).ok_or_else(|| ApiError::not_found("alarm not found"))
}

pub async fn create_alarm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(b): Json<CreateAlarm>,
) -> Result<Json<AlarmRow>, ApiError> {
    require_operator(&actor)?;
    validate(&b).map_err(ApiError::bad_request)?;
    let mut conn = state.pool.acquire().await?;
    if let Some(g) = b.group_id {
        let project: Option<Uuid> = crate::db::query_scalar("SELECT project_id FROM cloud_instance_groups WHERE id = ?")
            .bind(g)
            .fetch_optional(&mut *conn)
            .await?;
        let project = project.ok_or_else(|| ApiError::bad_request("no such instance group"))?;
        access(&mut conn, &actor, project, true).await?;
    }
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO cloud_alarms (id, name, subject, metric, statistic, period_secs, evaluation_periods, comparator, threshold, \
         action, group_id, step, cooldown_secs, created_by) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&b.name)
    .bind(&b.subject)
    .bind(&b.metric)
    .bind(&b.statistic)
    .bind(b.period_secs)
    .bind(b.evaluation_periods)
    .bind(&b.comparator)
    .bind(b.threshold)
    .bind(&b.action)
    .bind(b.group_id)
    .bind(b.step)
    .bind(b.cooldown_secs)
    .bind(&actor.username)
    .execute(&mut *conn)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            ApiError::conflict("an alarm with that name exists", "choose another name")
        }
        other => other.into(),
    })?;
    drop(conn);
    get_alarm(State(state), Extension(actor), Path(id)).await
}

#[derive(Deserialize)]
pub struct Enabled {
    pub enabled: bool,
}

pub async fn set_alarm_enabled(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(b): Json<Enabled>,
) -> Result<Json<AlarmRow>, ApiError> {
    require_operator(&actor)?;
    // Re-enabling starts from "no data" so a stale ALARM cannot fire an action on the first tick.
    let r = crate::db::query("UPDATE cloud_alarms SET enabled = ?, state = CASE WHEN ? THEN 'INSUFFICIENT_DATA' ELSE state END WHERE id = ?")
        .bind(b.enabled)
        .bind(b.enabled)
        .bind(id)
        .execute(&state.pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found("alarm not found"));
    }
    get_alarm(State(state), Extension(actor), Path(id)).await
}

pub async fn delete_alarm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let r = crate::db::query("DELETE FROM cloud_alarms WHERE id = ?").bind(id).execute(&state.pool).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found("alarm not found"));
    }
    Ok(Json(serde_json::json!({ "deleted": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok() -> CreateAlarm {
        CreateAlarm {
            name: "cpu-high".into(),
            subject: "web-1".into(),
            metric: "cpu_percent".into(),
            statistic: "Average".into(),
            period_secs: 300,
            evaluation_periods: 2,
            comparator: "gt".into(),
            threshold: 80.0,
            action: "none".into(),
            group_id: None,
            step: 0,
            cooldown_secs: 300,
        }
    }

    #[test]
    fn a_plain_alarm_validates() {
        assert!(validate(&ok()).is_ok());
    }

    #[test]
    fn bad_fields_are_refused() {
        for f in [
            |a: &mut CreateAlarm| a.statistic = "P99".into(),
            |a: &mut CreateAlarm| a.period_secs = 90,
            |a: &mut CreateAlarm| a.evaluation_periods = 0,
            |a: &mut CreateAlarm| a.comparator = "eq".into(),
            |a: &mut CreateAlarm| a.threshold = f64::NAN,
            |a: &mut CreateAlarm| a.cooldown_secs = -1,
            |a: &mut CreateAlarm| a.subject = String::new(),
        ] {
            let mut a = ok();
            f(&mut a);
            assert!(validate(&a).is_err());
        }
    }

    #[test]
    fn scale_group_needs_a_group_and_a_sane_step() {
        let mut a = ok();
        a.action = "scale_group".into();
        assert!(validate(&a).is_err(), "no group");
        a.group_id = Some(Uuid::new_v4());
        assert!(validate(&a).is_err(), "no step");
        a.step = 11;
        assert!(validate(&a).is_err(), "step too big");
        a.step = -2;
        assert!(validate(&a).is_ok());
        let mut n = ok();
        n.step = 1;
        assert!(validate(&n).is_err(), "step without an action");
    }
}
