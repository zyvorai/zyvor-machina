// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! `/api/v1/elastic-ip-pools` and `/api/v1/elastic-ips`: public addresses a host holds and maps 1:1 to instances (see
//! `agent/src/eip.rs` for what happens on the host).

use std::collections::HashSet;

use axum::extract::{Path, State};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::engine::eip::{next_free, push_host};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PoolRow {
    pub id: Uuid,
    pub name: String,
    pub cidr: String,
    pub host_id: Uuid,
    pub interface: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct EipRow {
    pub id: Uuid,
    pub pool_id: Uuid,
    pub address: String,
    pub vm_id: Option<Uuid>,
    #[sqlx(skip)]
    pub allocation_id: String,
    /// Whether the host confirmed the mapping on the last push (set on associate/disassociate/release).
    #[sqlx(skip)]
    pub applied: Option<bool>,
    #[sqlx(skip)]
    pub apply_error: Option<String>,
}

const EIP_SELECT: &str = "SELECT id, pool_id, address, vm_id FROM elastic_ips";

pub fn allocation_id(id: Uuid) -> String {
    format!("eipalloc-{}", &id.simple().to_string()[..17])
}

fn with_id(mut r: EipRow) -> EipRow {
    r.allocation_id = allocation_id(r.id);
    r
}

#[derive(Debug, Deserialize)]
pub struct CreatePool {
    pub name: String,
    pub cidr: String,
    pub host_id: Uuid,
    pub interface: String,
}

pub(crate) fn validate_pool(b: &CreatePool) -> Result<(), String> {
    machina_spec::validate_name(&b.name).map_err(|e| e.to_string())?;
    let (_, bits) = b.cidr.split_once('/').ok_or("cidr must look like 203.0.113.0/28")?;
    let bits: u32 = bits.parse().map_err(|_| "cidr prefix must be a number")?;
    if !(16..=32).contains(&bits) {
        return Err("a pool must be a /16 to /32".into());
    }
    if b.cidr.split('/').next().and_then(|a| a.parse::<std::net::Ipv4Addr>().ok()).is_none() {
        return Err("cidr must be an IPv4 network".into());
    }
    if !(1..=11).contains(&b.interface.len()) || !b.interface.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-')) {
        return Err("interface must be 1-11 characters (letters, digits, _ . -)".into());
    }
    Ok(())
}

pub async fn create_pool(State(state): State<AppState>, Extension(actor): Extension<AuthUser>, Json(b): Json<CreatePool>) -> Result<Json<PoolRow>, ApiError> {
    require_admin(&actor)?;
    validate_pool(&b).map_err(ApiError::bad_request)?;
    let host: Option<String> = crate::db::query_scalar("SELECT hostname FROM hosts WHERE id = ?").bind(b.host_id).fetch_optional(&state.pool).await?;
    if host.is_none() {
        return Err(ApiError::bad_request("no such host"));
    }
    let id = Uuid::new_v4();
    crate::db::query("INSERT INTO eip_pools (id, name, cidr, host_id, interface) VALUES (?, ?, ?, ?, ?)")
        .bind(id)
        .bind(&b.name)
        .bind(&b.cidr)
        .bind(b.host_id)
        .bind(&b.interface)
        .execute(&state.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(d) if d.is_unique_violation() => ApiError::conflict("a pool with that name exists", "choose another name"),
            other => other.into(),
        })?;
    Ok(Json(PoolRow { id, name: b.name, cidr: b.cidr, host_id: b.host_id, interface: b.interface }))
}

pub async fn list_pools(State(state): State<AppState>, Extension(actor): Extension<AuthUser>) -> Result<Json<Vec<PoolRow>>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(crate::db::query_as("SELECT id, name, cidr, host_id, interface FROM eip_pools ORDER BY name").fetch_all(&state.pool).await?))
}

pub async fn delete_pool(State(state): State<AppState>, Extension(actor): Extension<AuthUser>, Path(id): Path<Uuid>) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    let used: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM elastic_ips WHERE pool_id = ?").bind(id).fetch_one(&state.pool).await?;
    if used > 0 {
        return Err(ApiError::conflict(format!("{used} address(es) are still allocated from this pool"), "release them first"));
    }
    let host: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM eip_pools WHERE id = ?").bind(id).fetch_optional(&state.pool).await?;
    let r = crate::db::query("DELETE FROM eip_pools WHERE id = ?").bind(id).execute(&state.pool).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found("pool not found"));
    }
    // The sync loop only visits hosts that still have a pool, so without this push the last pool's alias would stay on the host.
    if let Some(host) = host {
        if let Err(e) = crate::engine::eip::push_host(&state.pool, host).await {
            tracing::warn!(%host, "elastic ip cleanup after pool delete: {e:#}");
        }
    }
    Ok(Json(serde_json::json!({ "deleted": true })))
}

#[derive(Debug, Deserialize, Default)]
pub struct Allocate {
    /// Pool name; optional when there is exactly one pool.
    #[serde(default)]
    pub pool: Option<String>,
}

pub async fn allocate(State(state): State<AppState>, Extension(actor): Extension<AuthUser>, Json(b): Json<Allocate>) -> Result<Json<EipRow>, ApiError> {
    require_operator(&actor)?;
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let pools: Vec<PoolRow> = match &b.pool {
        Some(n) => crate::db::query_as("SELECT id, name, cidr, host_id, interface FROM eip_pools WHERE name = ?").bind(n).fetch_all(&mut *tx).await?,
        None => crate::db::query_as("SELECT id, name, cidr, host_id, interface FROM eip_pools ORDER BY name").fetch_all(&mut *tx).await?,
    };
    let pool = match pools.as_slice() {
        [] => return Err(ApiError::bad_request("no elastic IP pool is defined")),
        [p] => p,
        _ => return Err(ApiError::bad_request("several pools exist; name one with `pool`")),
    };
    let used: HashSet<String> = crate::db::query_scalar("SELECT address FROM elastic_ips WHERE pool_id = ?").bind(pool.id).fetch_all(&mut *tx).await?.into_iter().collect();
    let address = next_free(&pool.cidr, &used).ok_or_else(|| ApiError::conflict("the pool has no free address", "release one or add a pool"))?;
    let id = Uuid::new_v4();
    crate::db::query("INSERT INTO elastic_ips (id, pool_id, address, allocated_by) VALUES (?, ?, ?, ?)")
        .bind(id)
        .bind(pool.id)
        .bind(&address)
        .bind(&actor.username)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(with_id(EipRow { id, pool_id: pool.id, address, vm_id: None, allocation_id: String::new(), applied: None, apply_error: None })))
}

pub async fn list(State(state): State<AppState>, Extension(actor): Extension<AuthUser>) -> Result<Json<Vec<EipRow>>, ApiError> {
    require_operator(&actor)?;
    let rows: Vec<EipRow> = crate::db::query_as(&format!("{EIP_SELECT} ORDER BY address")).fetch_all(&state.pool).await?;
    Ok(Json(rows.into_iter().map(with_id).collect()))
}

/// An elastic IP by UUID, `eipalloc-…` id or address.
pub(crate) async fn find(pool: &crate::db::DbPool, reference: &str) -> Result<EipRow, ApiError> {
    let row: Option<EipRow> = if let Ok(u) = Uuid::parse_str(reference) {
        crate::db::query_as(&format!("{EIP_SELECT} WHERE id = ?")).bind(u).fetch_optional(pool).await?
    } else if let Some(hex) = reference.strip_prefix("eipalloc-") {
        crate::db::query_as(&format!("{EIP_SELECT} WHERE lower(hex(id)) LIKE ?")).bind(format!("{}%", hex.to_ascii_lowercase())).fetch_optional(pool).await?
    } else {
        crate::db::query_as(&format!("{EIP_SELECT} WHERE address = ?")).bind(reference).fetch_optional(pool).await?
    };
    row.map(with_id).ok_or_else(|| ApiError::not_found("elastic IP not found"))
}

async fn host_of(pool: &crate::db::DbPool, eip: &EipRow) -> Result<Uuid, ApiError> {
    Ok(crate::db::query_scalar("SELECT host_id FROM eip_pools WHERE id = ?").bind(eip.pool_id).fetch_one(pool).await?)
}

async fn applied(state: &AppState, host: Uuid, mut r: EipRow) -> EipRow {
    match push_host(&state.pool, host).await {
        Ok(()) => r.applied = Some(true),
        Err(e) => {
            r.applied = Some(false);
            r.apply_error = Some(format!("{e:#}"));
        }
    }
    r
}

#[derive(Debug, Deserialize)]
pub struct Associate {
    pub vm_id: Uuid,
}

pub async fn associate(State(state): State<AppState>, Extension(actor): Extension<AuthUser>, Path(reference): Path<String>, Json(b): Json<Associate>) -> Result<Json<EipRow>, ApiError> {
    require_operator(&actor)?;
    let eip = find(&state.pool, &reference).await?;
    let host = host_of(&state.pool, &eip).await?;
    let vm: Option<(Option<Uuid>, Option<String>, Option<String>)> =
        crate::db::query_as("SELECT host_id, guest_ip, guest_ips FROM vms WHERE id = ?").bind(b.vm_id).fetch_optional(&state.pool).await?;
    let (vm_host, ip, ips) = vm.ok_or_else(|| ApiError::not_found("instance not found"))?;
    if vm_host != Some(host) {
        return Err(ApiError::conflict("the instance is not on the host that holds this pool", "elastic IPs are host-local"));
    }
    let known = ip.filter(|a| !a.is_empty()).or_else(|| ips.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok()).and_then(|v| v.into_iter().next()));
    if known.is_none() {
        return Err(ApiError::conflict("the instance has no known address yet", "wait for its DHCP lease or guest agent"));
    }
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let current: Option<Uuid> = crate::db::query_scalar("SELECT vm_id FROM elastic_ips WHERE id = ?").bind(eip.id).fetch_one(&mut *tx).await?;
    if current.is_some() && current != Some(b.vm_id) {
        return Err(ApiError::conflict("the address is already associated with another instance", "disassociate it first"));
    }
    let other: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM elastic_ips WHERE vm_id = ? AND id <> ?").bind(b.vm_id).bind(eip.id).fetch_one(&mut *tx).await?;
    if other > 0 {
        return Err(ApiError::conflict("the instance already has an elastic IP", "an instance has at most one"));
    }
    crate::db::query("UPDATE elastic_ips SET vm_id = ?, associated_at = CURRENT_TIMESTAMP WHERE id = ?").bind(b.vm_id).bind(eip.id).execute(&mut *tx).await?;
    tx.commit().await?;
    let row = find(&state.pool, &eip.id.to_string()).await?;
    Ok(Json(applied(&state, host, row).await))
}

pub async fn disassociate(State(state): State<AppState>, Extension(actor): Extension<AuthUser>, Path(reference): Path<String>) -> Result<Json<EipRow>, ApiError> {
    require_operator(&actor)?;
    let eip = find(&state.pool, &reference).await?;
    let host = host_of(&state.pool, &eip).await?;
    crate::db::query("UPDATE elastic_ips SET vm_id = NULL, associated_at = NULL WHERE id = ?").bind(eip.id).execute(&state.pool).await?;
    let row = find(&state.pool, &eip.id.to_string()).await?;
    Ok(Json(applied(&state, host, row).await))
}

pub async fn release(State(state): State<AppState>, Extension(actor): Extension<AuthUser>, Path(reference): Path<String>) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let eip = find(&state.pool, &reference).await?;
    if eip.vm_id.is_some() {
        return Err(ApiError::conflict("the address is still associated with an instance", "disassociate it first"));
    }
    crate::db::query("DELETE FROM elastic_ips WHERE id = ?").bind(eip.id).execute(&state.pool).await?;
    Ok(Json(serde_json::json!({ "released": eip.address })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(name: &str, cidr: &str, iface: &str) -> CreatePool {
        CreatePool { name: name.into(), cidr: cidr.into(), host_id: Uuid::nil(), interface: iface.into() }
    }

    #[test]
    fn pool_validation() {
        assert!(validate_pool(&pool("pub", "203.0.113.0/28", "eno8303")).is_ok());
        assert!(validate_pool(&pool("pub", "203.0.113.0/8", "eno1")).is_err(), "too large");
        assert!(validate_pool(&pool("pub", "203.0.113.0", "eno1")).is_err());
        assert!(validate_pool(&pool("pub", "2001:db8::/32", "eno1")).is_err());
        assert!(validate_pool(&pool("pub", "203.0.113.0/28", "a-very-long-interface")).is_err());
        assert!(validate_pool(&pool("pub", "203.0.113.0/28", "eth0;x")).is_err());
        assert!(validate_pool(&pool("bad name!", "203.0.113.0/28", "eno1")).is_err());
    }

    #[test]
    fn allocation_ids_look_like_eipalloc() {
        let id = Uuid::parse_str("0123456789abcdef0123456789abcdef").unwrap();
        assert_eq!(allocation_id(id), "eipalloc-0123456789abcdef0");
    }
}
