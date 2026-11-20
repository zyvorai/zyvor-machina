// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Chaos game days: experiments, runs and the faults live on the hosts.

use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use machina_bpf::api::Request;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::bpf;
use crate::engine::chaos::{self, ExperimentSpec};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Experiment {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    #[serde(skip)]
    spec_json: String,
    #[sqlx(skip)]
    pub spec: Value,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
    #[sqlx(default)]
    pub last_status: Option<String>,
    #[sqlx(default)]
    pub last_run_at: Option<String>,
}

const EXPERIMENTS: &str = "SELECT e.id, e.name, e.description, e.spec_json, e.created_by, e.created_at, e.updated_at, \
    (SELECT status FROM chaos_runs r WHERE r.experiment_id = e.id ORDER BY r.started_at DESC LIMIT 1) AS last_status, \
    (SELECT started_at FROM chaos_runs r WHERE r.experiment_id = e.id ORDER BY r.started_at DESC LIMIT 1) AS last_run_at \
    FROM chaos_experiments e";

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Run {
    pub id: Uuid,
    pub experiment_id: Uuid,
    pub status: String,
    pub started_by: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub abort_reason: String,
    #[serde(skip)]
    report_json: String,
    #[sqlx(skip)]
    pub report: Value,
}

impl Run {
    fn parsed(mut self) -> Self {
        self.report = serde_json::from_str(&self.report_json).unwrap_or(Value::Null);
        self
    }
}

impl Experiment {
    fn parsed(mut self) -> Self {
        self.spec = serde_json::from_str(&self.spec_json).unwrap_or(Value::Null);
        self
    }
}

const RUNS: &str = "SELECT id, experiment_id, status, started_by, started_at, finished_at, abort_reason, report_json FROM chaos_runs";

fn bad(e: impl ToString) -> ApiError {
    ApiError::bad_request(e.to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentBody {
    name: String,
    #[serde(default)]
    description: String,
    spec: ExperimentSpec,
}

async fn check_targets(state: &AppState, spec: &ExperimentSpec) -> Result<(), ApiError> {
    for id in &spec.targets {
        let tags: Option<Option<String>> = crate::db::query_scalar("SELECT tags FROM vms WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
        let Some(tags) = tags else {
            return Err(ApiError::not_found(format!("target VM {id} not found")));
        };
        let protected = tags
            .as_deref()
            .and_then(|t| serde_json::from_str::<Vec<String>>(t).ok())
            .unwrap_or_default()
            .iter()
            .any(|t| t == "chaos=protected");
        if protected {
            return Err(ApiError::forbidden(format!(
                "target VM {id} is tagged chaos=protected"
            )));
        }
    }
    Ok(())
}

fn validate_body(b: &ExperimentBody) -> Result<(), ApiError> {
    machina_spec::validate_name(&b.name).map_err(bad)?;
    if b.description.len() > 2000 {
        return Err(bad("description at most 2000 characters"));
    }
    chaos::validate(&b.spec).map_err(bad)
}

async fn experiment(state: &AppState, id: Uuid) -> Result<Experiment, ApiError> {
    let e: Experiment = crate::db::query_as(&format!("{EXPERIMENTS} WHERE e.id = ?"))
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(e.parsed())
}

async fn running(state: &AppState, id: Uuid) -> Result<bool, ApiError> {
    Ok(crate::db::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM chaos_runs WHERE experiment_id = ? AND status = 'running')",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?)
}

pub async fn list_experiments(
    State(state): State<AppState>,
) -> Result<Json<Vec<Experiment>>, ApiError> {
    let rows: Vec<Experiment> = crate::db::query_as(&format!("{EXPERIMENTS} ORDER BY e.name LIMIT 500"))
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(rows.into_iter().map(Experiment::parsed).collect()))
}

pub async fn create_experiment(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(b): Json<ExperimentBody>,
) -> Result<Json<Experiment>, ApiError> {
    require_operator(&actor)?;
    validate_body(&b)?;
    check_targets(&state, &b.spec).await?;
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO chaos_experiments (id, name, description, spec_json, created_by) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&b.name)
    .bind(&b.description)
    .bind(serde_json::to_string(&b.spec).map_err(bad)?)
    .bind(&actor.username)
    .execute(&state.pool)
    .await?;
    Ok(Json(experiment(&state, id).await?))
}

pub async fn get_experiment(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let e = experiment(&state, id).await?;
    let runs: Vec<Run> = crate::db::query_as(&format!(
        "{RUNS} WHERE experiment_id = ? ORDER BY started_at DESC LIMIT 20"
    ))
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let runs: Vec<Run> = runs.into_iter().map(Run::parsed).collect();
    let total = serde_json::from_value::<ExperimentSpec>(e.spec.clone())
        .map(|s| chaos::total_secs(&s))
        .unwrap_or(0);
    Ok(Json(
        json!({ "experiment": e, "runs": runs, "max_secs": total }),
    ))
}

pub async fn update_experiment(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(b): Json<ExperimentBody>,
) -> Result<Json<Experiment>, ApiError> {
    require_operator(&actor)?;
    validate_body(&b)?;
    check_targets(&state, &b.spec).await?;
    if running(&state, id).await? {
        return Err(ApiError::conflict(
            "the experiment is running",
            "Abort or wait for the run first.",
        ));
    }
    let n = crate::db::query("UPDATE chaos_experiments SET name = ?, description = ?, spec_json = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(&b.name)
        .bind(&b.description)
        .bind(serde_json::to_string(&b.spec).map_err(bad)?)
        .bind(id)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(ApiError::not_found("experiment not found"));
    }
    Ok(Json(experiment(&state, id).await?))
}

pub async fn delete_experiment(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    if running(&state, id).await? {
        return Err(ApiError::conflict(
            "the experiment is running",
            "Abort or wait for the run first.",
        ));
    }
    let n = crate::db::query("DELETE FROM chaos_experiments WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(ApiError::not_found("experiment not found"));
    }
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBody {
    /// The experiment's name, typed to confirm.
    confirm: String,
}

pub async fn run_experiment(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(b): Json<RunBody>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let e = experiment(&state, id).await?;
    if b.confirm != e.name {
        return Err(
            bad("type the experiment's name to confirm the run").with_code("confirm_mismatch")
        );
    }
    let spec: ExperimentSpec = serde_json::from_value(e.spec).map_err(bad)?;
    check_targets(&state, &spec).await?;
    if running(&state, id).await? {
        return Err(ApiError::conflict(
            "the experiment is already running",
            "Wait for it or abort it.",
        ));
    }
    let run = chaos::start(&state, id, &e.name, spec, &actor.username)
        .await
        .map_err(|e| {
            if format!("{e:#}").contains("UNIQUE") {
                ApiError::conflict(
                    "the experiment is already running",
                    "Wait for it or abort it.",
                )
            } else {
                bad(format!("{e:#}"))
            }
        })?;
    Ok(Json(json!({ "run_id": run, "status": "running" })))
}

#[derive(Deserialize)]
pub struct RunsQuery {
    #[serde(default)]
    experiment_id: Option<Uuid>,
}

pub async fn list_runs(
    State(state): State<AppState>,
    Query(q): Query<RunsQuery>,
) -> Result<Json<Vec<Run>>, ApiError> {
    let rows: Vec<Run> = match q.experiment_id {
        Some(e) => {
            crate::db::query_as(&format!(
                "{RUNS} WHERE experiment_id = ? ORDER BY started_at DESC LIMIT 100"
            ))
            .bind(e)
            .fetch_all(&state.pool)
            .await?
        }
        None => {
            crate::db::query_as(&format!("{RUNS} ORDER BY started_at DESC LIMIT 100"))
                .fetch_all(&state.pool)
                .await?
        }
    };
    Ok(Json(rows.into_iter().map(Run::parsed).collect()))
}

pub async fn get_run(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let r: Run = crate::db::query_as(&format!("{RUNS} WHERE id = ?"))
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let r = r.parsed();
    let live = chaos::live(id);
    Ok(Json(json!({ "run": r, "live": live })))
}

pub async fn abort_run(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let status: String = crate::db::query_scalar("SELECT status FROM chaos_runs WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    if status != "running" {
        return Err(ApiError::conflict(
            "the run already ended",
            "Nothing to abort.",
        ));
    }
    if !chaos::abort(id, &format!("aborted by {}", actor.username)) {
        return Err(ApiError::conflict(
            "the run is not active on this controller",
            "It will be marked interrupted when the controller restarts; its faults end at their lease.",
        ));
    }
    Ok(Json(json!({ "aborting": true })))
}

/// Faults active on every online host right now.
pub async fn faults(State(state): State<AppState>) -> Json<Value> {
    let mut items = Vec::new();
    for (h, r) in bpf::fan_out(&state.pool, &Request::VmChaosStatus).await {
        if let Ok(v) = r {
            for f in v["faults"].as_array().into_iter().flatten() {
                let mut f = f.clone();
                f["host"] = json!(h.hostname);
                items.push(f);
            }
        }
    }
    Json(json!({ "items": items }))
}
