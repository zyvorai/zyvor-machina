// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Migration waves: plan a batch of VMs, start them a few at a time through the Transiva gateway,
//! follow each Transiva job, and keep a manual cutover checklist. Transiva/h2kvm still does the
//! conversion; this adds the plan, the pacing and the record. Nothing runs by itself: a client (or
//! a cron job) calls `POST /api/v1/migration-waves/{id}/advance` to poll and start the next items.

use axum::extract::{Path, State};
use axum::{Extension, Json};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::api::transiva::call;
use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::migration_waves as rules;
use crate::state::AppState;
use crate::tasks::enqueue::write_audit;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct WaveRow {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub planned_at: String,
    pub status: String,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

/// An item as returned to clients. The stored Transiva config is not included: it can name source
/// credentials.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ItemRow {
    pub id: Uuid,
    pub wave_id: Uuid,
    pub position: i64,
    pub name: String,
    pub transiva_job_id: String,
    pub status: String,
    pub phase: String,
    pub progress_percent: i64,
    pub message: String,
    pub cutover: String,
    pub cutover_note: String,
}

#[derive(Debug, Serialize)]
pub struct WaveDetail {
    #[serde(flatten)]
    pub wave: WaveRow,
    pub items: Vec<ItemRow>,
}

const WAVE_COLS: &str =
    "id, name, description, planned_at, status, created_by, created_at, updated_at";
const ITEM_COLS: &str = "id, wave_id, position, name, transiva_job_id, status, phase, progress_percent, message, cutover, cutover_note";

async fn load(state: &AppState, id: Uuid) -> Result<WaveDetail, ApiError> {
    let wave: WaveRow = crate::db::query_as(&format!(
        "SELECT {WAVE_COLS} FROM migration_waves WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("wave not found"))?;
    let items: Vec<ItemRow> = crate::db::query_as(&format!(
        "SELECT {ITEM_COLS} FROM migration_wave_items WHERE wave_id = ? ORDER BY position"
    ))
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    Ok(WaveDetail { wave, items })
}

fn states(items: &[ItemRow]) -> Vec<rules::ItemState> {
    items
        .iter()
        .map(|i| rules::ItemState {
            id: i.id,
            position: i.position,
            status: i.status.clone(),
        })
        .collect()
}

#[derive(Debug, Deserialize)]
pub struct ItemBody {
    pub name: String,
    /// The Transiva migration config for this VM, passed to `/migrations/submit` unchanged.
    pub config: Value,
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// When the wave is planned to run, free text such as an ISO date. Informational.
    #[serde(default)]
    pub planned_at: String,
    pub items: Vec<ItemBody>,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateBody>,
) -> Result<Json<WaveDetail>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    if body.items.is_empty() || body.items.len() > rules::MAX_ITEMS {
        return Err(ApiError::bad_request(format!(
            "a wave needs 1 to {} items",
            rules::MAX_ITEMS
        )));
    }
    if body.description.len() > 2000 || body.planned_at.len() > 64 {
        return Err(ApiError::bad_request(
            "description or planned_at is too long",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    let mut prepared = Vec::with_capacity(body.items.len());
    for it in &body.items {
        rules::validate_item_name(&it.name).map_err(ApiError::bad_request)?;
        if !seen.insert(it.name.trim().to_string()) {
            return Err(ApiError::bad_request(format!(
                "duplicate item '{}'",
                it.name
            )));
        }
        let cfg = rules::validate_config(&it.config).map_err(ApiError::bad_request)?;
        let enc = crate::engine::ai::crypto::store_api_key(&cfg)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        prepared.push((it.name.trim().to_string(), enc));
    }

    let id = Uuid::new_v4();
    let mut tx = state.pool.begin().await?;
    crate::db::query(
        "INSERT INTO migration_waves (id, name, description, planned_at, created_by) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&body.name)
    .bind(&body.description)
    .bind(&body.planned_at)
    .bind(&actor.username)
    .execute(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => {
            ApiError::conflict("a wave with that name exists", "choose another name")
        }
        other => other.into(),
    })?;
    for (pos, (name, enc)) in prepared.iter().enumerate() {
        crate::db::query(
            "INSERT INTO migration_wave_items (id, wave_id, position, name, config_enc) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(pos as i64)
        .bind(name)
        .bind(enc)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    write_audit(
        &state,
        &actor.username,
        "migration_wave.create",
        "migration_wave",
        Some(id),
        serde_json::json!({ "name": body.name, "items": prepared.len() }),
    )
    .await?;
    load(&state, id).await.map(Json)
}

pub async fn list(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<WaveRow>>, ApiError> {
    require_operator(&actor)?;
    let rows: Vec<WaveRow> = crate::db::query_as(&format!(
        "SELECT {WAVE_COLS} FROM migration_waves ORDER BY created_at DESC LIMIT 500"
    ))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn get(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<WaveDetail>, ApiError> {
    require_operator(&actor)?;
    load(&state, id).await.map(Json)
}

#[derive(Debug, Default, Deserialize)]
pub struct AdvanceBody {
    /// How many items may be in flight at once (1 to 20, default 3).
    #[serde(default)]
    pub max_parallel: Option<u32>,
}

async fn set_item(
    state: &AppState,
    id: Uuid,
    status: &str,
    phase: &str,
    percent: i64,
    message: &str,
) -> Result<(), ApiError> {
    crate::db::query(
        "UPDATE migration_wave_items SET status = ?, phase = ?, progress_percent = ?, message = ?,
                updated_at = datetime('now') WHERE id = ?",
    )
    .bind(status)
    .bind(phase)
    .bind(percent)
    .bind(message)
    .bind(id)
    .execute(&state.pool)
    .await?;
    Ok(())
}

/// Poll the started jobs, then start the next planned items. Safe to call repeatedly.
pub async fn advance(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    body: Option<Json<AdvanceBody>>,
) -> Result<Json<WaveDetail>, ApiError> {
    require_operator(&actor)?;
    let max_parallel = body
        .and_then(|b| b.0.max_parallel)
        .unwrap_or(rules::DEFAULT_PARALLEL);
    if !(1..=rules::MAX_PARALLEL_LIMIT).contains(&max_parallel) {
        return Err(ApiError::bad_request(format!(
            "max_parallel must be 1 to {}",
            rules::MAX_PARALLEL_LIMIT
        )));
    }
    let detail = load(&state, id).await?;
    if detail.wave.status == "cancelled" {
        return Err(ApiError::conflict(
            "the wave is cancelled",
            "create a new wave",
        ));
    }

    // 1. Follow the jobs already started.
    for it in detail.items.iter().filter(|i| {
        matches!(i.status.as_str(), "submitted" | "running") && !i.transiva_job_id.is_empty()
    }) {
        let path = format!("/api/v1/jobs/{}", it.transiva_job_id);
        match call(Method::GET, &path, None).await {
            Ok(Json(job)) => {
                if let Some(v) = rules::job_view(&job) {
                    let status = rules::item_status_from_job(&v.status, &it.status);
                    set_item(&state, it.id, &status, &v.phase, v.percent, &v.error).await?;
                }
            }
            Err(e) => {
                set_item(
                    &state,
                    it.id,
                    &it.status,
                    &it.phase,
                    it.progress_percent,
                    &format!("could not poll Transiva: {}", e.message),
                )
                .await?;
            }
        }
    }

    // 2. Start what fits.
    let current = load(&state, id).await?;
    let to_start = rules::next_to_start(&states(&current.items), max_parallel);
    for item_id in to_start {
        let enc: String =
            crate::db::query_scalar("SELECT config_enc FROM migration_wave_items WHERE id = ?")
                .bind(item_id)
                .fetch_one(&state.pool)
                .await?;
        let started: Result<String, String> = async {
            let cfg_text =
                crate::engine::ai::crypto::load_api_key(&enc).map_err(|e| e.to_string())?;
            let cfg: Value = serde_json::from_str(&cfg_text).map_err(|e| e.to_string())?;
            let resp = call(Method::POST, "/api/v1/migrations/submit", Some(cfg))
                .await
                .map_err(|e| e.message)?;
            rules::job_id_from_submit(&resp.0)
                .ok_or_else(|| "Transiva did not return a usable job id".to_string())
        }
        .await;
        match started {
            Ok(job_id) => {
                crate::db::query(
                    "UPDATE migration_wave_items SET status = 'submitted', transiva_job_id = ?, message = '',
                            updated_at = datetime('now') WHERE id = ?",
                )
                .bind(&job_id)
                .bind(item_id)
                .execute(&state.pool)
                .await?;
            }
            Err(msg) => set_item(&state, item_id, "failed", "", 0, &msg).await?,
        }
    }

    // 3. Wave status.
    let after = load(&state, id).await?;
    let status = rules::wave_status(&after.wave.status, &states(&after.items));
    crate::db::query(
        "UPDATE migration_waves SET status = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(status)
    .bind(id)
    .execute(&state.pool)
    .await?;
    write_audit(
        &state,
        &actor.username,
        "migration_wave.advance",
        "migration_wave",
        Some(id),
        serde_json::json!({ "max_parallel": max_parallel, "status": status }),
    )
    .await?;
    load(&state, id).await.map(Json)
}

/// Stop: cancel the Transiva jobs in flight (best effort) and mark every unfinished item cancelled.
pub async fn cancel(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<WaveDetail>, ApiError> {
    require_operator(&actor)?;
    let detail = load(&state, id).await?;
    for it in detail.items.iter().filter(|i| {
        matches!(i.status.as_str(), "submitted" | "running") && !i.transiva_job_id.is_empty()
    }) {
        let _ = call(
            Method::POST,
            &format!("/api/v1/jobs/{}/cancel", it.transiva_job_id),
            None,
        )
        .await;
    }
    crate::db::query(
        "UPDATE migration_wave_items SET status = 'cancelled', updated_at = datetime('now')
         WHERE wave_id = ? AND status IN ('planned', 'submitted', 'running')",
    )
    .bind(id)
    .execute(&state.pool)
    .await?;
    crate::db::query("UPDATE migration_waves SET status = 'cancelled', updated_at = datetime('now') WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    write_audit(
        &state,
        &actor.username,
        "migration_wave.cancel",
        "migration_wave",
        Some(id),
        serde_json::json!({}),
    )
    .await?;
    load(&state, id).await.map(Json)
}

#[derive(Debug, Deserialize)]
pub struct CutoverBody {
    /// `none`, `ready` (verified, waiting for the maintenance window) or `cut_over` (switched).
    pub state: String,
    #[serde(default)]
    pub note: String,
}

pub async fn cutover(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((id, item_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<CutoverBody>,
) -> Result<Json<WaveDetail>, ApiError> {
    require_operator(&actor)?;
    if body.note.len() > 1000 {
        return Err(ApiError::bad_request("note is too long"));
    }
    let status: String = crate::db::query_scalar(
        "SELECT status FROM migration_wave_items WHERE id = ? AND wave_id = ?",
    )
    .bind(item_id)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("item not found"))?;
    rules::cutover_allowed(&status, &body.state).map_err(ApiError::bad_request)?;
    crate::db::query(
        "UPDATE migration_wave_items SET cutover = ?, cutover_note = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(&body.state)
    .bind(&body.note)
    .bind(item_id)
    .execute(&state.pool)
    .await?;
    write_audit(
        &state,
        &actor.username,
        "migration_wave.cutover",
        "migration_wave",
        Some(id),
        serde_json::json!({ "item": item_id, "state": body.state }),
    )
    .await?;
    load(&state, id).await.map(Json)
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let detail = load(&state, id).await?;
    if detail
        .items
        .iter()
        .any(|i| matches!(i.status.as_str(), "submitted" | "running"))
    {
        return Err(ApiError::conflict(
            "the wave has migrations in flight",
            "cancel it first",
        ));
    }
    crate::db::query("DELETE FROM migration_waves WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    write_audit(
        &state,
        &actor.username,
        "migration_wave.delete",
        "migration_wave",
        Some(id),
        serde_json::json!({ "name": detail.wave.name }),
    )
    .await?;
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn schema_holds_a_wave_with_ordered_items_and_cascades() {
        let pool = crate::db::testing::pool().await;
        let wave = Uuid::new_v4();
        crate::db::query("INSERT INTO migration_waves (id, name) VALUES (?, 'wave-1')")
            .bind(wave)
            .execute(&pool)
            .await
            .unwrap();
        for (pos, name) in [(1_i64, "db-01"), (0, "web-01")] {
            crate::db::query(
                "INSERT INTO migration_wave_items (id, wave_id, position, name) VALUES (?, ?, ?, ?)",
            )
            .bind(Uuid::new_v4())
            .bind(wave)
            .bind(pos)
            .bind(name)
            .execute(&pool)
            .await
            .unwrap();
        }
        let items: Vec<ItemRow> = crate::db::query_as(&format!(
            "SELECT {ITEM_COLS} FROM migration_wave_items WHERE wave_id = ? ORDER BY position"
        ))
        .bind(wave)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
            ["web-01", "db-01"]
        );
        assert_eq!(items[0].status, "planned");
        assert_eq!(items[0].cutover, "none");
        // The same VM twice in one wave is refused.
        let dup = crate::db::query(
            "INSERT INTO migration_wave_items (id, wave_id, position, name) VALUES (?, ?, 5, 'web-01')",
        )
        .bind(Uuid::new_v4())
        .bind(wave)
        .execute(&pool)
        .await;
        assert!(dup.is_err());
        crate::db::query("DELETE FROM migration_waves WHERE id = ?")
            .bind(wave)
            .execute(&pool)
            .await
            .unwrap();
        let n: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM migration_wave_items")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }
}
