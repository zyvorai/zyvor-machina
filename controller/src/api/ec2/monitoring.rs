// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! CloudWatch-shaped actions. boto3's cloudwatch client signs for service `monitoring`; those requests
//! reach `services::monitoring_dispatch` (`POST /monitoring`, or any alias) and the ec2-shaped bodies below
//! are converted to CloudWatch's shape there. The actions also keep answering on `ec2` for older clients.
//! No notification action. `INSUFFICIENT_DATA` does not fire.

use std::collections::BTreeMap;

use axum::extract::{Path, Query, State};
use axum::Extension;
use axum::Json;
use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

use super::more::api_err;
use super::{indexed, xml_escape, Ec2Error};

type Params = BTreeMap<String, String>;

fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

fn need(p: &Params, k: &str) -> Result<String, Ec2Error> {
    p.get(k).cloned().ok_or_else(|| bad("MissingParameter", format!("The request must contain the parameter {k}")))
}

fn comparator(op: &str) -> Result<&'static str, Ec2Error> {
    match op {
        "GreaterThanThreshold" | "gt" => Ok("gt"),
        "GreaterThanOrEqualToThreshold" | "gte" => Ok("gte"),
        "LessThanThreshold" | "lt" => Ok("lt"),
        "LessThanOrEqualToThreshold" | "lte" => Ok("lte"),
        _ => Err(bad("InvalidParameterValue", "ComparisonOperator must be GreaterThanThreshold or LessThanThreshold")),
    }
}

fn comparator_out(stored: &str) -> &str {
    match stored {
        "gt" => "GreaterThanThreshold",
        "gte" => "GreaterThanOrEqualToThreshold",
        "lt" => "LessThanThreshold",
        "lte" => "LessThanOrEqualToThreshold",
        other => other,
    }
}

fn alarm_item(row: &crate::api::alarms::AlarmRow) -> String {
    let group = row.group_id.map(|id| format!("<instanceGroupId>{}</instanceGroupId>", ec2_id(Kind::InstanceGroup, id))).unwrap_or_default();
    format!(
        "<item><alarmName>{}</alarmName><metricName>{}</metricName><namespace>Machina/EC2</namespace><statistic>{}</statistic>\
<period>{}</period><evaluationPeriods>{}</evaluationPeriods><threshold>{}</threshold><comparisonOperator>{}</comparisonOperator>\
<stateValue>{}</stateValue><stateReason>{}</stateReason><actionsEnabled>{}</actionsEnabled>{group}</item>",
        xml_escape(&row.name),
        xml_escape(&row.metric),
        xml_escape(&row.statistic),
        row.period_secs,
        row.evaluation_periods,
        row.threshold,
        comparator_out(&row.comparator),
        xml_escape(&row.state),
        xml_escape(&row.state_reason),
        row.enabled
    )
}

pub async fn describe_alarms(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    let Json(rows) = crate::api::alarms::list_alarms(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    let wanted = indexed(p, "AlarmName");
    let items: String = rows
        .iter()
        .filter(|r| wanted.is_empty() || wanted.iter().any(|n| n == &r.name))
        .map(alarm_item)
        .collect();
    Ok(format!("<metricAlarms>{items}</metricAlarms>"))
}

pub async fn put_metric_alarm(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let name = need(p, "AlarmName")?;
    let metric = need(p, "MetricName")?;
    let subject = p.get("Dimensions.member.1.Value").cloned().or_else(|| p.get("Subject").cloned()).unwrap_or_else(|| name.clone());
    let threshold: f64 = need(p, "Threshold")?.parse().map_err(|_| bad("InvalidParameterValue", "Threshold must be a number"))?;
    let period: i64 = p.get("Period").map(|s| s.parse()).transpose().map_err(|_| bad("InvalidParameterValue", "Period must be a number"))?.unwrap_or(300);
    let evaluation: i64 = p.get("EvaluationPeriods").map(|s| s.parse()).transpose().map_err(|_| bad("InvalidParameterValue", "EvaluationPeriods must be a number"))?.unwrap_or(1);
    let statistic = p.get("Statistic").cloned().unwrap_or_else(|| "Average".into());
    let comparison = comparator(p.get("ComparisonOperator").map(String::as_str).unwrap_or("GreaterThanThreshold"))?;
    let policy_arn = super::autoscaling::members(p, "AlarmActions").into_iter().find(|a| a.starts_with("arn:aws:autoscaling:"));
    let (action, group_id, step) = if let Some(group) = p.get("InstanceGroupId") {
        let id = super::more::resolve(state, Kind::InstanceGroup, group, "InvalidParameterValue").await?;
        let step: i64 = p.get("Step").map(|s| s.parse()).transpose().map_err(|_| bad("InvalidParameterValue", "Step must be a number"))?.unwrap_or(1);
        ("scale_group".to_string(), Some(id), step)
    } else if let Some(arn) = policy_arn {
        // an Auto Scaling policy as the alarm action: the alarm applies the policy's step to its group
        let (group, step) = super::autoscaling::alarm_target(state, &arn).await?;
        ("scale_group".to_string(), Some(group), step)
    } else {
        ("none".into(), None, 0)
    };
    let body = crate::api::alarms::CreateAlarm {
        name,
        subject,
        metric,
        statistic,
        period_secs: period,
        evaluation_periods: evaluation,
        comparator: comparison.into(),
        threshold,
        action,
        group_id,
        step,
        cooldown_secs: 300,
    };
    let _ = crate::api::alarms::create_alarm(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}

pub async fn delete_alarms(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let names = indexed(p, "AlarmName");
    if names.is_empty() {
        return Err(bad("MissingParameter", "The request must contain the parameter AlarmName.1"));
    }
    for name in names {
        let id: Option<Uuid> = crate::db::query_scalar("SELECT id FROM cloud_alarms WHERE name = ?")
            .bind(&name)
            .fetch_optional(&state.pool)
            .await?;
        if let Some(id) = id {
            let _ = crate::api::alarms::delete_alarm(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
        }
    }
    Ok("<return>true</return>".into())
}

pub async fn set_alarm_actions(state: &AppState, actor: &AuthUser, p: &Params, enabled: bool) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    for name in indexed(p, "AlarmName") {
        let id: Option<Uuid> = crate::db::query_scalar("SELECT id FROM cloud_alarms WHERE name = ?")
            .bind(&name)
            .fetch_optional(&state.pool)
            .await?;
        let Some(id) = id else { continue };
        let _ = crate::api::alarms::set_alarm_enabled(
            State(state.clone()),
            Extension(actor.clone()),
            Path(id),
            Json(crate::api::alarms::Enabled { enabled }),
        )
        .await
        .map_err(api_err)?;
    }
    Ok("<return>true</return>".into())
}

pub async fn get_metric_statistics(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let subject = p.get("Dimensions.member.1.Value").cloned().or_else(|| p.get("Subject").cloned()).ok_or_else(|| bad("MissingParameter", "Dimensions.member.1.Value (the instance id or name) is required"))?;
    let metric = need(p, "MetricName")?;
    let period = p.get("Period").and_then(|s| s.parse().ok());
    let stats = p.get("Statistics.member.1").cloned().or_else(|| p.get("Statistics").cloned());
    let query = crate::api::metric_stats::StatsQuery {
        subject,
        metric,
        period,
        start: p.get("StartTime").and_then(|s| s.parse().ok()),
        end: p.get("EndTime").and_then(|s| s.parse().ok()),
        statistics: stats,
    };
    let Json(body) = crate::api::metric_stats::statistics(State(state.clone()), Extension(actor.clone()), Query(query)).await.map_err(api_err)?;
    let points = body.get("datapoints").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let items: String = points
        .iter()
        .map(|d| {
            let ts = d.get("timestamp").and_then(|v| v.as_i64()).unwrap_or(0);
            let avg = d.get("average").and_then(|v| v.as_f64());
            let max = d.get("maximum").and_then(|v| v.as_f64());
            let min = d.get("minimum").and_then(|v| v.as_f64());
            let sum = d.get("sum").and_then(|v| v.as_f64());
            let n = d.get("sample_count").and_then(|v| v.as_u64());
            format!(
                "<item><timestamp>{ts}</timestamp>{}{}{}{}{}</item>",
                avg.map(|v| format!("<average>{v}</average>")).unwrap_or_default(),
                max.map(|v| format!("<maximum>{v}</maximum>")).unwrap_or_default(),
                min.map(|v| format!("<minimum>{v}</minimum>")).unwrap_or_default(),
                sum.map(|v| format!("<sum>{v}</sum>")).unwrap_or_default(),
                n.map(|v| format!("<sampleCount>{v}</sampleCount>")).unwrap_or_default()
            )
        })
        .collect();
    Ok(format!("<label>{}</label><datapoints>{items}</datapoints>", xml_escape(body.get("metric").and_then(|v| v.as_str()).unwrap_or(""))))
}
