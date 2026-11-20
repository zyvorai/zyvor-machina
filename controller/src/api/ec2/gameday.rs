// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Game-day runs. Creating an experiment stays on REST: the spec is a structured fault list.
//! Run and abort are here. Run requires Confirm equal to the experiment name.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::state::AppState;

use super::more::api_err;
use super::{xml_escape, Ec2Error};

type Params = BTreeMap<String, String>;

fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

fn need(p: &Params, k: &str) -> Result<String, Ec2Error> {
    p.get(k).cloned().ok_or_else(|| bad("MissingParameter", format!("The request must contain the parameter {k}")))
}

pub async fn describe_experiments(state: &AppState, _actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    let Json(rows) = crate::api::chaos::list_experiments(State(state.clone())).await.map_err(api_err)?;
    let items: String = rows
        .iter()
        .map(|e| {
            format!(
                "<item><experimentId>{}</experimentId><name>{}</name><description>{}</description><lastStatus>{}</lastStatus></item>",
                e.id,
                xml_escape(&e.name),
                xml_escape(&e.description),
                xml_escape(e.last_status.as_deref().unwrap_or(""))
            )
        })
        .collect();
    Ok(format!("<experimentSet>{items}</experimentSet>"))
}

pub async fn run_experiment(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    let id: Uuid = need(p, "ExperimentId")?.parse().map_err(|_| bad("InvalidParameterValue", "ExperimentId must be a UUID"))?;
    let body: crate::api::chaos::RunBody = serde_json::from_value(serde_json::json!({ "confirm": need(p, "Confirm")? }))
        .map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
    let Json(row) = crate::api::chaos::run_experiment(State(state.clone()), Extension(actor.clone()), Path(id), Json(body)).await.map_err(api_err)?;
    Ok(format!("<experimentId>{id}</experimentId><run>{}</run>", xml_escape(&row.to_string())))
}

pub async fn abort_experiment(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    let id: Uuid = need(p, "RunId")?.parse().map_err(|_| bad("InvalidParameterValue", "RunId must be a UUID"))?;
    let _ = crate::api::chaos::abort_run(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}
