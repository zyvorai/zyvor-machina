// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Off-site backup replication. A completed backup is copied to an S3-compatible `backup_targets`
//! row by the agent on the host that holds it (so the data never passes through the controller),
//! then verified by comparing object sizes with the files on disk. A verified copy can be fetched
//! back to a host, for a restore at another site. This replicates and verifies; it does not fail
//! anything over (see the disaster-recovery page for the manual restore).
//!
//! Copies run in the background; poll `GET /api/v1/backups/{id}/replicas`. A controller restart
//! mid-copy leaves the row `running`: re-run it with `replicate` to retry.

use axum::extract::{Path, Query, State};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;
use crate::tasks::enqueue::write_audit;
use machina_core::backup_replica::S3Dest;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ReplicaRow {
    pub id: Uuid,
    pub backup_id: Uuid,
    pub target_id: Uuid,
    pub remote_uri: String,
    pub bytes: i64,
    pub files: i64,
    pub is_dir: bool,
    /// True when the copy has a SHA-256 manifest; false for copies made before manifests (size-verified only).
    pub checksummed: bool,
    pub manifest_sha256: String,
    pub status: String,
    pub message: String,
    pub verified_at: String,
    pub fetch_status: String,
    pub fetch_path: String,
    pub fetch_message: String,
    pub created_at: String,
    pub updated_at: String,
}

const COLS: &str = "id, backup_id, target_id, remote_uri, bytes, files, is_dir, checksummed, manifest_sha256, status, message, verified_at, fetch_status, fetch_path, fetch_message, created_at, updated_at";

/// What replicating one backup needs, resolved from the database.
struct Job {
    host_id: Uuid,
    local_path: String,
    dest: S3Dest,
    key: String,
}

async fn target_dest(state: &AppState, target_id: Uuid) -> Result<S3Dest, ApiError> {
    let row: Option<(String, serde_json::Value)> =
        crate::db::query_as("SELECT kind, config_json FROM backup_targets WHERE id = ?")
            .bind(target_id)
            .fetch_optional(&state.pool)
            .await?;
    let (kind, cfg) = row.ok_or_else(|| ApiError::not_found("backup target not found"))?;
    if kind != "s3" {
        return Err(ApiError::bad_request(
            "only S3 (or S3-compatible) targets can hold replicas",
        ));
    }
    S3Dest::from_config(&cfg).map_err(ApiError::bad_request)
}

async fn job_for(state: &AppState, backup_id: Uuid, target_id: Uuid) -> Result<Job, ApiError> {
    let rec: Option<(Uuid, String, String)> =
        crate::db::query_as("SELECT vm_id, status, backup_path FROM backup_records WHERE id = ?")
            .bind(backup_id)
            .fetch_optional(&state.pool)
            .await?;
    let (vm_id, status, path) = rec.ok_or_else(|| ApiError::not_found("backup not found"))?;
    if !matches!(status.as_str(), "completed" | "succeeded") {
        return Err(ApiError::bad_request(format!(
            "the backup is {status}, not completed"
        )));
    }
    if !path.starts_with('/') {
        return Err(ApiError::bad_request(
            "this backup has no file on a host (an Atlas backup is replicated by Atlas)",
        ));
    }
    let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(&state.pool)
        .await?
        .flatten();
    let host_id = host_id.ok_or_else(|| ApiError::bad_request("the VM has no host"))?;
    let dest = target_dest(state, target_id).await?;
    let name = std::path::Path::new(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| ApiError::bad_request("the backup path has no file name"))?;
    let key = dest.key_for(name).map_err(ApiError::bad_request)?;
    Ok(Job {
        host_id,
        local_path: path,
        dest,
        key,
    })
}

/// A `running` copy older than this has lost its task (the agent gives up after 4 hours).
const STALE_AFTER_HOURS: i64 = 6;

/// `updated_at` is `YYYY-MM-DD HH:MM:SS` in UTC.
fn is_stale(updated_at: &str, now: chrono::DateTime<chrono::Utc>) -> bool {
    chrono::NaiveDateTime::parse_from_str(updated_at, "%Y-%m-%d %H:%M:%S")
        .map(|t| now.naive_utc() - t > chrono::Duration::hours(STALE_AFTER_HOURS))
        .unwrap_or(false)
}

/// Create or reset the replica row to `running`. A verified row is returned as is.
async fn begin(
    state: &AppState,
    backup_id: Uuid,
    target_id: Uuid,
) -> Result<(ReplicaRow, bool), ApiError> {
    let existing: Option<ReplicaRow> = crate::db::query_as(&format!(
        "SELECT {COLS} FROM backup_replicas WHERE backup_id = ? AND target_id = ?"
    ))
    .bind(backup_id)
    .bind(target_id)
    .fetch_optional(&state.pool)
    .await?;
    match existing {
        Some(r) if r.status == "verified" => Ok((r, false)),
        Some(r) if r.status == "running" && !is_stale(&r.updated_at, chrono::Utc::now()) => {
            Err(ApiError::conflict(
                "a copy of this backup to this target is already running",
                "poll its status",
            ))
        }
        // Failed, or `running` for so long its task must have died with a controller restart.
        Some(r) => {
            crate::db::query(
                "UPDATE backup_replicas SET status = 'running', message = '', updated_at = datetime('now') WHERE id = ?",
            )
            .bind(r.id)
            .execute(&state.pool)
            .await?;
            Ok((reload(state, r.id).await?, true))
        }
        None => {
            let id = Uuid::new_v4();
            crate::db::query(
                "INSERT INTO backup_replicas (id, backup_id, target_id) VALUES (?, ?, ?)",
            )
            .bind(id)
            .bind(backup_id)
            .bind(target_id)
            .execute(&state.pool)
            .await?;
            Ok((reload(state, id).await?, true))
        }
    }
}

async fn reload(state: &AppState, id: Uuid) -> Result<ReplicaRow, ApiError> {
    crate::db::query_as(&format!("SELECT {COLS} FROM backup_replicas WHERE id = ?"))
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("replica not found"))
}

/// The background copy: push, verify, and record the outcome.
async fn run_push(state: AppState, replica_id: Uuid, job: Job) {
    let result = async {
        let (_, addr) =
            crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, job.host_id)
                .await?;
        crate::agent_client::replicate_backup(
            &addr,
            "push",
            &job.local_path,
            &job.dest.bucket,
            &job.key,
            &job.dest.endpoint_url,
        )
        .await
    }
    .await;
    let _ = match result {
        Ok(r) => {
            crate::db::query(
                "UPDATE backup_replicas SET status = 'verified', message = ?, remote_uri = ?, bytes = ?, files = ?,
                        is_dir = ?, checksummed = ?, manifest_sha256 = ?, verified_at = datetime('now'),
                        updated_at = datetime('now') WHERE id = ?",
            )
            .bind(&r.message)
            .bind(&r.remote_uri)
            .bind(r.bytes as i64)
            .bind(r.files as i64)
            .bind(r.is_dir)
            .bind(r.checksummed)
            .bind(&r.manifest_sha256)
            .bind(replica_id)
            .execute(&state.pool)
            .await
        }
        Err(e) => {
            crate::db::query(
                "UPDATE backup_replicas SET status = 'failed', message = ?, updated_at = datetime('now') WHERE id = ?",
            )
            .bind(format!("{e:#}"))
            .bind(replica_id)
            .execute(&state.pool)
            .await
        }
    };
}

#[derive(Debug, Deserialize)]
pub struct ReplicateBody {
    pub target_id: Uuid,
}

/// Copy one backup to a target (in the background).
pub async fn replicate(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(backup_id): Path<Uuid>,
    Json(body): Json<ReplicateBody>,
) -> Result<Json<ReplicaRow>, ApiError> {
    require_operator(&actor)?;
    let job = job_for(&state, backup_id, body.target_id).await?;
    let (row, start) = begin(&state, backup_id, body.target_id).await?;
    if start {
        write_audit(
            &state,
            &actor.username,
            "backup.replicate",
            "backup",
            Some(backup_id),
            serde_json::json!({ "target_id": body.target_id }),
        )
        .await?;
        tokio::spawn(run_push(state.clone(), row.id, job));
    }
    Ok(Json(row))
}

pub async fn list_for_backup(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(backup_id): Path<Uuid>,
) -> Result<Json<Vec<ReplicaRow>>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(
        crate::db::query_as(&format!(
            "SELECT {COLS} FROM backup_replicas WHERE backup_id = ? ORDER BY created_at"
        ))
        .bind(backup_id)
        .fetch_all(&state.pool)
        .await?,
    ))
}

#[derive(Debug, Default, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub target_id: Option<Uuid>,
    #[serde(default)]
    pub status: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<ReplicaRow>>, ApiError> {
    require_operator(&actor)?;
    // Filters are added only when given: an untyped NULL parameter in `? IS NULL` does not
    // prepare on PostgreSQL.
    let sql = list_sql(q.target_id.is_some(), q.status.is_some());
    let mut query = crate::db::query_as::<_, ReplicaRow>(&sql);
    if let Some(t) = q.target_id {
        query = query.bind(t);
    }
    if let Some(st) = q.status {
        query = query.bind(st);
    }
    let rows: Vec<ReplicaRow> = query.fetch_all(&state.pool).await?;
    Ok(Json(rows))
}

/// SQL for the newest completed file backup per VM that has no verified or running copy on a
/// target. Binds, in order: the VM id (only when `with_vm`), then the target id.
fn candidates_sql(with_vm: bool) -> String {
    let vm_clause = if with_vm { " AND b.vm_id = ?" } else { "" };
    format!(
        "SELECT b.id FROM backup_records b
         WHERE b.status IN ('completed', 'succeeded') AND b.backup_path LIKE '/%'{vm_clause}
           AND b.created_at = (SELECT MAX(b2.created_at) FROM backup_records b2
                               WHERE b2.vm_id = b.vm_id AND b2.status IN ('completed', 'succeeded')
                                 AND b2.backup_path LIKE '/%')
           AND NOT EXISTS (SELECT 1 FROM backup_replicas r
                           WHERE r.backup_id = b.id AND r.target_id = ? AND r.status IN ('verified', 'running'))
         ORDER BY b.created_at DESC LIMIT 200"
    )
}

/// SQL for the replica list; filters are added only when given. Binds: target, then status.
fn list_sql(with_target: bool, with_status: bool) -> String {
    let mut sql = format!("SELECT {COLS} FROM backup_replicas WHERE 1 = 1");
    if with_target {
        sql.push_str(" AND target_id = ?");
    }
    if with_status {
        sql.push_str(" AND status = ?");
    }
    sql.push_str(" ORDER BY updated_at DESC LIMIT 200");
    sql
}

#[derive(Debug, Deserialize)]
pub struct RunBody {
    pub target_id: Uuid,
    /// Limit to one VM.
    #[serde(default)]
    pub vm_id: Option<Uuid>,
    /// How many copies to start in this call (default 10, at most 50).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct RunResult {
    pub started: Vec<Uuid>,
    pub skipped: Vec<String>,
}

/// Start copies of each VM's newest completed file backup that has no verified or running copy on
/// the target, at most `limit`. Shared by the `run` call and the background loop.
pub(crate) async fn start_newest(
    state: &AppState,
    target_id: Uuid,
    vm_id: Option<Uuid>,
    limit: usize,
) -> Result<RunResult, ApiError> {
    target_dest(state, target_id).await?;
    let sql = candidates_sql(vm_id.is_some());
    let mut query = crate::db::query_as::<_, (Uuid,)>(&sql);
    if let Some(v) = vm_id {
        query = query.bind(v);
    }
    let candidates: Vec<(Uuid,)> = query.bind(target_id).fetch_all(&state.pool).await?;
    let mut started = Vec::new();
    let mut skipped = Vec::new();
    for (backup_id,) in candidates {
        if started.len() >= limit {
            break;
        }
        match job_for(state, backup_id, target_id).await {
            Ok(job) => match begin(state, backup_id, target_id).await {
                Ok((row, true)) => {
                    started.push(row.id);
                    tokio::spawn(run_push(state.clone(), row.id, job));
                }
                Ok((_, false)) => {}
                Err(e) => skipped.push(format!("{backup_id}: {}", e.message)),
            },
            Err(e) => skipped.push(format!("{backup_id}: {}", e.message)),
        }
    }
    Ok(RunResult { started, skipped })
}

/// Replicate each VM's newest completed backup that has no verified or running copy on the target.
/// Meant for a cron job; set `auto_replicate` on the target to have the controller do it.
pub async fn run(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<RunBody>,
) -> Result<Json<RunResult>, ApiError> {
    require_operator(&actor)?;
    let limit = body.limit.unwrap_or(10).clamp(1, 50) as usize;
    let result = start_newest(&state, body.target_id, body.vm_id, limit).await?;
    write_audit(
        &state,
        &actor.username,
        "backup.replicate_run",
        "backup_target",
        Some(body.target_id),
        serde_json::json!({ "started": result.started.len(), "skipped": result.skipped.len() }),
    )
    .await?;
    Ok(Json(result))
}

/// Never run more than this many copies to one target at once from the background loop.
const AUTO_MAX_RUNNING: i64 = 3;

/// For every S3 target whose config has `"auto_replicate": true`, copy the newest backups that are
/// not yet replicated, keeping at most [`AUTO_MAX_RUNNING`] copies in flight per target.
pub(crate) async fn replicate_automatically(state: &AppState) {
    let targets: Vec<(Uuid, serde_json::Value)> =
        match crate::db::query_as("SELECT id, config_json FROM backup_targets WHERE kind = 's3'")
            .fetch_all(&state.pool)
            .await
        {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("backup replication: listing targets failed: {e}");
                return;
            }
        };
    for (target_id, cfg) in targets {
        if cfg
            .get("auto_replicate")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        {
            continue;
        }
        let running: i64 = crate::db::query_scalar(
            "SELECT COUNT(*) FROM backup_replicas WHERE target_id = ? AND status = 'running'",
        )
        .bind(target_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(AUTO_MAX_RUNNING);
        if running >= AUTO_MAX_RUNNING {
            continue;
        }
        let room = (AUTO_MAX_RUNNING - running) as usize;
        if let Err(e) = start_newest(state, target_id, None, room).await {
            tracing::warn!(target = %target_id, "auto replication failed: {}", e.message);
        }
    }
}

/// Verified copies per target, newest backup first, so [`machina_core::backup_replica::expired`]
/// can keep the first `retain` of each VM. Copies being fetched are left alone.
pub(crate) const PRUNE_CANDIDATES_SQL: &str = "SELECT r.id, b.vm_id
 FROM backup_replicas r JOIN backup_records b ON b.id = r.backup_id
 WHERE r.target_id = ? AND r.status = 'verified' AND r.fetch_status <> 'running'
 ORDER BY b.created_at DESC, r.id";

/// At most this many remote deletions per pass, so a misconfigured `retain` cannot empty a store at once.
const PRUNE_PER_PASS: usize = 5;

/// Remove one replica from the store (through the agent on the VM's host) and forget it.
async fn delete_copy(state: &AppState, dest: &S3Dest, replica_id: Uuid) -> Result<(), String> {
    let (uri, host): (String, Option<Uuid>) = crate::db::query_as(
        "SELECT r.remote_uri, v.host_id FROM backup_replicas r
         JOIN backup_records b ON b.id = r.backup_id JOIN vms v ON v.id = b.vm_id WHERE r.id = ?",
    )
    .bind(replica_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| e.to_string())?;
    // Never delete outside the prefix this target owns.
    if !machina_core::backup_replica::within_dest(dest, &uri) {
        return Err(format!("{uri} is outside the target's prefix"));
    }
    let key = uri
        .strip_prefix(&format!("s3://{}/", dest.bucket))
        .ok_or("the remote URI does not match the target")?
        .to_string();
    let host = host.ok_or("the VM has no host to run the delete from")?;
    let (_, addr) = crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host)
        .await
        .map_err(|e| e.to_string())?;
    crate::agent_client::replicate_backup(
        &addr,
        "delete",
        "",
        &dest.bucket,
        &key,
        &dest.endpoint_url,
    )
    .await
    .map_err(|e| format!("{e:#}"))?;
    crate::db::query("DELETE FROM backup_replicas WHERE id = ?")
        .bind(replica_id)
        .execute(&state.pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// For every S3 target whose config sets `"retain": N` (1 to 365), delete the copies beyond the
/// newest N verified ones per VM. Opt-in: a target without `retain` is never pruned. A copy whose
/// local backup was already removed has lost its row (the row follows the backup) and is not seen
/// here: use a store lifecycle rule for those.
pub(crate) async fn prune_old_copies(state: &AppState) {
    let targets: Vec<(Uuid, serde_json::Value)> =
        match crate::db::query_as("SELECT id, config_json FROM backup_targets WHERE kind = 's3'")
            .fetch_all(&state.pool)
            .await
        {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("backup replication: listing targets failed: {e}");
                return;
            }
        };
    for (target_id, cfg) in targets {
        let Some(retain) = cfg
            .get("retain")
            .and_then(serde_json::Value::as_u64)
            .filter(|n| (1..=365).contains(n))
        else {
            continue;
        };
        let Ok(dest) = S3Dest::from_config(&cfg) else {
            continue;
        };
        let rows: Vec<(Uuid, Uuid)> = match crate::db::query_as(PRUNE_CANDIDATES_SQL)
            .bind(target_id)
            .fetch_all(&state.pool)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(target = %target_id, "retention listing failed: {e}");
                continue;
            }
        };
        for id in machina_core::backup_replica::expired(&rows, retain as usize)
            .into_iter()
            .take(PRUNE_PER_PASS)
        {
            if let Err(e) = delete_copy(state, &dest, id).await {
                tracing::warn!(replica = %id, "removing an old off-site copy failed: {e}");
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct FetchBody {
    /// The host to pull the copy onto (its agent must have the AWS CLI and credentials).
    pub host_id: Uuid,
}

/// Pull a verified copy onto a host, under `<backup dir>/fetched/`, in the background.
pub async fn fetch(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(replica_id): Path<Uuid>,
    Json(body): Json<FetchBody>,
) -> Result<Json<ReplicaRow>, ApiError> {
    require_operator(&actor)?;
    let r = reload(&state, replica_id).await?;
    if r.status != "verified" {
        return Err(ApiError::bad_request("only a verified copy can be fetched"));
    }
    if r.fetch_status == "running" {
        return Err(ApiError::conflict(
            "a fetch is already running",
            "poll its status",
        ));
    }
    let dest = target_dest(&state, r.target_id).await?;
    // The object key is the remote URI minus `s3://<bucket>/`.
    let key = r
        .remote_uri
        .strip_prefix(&format!("s3://{}/", dest.bucket))
        .ok_or_else(|| ApiError::internal("the replica's remote URI does not match its target"))?
        .to_string();
    let name = key.rsplit('/').next().unwrap_or("backup");
    let path = state
        .config
        .backup_dir
        .join("fetched")
        .join(format!("{}-{}", replica_id.simple(), name))
        .to_string_lossy()
        .into_owned();
    crate::db::query(
        "UPDATE backup_replicas SET fetch_status = 'running', fetch_path = ?, fetch_message = '',
                updated_at = datetime('now') WHERE id = ?",
    )
    .bind(&path)
    .bind(replica_id)
    .execute(&state.pool)
    .await?;
    write_audit(
        &state,
        &actor.username,
        "backup.replica_fetch",
        "backup",
        Some(r.backup_id),
        serde_json::json!({ "replica_id": replica_id, "host_id": body.host_id }),
    )
    .await?;
    let st = state.clone();
    let host_id = body.host_id;
    tokio::spawn(async move {
        let result = async {
            let (_, addr) =
                crate::engine::host_os::resolve_agent_addr(&st.pool, &st.config, host_id).await?;
            crate::agent_client::replicate_backup(
                &addr,
                "fetch",
                &path,
                &dest.bucket,
                &key,
                &dest.endpoint_url,
            )
            .await
        }
        .await;
        let _ = match result {
            Ok(res) => {
                crate::db::query(
                    "UPDATE backup_replicas SET fetch_status = 'done', fetch_message = ?, updated_at = datetime('now') WHERE id = ?",
                )
                .bind(res.message)
                .bind(replica_id)
                .execute(&st.pool)
                .await
            }
            Err(e) => {
                crate::db::query(
                    "UPDATE backup_replicas SET fetch_status = 'failed', fetch_message = ?, updated_at = datetime('now') WHERE id = ?",
                )
                .bind(format!("{e:#}"))
                .bind(replica_id)
                .execute(&st.pool)
                .await
            }
        };
    });
    reload(&state, replica_id).await.map(Json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_copy_goes_stale_after_six_hours() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 10, 21)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc();
        assert!(!is_stale("2026-10-21 11:00:00", now));
        assert!(!is_stale("2026-10-21 06:00:01", now));
        assert!(is_stale("2026-10-21 05:59:59", now));
        // An unreadable timestamp is not treated as stale: never restart something we cannot date.
        assert!(!is_stale("", now));
    }

    #[tokio::test]
    async fn the_built_queries_prepare_and_run_on_this_database() {
        let pool = crate::db::testing::pool().await;
        for with_vm in [false, true] {
            let sql = candidates_sql(with_vm);
            let mut q = crate::db::query_as::<_, (Uuid,)>(&sql);
            if with_vm {
                q = q.bind(Uuid::new_v4());
            }
            let rows = q.bind(Uuid::new_v4()).fetch_all(&pool).await.unwrap();
            assert!(rows.is_empty());
        }
        let pruned: Vec<(Uuid, Uuid)> = crate::db::query_as(PRUNE_CANDIDATES_SQL)
            .bind(Uuid::new_v4())
            .fetch_all(&pool)
            .await
            .unwrap();
        assert!(pruned.is_empty());
        for (t, st) in [(false, false), (true, false), (false, true), (true, true)] {
            let sql = list_sql(t, st);
            let mut q = crate::db::query_as::<_, ReplicaRow>(&sql);
            if t {
                q = q.bind(Uuid::new_v4());
            }
            if st {
                q = q.bind("verified".to_string());
            }
            assert!(q.fetch_all(&pool).await.unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn one_replica_per_backup_and_target_and_cascade() {
        let pool = crate::db::testing::pool().await;
        let (vm, backup, target) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        crate::db::query("INSERT INTO vms (id, name, spec_json) VALUES (?, 'vm1', '{}')")
            .bind(vm)
            .execute(&pool)
            .await
            .unwrap();
        crate::db::query("INSERT INTO backup_records (id, vm_id, status, backup_path) VALUES (?, ?, 'completed', '/var/lib/machina/backups/a.qcow2')")
            .bind(backup)
            .bind(vm)
            .execute(&pool)
            .await
            .unwrap();
        crate::db::query("INSERT INTO backup_targets (id, name, kind, config_json) VALUES (?, 'dr', 's3', '{\"bucket\":\"dr-backups\"}')")
            .bind(target)
            .execute(&pool)
            .await
            .unwrap();
        let id = Uuid::new_v4();
        crate::db::query("INSERT INTO backup_replicas (id, backup_id, target_id) VALUES (?, ?, ?)")
            .bind(id)
            .bind(backup)
            .bind(target)
            .execute(&pool)
            .await
            .unwrap();
        let row: ReplicaRow =
            crate::db::query_as(&format!("SELECT {COLS} FROM backup_replicas WHERE id = ?"))
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row.status, "running");
        assert!(!row.is_dir);
        assert_eq!(row.fetch_status, "");
        // The same backup cannot have two rows for one target.
        let dup = crate::db::query(
            "INSERT INTO backup_replicas (id, backup_id, target_id) VALUES (?, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(backup)
        .bind(target)
        .execute(&pool)
        .await;
        assert!(dup.is_err());
        // Deleting the backup record removes its replicas.
        crate::db::query("DELETE FROM backup_records WHERE id = ?")
            .bind(backup)
            .execute(&pool)
            .await
            .unwrap();
        let n: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM backup_replicas")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }
}
