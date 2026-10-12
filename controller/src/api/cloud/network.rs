// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::{access, audit, conflict, invalid};
use crate::{api::ApiError, auth::AuthUser, state::AppState};
use axum::{
    extract::{Path, State},
    Extension, Json,
};
use machina_spec::CloudCidr;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::db::DbConn;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub(crate) struct Vpc {
    pub id: Uuid,
    pub project_id: Uuid,
    pub host_id: Uuid,
    pub name: String,
    pub cidr: String,
}
#[derive(Debug, Serialize, sqlx::FromRow)]
pub(crate) struct Subnet {
    pub id: Uuid,
    pub vpc_id: Uuid,
    pub network_id: Uuid,
    pub name: String,
    pub cidr: String,
    pub status: String,
    pub last_error: String,
    pub nat_enabled: bool,
}
const VPCS: &str = "SELECT id, project_id, host_id, name, cidr FROM cloud_vpcs";
const SUBNETS: &str =
    "SELECT id, vpc_id, network_id, name, cidr, status, last_error, nat_enabled FROM cloud_subnets";

pub(crate) async fn vpc(
    conn: &mut DbConn,
    actor: &AuthUser,
    id: Uuid,
    write: bool,
) -> Result<Vpc, ApiError> {
    let row: Vpc = crate::db::query_as(&format!("{VPCS} WHERE id = ?"))
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    access(conn, actor, row.project_id, write).await?;
    Ok(row)
}
async fn subnet_vpc(
    conn: &mut DbConn,
    actor: &AuthUser,
    id: Uuid,
    write: bool,
) -> Result<(Subnet, Vpc), ApiError> {
    let subnet: Subnet = crate::db::query_as(&format!("{SUBNETS} WHERE id = ?"))
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    let parent = vpc(conn, actor, subnet.vpc_id, write).await?;
    Ok((subnet, parent))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateVpc {
    name: String,
    cidr: String,
    host_id: Uuid,
}
pub async fn create_vpc(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(project): Path<Uuid>,
    Json(body): Json<CreateVpc>,
) -> Result<Json<Vpc>, ApiError> {
    machina_spec::validate_name(&body.name).map_err(invalid)?;
    let cidr: CloudCidr = body.cidr.parse().map_err(invalid)?;
    cidr.validate_private(false).map_err(invalid)?;
    let mut tx = crate::db::begin_write(&state.pool).await?;
    access(&mut tx, &actor, project, true).await?;
    let online: bool =
        crate::db::query_scalar("SELECT EXISTS(SELECT 1 FROM hosts WHERE id = ? AND state = 'online')")
            .bind(body.host_id)
            .fetch_one(&mut *tx)
            .await?;
    if !online {
        return Err(invalid("online host required"));
    }
    let others: Vec<String> = crate::db::query_scalar("SELECT cidr FROM cloud_vpcs WHERE host_id = ?")
        .bind(body.host_id)
        .fetch_all(&mut *tx)
        .await?;
    if others
        .iter()
        .any(|c| c.parse::<CloudCidr>().is_ok_and(|c| c.overlaps(cidr)))
    {
        return Err(conflict("CIDR overlaps another VPC on this host"));
    }
    let duplicate: bool = crate::db::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM cloud_vpcs WHERE project_id = ? AND name = ?)",
    )
    .bind(project)
    .bind(&body.name)
    .fetch_one(&mut *tx)
    .await?;
    if duplicate {
        return Err(conflict("VPC name already exists"));
    }
    let row = Vpc {
        id: Uuid::new_v4(),
        project_id: project,
        host_id: body.host_id,
        name: body.name,
        cidr: body.cidr,
    };
    crate::db::query(
        "INSERT INTO cloud_vpcs (id, project_id, host_id, name, cidr) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(row.id)
    .bind(project)
    .bind(row.host_id)
    .bind(&row.name)
    .bind(&row.cidr)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, &actor, "cloud.vpc.create", row.id).await?;
    tx.commit().await?;
    Ok(Json(row))
}
pub async fn list_vpcs(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(project): Path<Uuid>,
) -> Result<Json<Vec<Vpc>>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    access(&mut conn, &actor, project, false).await?;
    Ok(Json(
        crate::db::query_as(&format!(
            "{VPCS} WHERE project_id = ? ORDER BY name LIMIT 500"
        ))
        .bind(project)
        .fetch_all(&mut *conn)
        .await?,
    ))
}
pub async fn get_vpc(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vpc>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    Ok(Json(vpc(&mut conn, &actor, id, false).await?))
}
pub async fn delete_vpc(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let mut tx = crate::db::begin_write(&state.pool).await?;
    vpc(&mut tx, &actor, id, true).await?;
    let count: i64 = crate::db::query_scalar("SELECT (SELECT COUNT(*) FROM cloud_subnets WHERE vpc_id = ?) + (SELECT COUNT(*) FROM cloud_peerings WHERE requester_id = ? OR accepter_id = ?)").bind(id).bind(id).bind(id).fetch_one(&mut *tx).await?;
    if count > 0 {
        return Err(conflict("VPC has subnet or peering dependencies"));
    }
    crate::db::query("DELETE FROM cloud_vpcs WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, &actor, "cloud.vpc.delete", id).await?;
    tx.commit().await?;
    Ok(Json(json!({"deleted":true})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSubnet {
    name: String,
    cidr: String,
}
pub async fn create_subnet(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateSubnet>,
) -> Result<Json<Value>, ApiError> {
    machina_spec::validate_name(&body.name).map_err(invalid)?;
    let cidr: CloudCidr = body.cidr.parse().map_err(invalid)?;
    cidr.validate_private(true).map_err(invalid)?;
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let parent = vpc(&mut tx, &actor, id, true).await?;
    if !parent
        .cidr
        .parse::<CloudCidr>()
        .map_err(invalid)?
        .contains(cidr)
    {
        return Err(invalid("subnet must fit inside VPC"));
    }
    let existing: Vec<(String, String)> =
        crate::db::query_as("SELECT cidr, name FROM cloud_subnets WHERE vpc_id = ?")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    if existing
        .iter()
        .any(|(c, n)| n == &body.name || c.parse::<CloudCidr>().is_ok_and(|c| c.overlaps(cidr)))
    {
        return Err(conflict("subnet overlaps or name is already used"));
    }
    let cluster: Uuid = crate::db::query_scalar("SELECT cluster_id FROM hosts WHERE id = ?")
        .bind(parent.host_id)
        .fetch_one(&mut *tx)
        .await?;
    let sid = Uuid::new_v4();
    let network = Uuid::new_v4();
    let compact = sid.simple().to_string();
    crate::db::query("INSERT INTO networks (id, cluster_id, name, backend, bridge) VALUES (?, ?, ?, 'cloud-isolated', ?)").bind(network).bind(cluster).bind(format!("mc-{sid}")).bind(format!("mc{}",&compact[..12])).execute(&mut *tx).await?;
    crate::db::query("INSERT INTO cloud_subnets (id,vpc_id,network_id,name,cidr) VALUES (?,?,?,?,?)")
        .bind(sid)
        .bind(id)
        .bind(network)
        .bind(&body.name)
        .bind(&body.cidr)
        .execute(&mut *tx)
        .await?;
    // Durable outbox: insertion shares the resource transaction. The cloud loop
    // republishes pending jobs through the existing worker after a bus outage.
    let task = Uuid::new_v4();
    let payload = json!({"subnet_id":sid,"host_id":parent.host_id});
    crate::db::query("INSERT INTO tasks (id,operation,status,resource_type,resource_id,host_id,payload) VALUES (?, 'cloud.subnet.provision', 'pending', 'cloud_subnet', ?, ?, ?)").bind(task).bind(sid).bind(parent.host_id).bind(&payload).execute(&mut *tx).await?;
    audit(&mut tx, &actor, "cloud.subnet.create", sid).await?;
    tx.commit().await?;
    // Failure is recoverable: the committed outbox row remains pending.
    let _ = state
        .task_bus
        .publish(
            "machina.tasks",
            &crate::tasks::TaskMessage {
                task_id: task,
                operation: "cloud.subnet.provision".into(),
                payload,
            },
        )
        .await;
    Ok(Json(
        json!({"id":sid,"network_id":network,"task_id":task,"status":"pending"}),
    ))
}
pub async fn list_subnets(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Subnet>>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    vpc(&mut conn, &actor, id, false).await?;
    Ok(Json(
        crate::db::query_as(&format!(
            "{SUBNETS} WHERE vpc_id = ? ORDER BY name LIMIT 500"
        ))
        .bind(id)
        .fetch_all(&mut *conn)
        .await?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetNat {
    enabled: bool,
}

/// `PUT /api/v1/cloud/subnets/{id}/nat {enabled}`: let the instances of a private subnet reach the outside through the host
/// (masqueraded behind its uplink address). The host confirms in the response (`applied`, `apply_error`).
pub async fn set_subnet_nat(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetNat>,
) -> Result<Json<Value>, ApiError> {
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let (sub, parent) = subnet_vpc(&mut tx, &actor, id, true).await?;
    if body.enabled && sub.status != "ready" {
        return Err(conflict("the subnet is not ready yet"));
    }
    crate::db::query("UPDATE cloud_subnets SET nat_enabled = ? WHERE id = ?").bind(body.enabled).bind(id).execute(&mut *tx).await?;
    audit(&mut tx, &actor, if body.enabled { "cloud.subnet.nat.enable" } else { "cloud.subnet.nat.disable" }, id).await?;
    tx.commit().await?;
    let (applied, error) = match crate::engine::natgw::push_host(&state.pool, parent.host_id).await {
        Ok(()) => (true, None),
        Err(e) => (false, Some(format!("{e:#}"))),
    };
    Ok(Json(json!({ "subnet_id": id, "nat_enabled": body.enabled, "applied": applied, "apply_error": error })))
}

/// What still depends on a subnet (None = it can be deleted).
pub(crate) fn subnet_delete_blocker(addresses: i64, ports: i64, groups: i64, instances: i64) -> Option<String> {
    let mut parts = Vec::new();
    if addresses > 0 {
        parts.push(format!("{addresses} reserved address(es)"));
    }
    if ports > 0 {
        parts.push(format!("{ports} network interface(s)"));
    }
    if groups > 0 {
        parts.push(format!("{groups} instance group(s)"));
    }
    if instances > 0 {
        parts.push(format!("{instances} instance(s)"));
    }
    (!parts.is_empty()).then(|| format!("the subnet still has {}; remove them first", parts.join(", ")))
}

async fn subnet_dependents(conn: &mut DbConn, id: Uuid, network_id: Uuid, network_name: &str) -> Result<(i64, i64, i64, i64), ApiError> {
    let addresses: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM cloud_ip_allocations WHERE subnet_id = ?").bind(id).fetch_one(&mut *conn).await?;
    let ports: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM ports WHERE network_id = ?").bind(network_id).fetch_one(&mut *conn).await?;
    let groups: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM cloud_instance_groups WHERE subnet_id = ?").bind(id).fetch_one(&mut *conn).await?;
    // Instances whose spec names the libvirt network (created on the subnet directly or through a group).
    let instances: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vms WHERE spec_json LIKE ?")
        .bind(format!("%\"network\":\"{network_name}\"%"))
        .fetch_one(&mut *conn)
        .await?;
    Ok((addresses, ports, groups, instances))
}

/// Delete a subnet and tear down its libvirt network on the owning host. Refused while addresses, interfaces, groups or
/// instances still use it. The libvirt network is removed first; if the host cannot be reached nothing is deleted.
pub async fn delete_subnet(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    let (sub, parent) = subnet_vpc(&mut conn, &actor, id, true).await?;
    let (network_name, agent_addr): (String, String) = crate::db::query_as(
        "SELECT n.name, h.agent_grpc_addr FROM networks n JOIN hosts h ON h.id = ? WHERE n.id = ?",
    )
    .bind(parent.host_id)
    .bind(sub.network_id)
    .fetch_one(&mut *conn)
    .await?;
    let (a, p, g, i) = subnet_dependents(&mut conn, id, sub.network_id, &network_name).await?;
    if let Some(why) = subnet_delete_blocker(a, p, g, i) {
        return Err(conflict(why));
    }
    drop(conn);
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(format!("the subnet's host is not reachable: {e:#}")))?;
    if let Err(e) = crate::agent_client::host_libvirt_invoke(&mut client, "network.delete", &json!({ "name": network_name })).await {
        let msg = format!("{e:#}");
        // A network that is already gone is fine; anything else keeps the subnet so the operator can retry.
        if !msg.to_ascii_lowercase().contains("not found") && !msg.to_ascii_lowercase().contains("no network") {
            return Err(ApiError::internal(format!("could not remove the libvirt network: {msg}")));
        }
    }
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let (a, p, g, i) = subnet_dependents(&mut tx, id, sub.network_id, &network_name).await?;
    if let Some(why) = subnet_delete_blocker(a, p, g, i) {
        return Err(conflict(why));
    }
    crate::db::query("DELETE FROM cloud_subnets WHERE id = ?").bind(id).execute(&mut *tx).await?;
    crate::db::query("DELETE FROM networks WHERE id = ?").bind(sub.network_id).execute(&mut *tx).await?;
    audit(&mut tx, &actor, "cloud.subnet.delete", id).await?;
    tx.commit().await?;
    Ok(Json(json!({"deleted":true})))
}

#[cfg(test)]
mod subnet_delete_tests {
    use super::subnet_delete_blocker;

    #[test]
    fn lists_what_still_uses_the_subnet() {
        assert!(subnet_delete_blocker(0, 0, 0, 0).is_none());
        let m = subnet_delete_blocker(2, 0, 1, 3).unwrap();
        assert!(m.contains("2 reserved address") && m.contains("1 instance group") && m.contains("3 instance(s)"));
        assert!(!m.contains("network interface"));
    }
}

pub async fn retry_subnet(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let (sub, parent) = subnet_vpc(&mut tx, &actor, id, true).await?;
    let pending: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM tasks WHERE resource_id = ? AND operation = 'cloud.subnet.provision' AND status IN ('pending','running')").bind(id).fetch_one(&mut *tx).await?;
    if pending > 0 || sub.status == "ready" {
        return Err(conflict("subnet is ready or provisioning"));
    }
    crate::db::query("UPDATE cloud_subnets SET status = 'pending', last_error = '' WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let tid = Uuid::new_v4();
    let payload = json!({"subnet_id":id,"host_id":parent.host_id});
    crate::db::query("INSERT INTO tasks (id,operation,status,resource_type,resource_id,host_id,payload) VALUES (?, 'cloud.subnet.provision','pending','cloud_subnet',?,?,?)").bind(tid).bind(id).bind(parent.host_id).bind(&payload).execute(&mut *tx).await?;
    audit(&mut tx, &actor, "cloud.subnet.retry", id).await?;
    tx.commit().await?;
    Ok(Json(json!({"task_id":tid,"status":"pending"})))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct Address {
    id: Uuid,
    subnet_id: Uuid,
    request_key: String,
    address: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Allocate {
    request_key: String,
}
pub async fn allocate_address(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<Allocate>,
) -> Result<Json<Address>, ApiError> {
    if body.request_key.is_empty() || body.request_key.len() > 128 {
        return Err(invalid("request_key must be 1..128 bytes"));
    }
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let (sub, _) = subnet_vpc(&mut tx, &actor, id, true).await?;
    let prior = crate::db::query_as("SELECT id,subnet_id,request_key,address FROM cloud_ip_allocations WHERE subnet_id = ? AND request_key = ?").bind(id).bind(&body.request_key).fetch_optional(&mut *tx).await?;
    if let Some(prior) = prior {
        return Ok(Json(prior));
    }
    let cidr: CloudCidr = sub.cidr.parse().map_err(invalid)?;
    let used: Vec<String> =
        crate::db::query_scalar("SELECT address FROM cloud_ip_allocations WHERE subnet_id = ?")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    let used: std::collections::HashSet<_> = used.into_iter().collect();
    let address = (4..=cidr.ipam_end_offset())
        .filter_map(|n| cidr.address(n).ok())
        .find(|a| !used.contains(a))
        .ok_or_else(|| conflict("managed address pool exhausted"))?;
    let row = Address {
        id: Uuid::new_v4(),
        subnet_id: id,
        request_key: body.request_key,
        address,
    };
    crate::db::query(
        "INSERT INTO cloud_ip_allocations (id,subnet_id,request_key,address) VALUES (?,?,?,?)",
    )
    .bind(row.id)
    .bind(id)
    .bind(&row.request_key)
    .bind(&row.address)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, &actor, "cloud.ip.allocate", row.id).await?;
    tx.commit().await?;
    Ok(Json(row))
}
/// First free managed address in `cidr`, or `wanted` when it is a free managed address of the subnet.
pub(crate) fn pick_address(
    cidr: &CloudCidr,
    used: &std::collections::HashSet<String>,
    wanted: Option<&str>,
) -> Result<String, String> {
    let managed = || (4..=cidr.ipam_end_offset()).filter_map(|n| cidr.address(n).ok());
    if let Some(w) = wanted {
        let ip: std::net::Ipv4Addr = w.trim().parse().map_err(|_| format!("'{w}' is not an IPv4 address"))?;
        let w = ip.to_string();
        if !managed().any(|a| a == w) {
            return Err(format!("{w} is not in the subnet's managed address range"));
        }
        if used.contains(&w) {
            return Err(format!("{w} is already reserved"));
        }
        return Ok(w);
    }
    managed()
        .find(|a| !used.contains(a))
        .ok_or_else(|| "managed address pool exhausted".to_string())
}

/// Reserve an address of `subnet_id` for a network interface (idempotent per `request_key`).
pub(crate) async fn reserve_address(
    pool: &crate::db::DbPool,
    subnet_id: Uuid,
    request_key: &str,
    wanted: Option<&str>,
) -> Result<String, ApiError> {
    let mut tx = crate::db::begin_write(&pool).await?;
    let prior: Option<String> = crate::db::query_scalar(
        "SELECT address FROM cloud_ip_allocations WHERE subnet_id = ? AND request_key = ?",
    )
    .bind(subnet_id)
    .bind(request_key)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(a) = prior {
        return Ok(a);
    }
    let cidr: String = crate::db::query_scalar("SELECT cidr FROM cloud_subnets WHERE id = ?")
        .bind(subnet_id)
        .fetch_one(&mut *tx)
        .await?;
    let cidr: CloudCidr = cidr.parse().map_err(invalid)?;
    let used: std::collections::HashSet<String> =
        crate::db::query_scalar("SELECT address FROM cloud_ip_allocations WHERE subnet_id = ?")
            .bind(subnet_id)
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .collect();
    let address = pick_address(&cidr, &used, wanted).map_err(conflict)?;
    crate::db::query("INSERT INTO cloud_ip_allocations (id,subnet_id,request_key,address) VALUES (?,?,?,?)")
        .bind(Uuid::new_v4())
        .bind(subnet_id)
        .bind(request_key)
        .bind(&address)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(address)
}

#[cfg(test)]
mod pick_address_tests {
    use super::*;

    fn cidr() -> CloudCidr {
        "10.9.0.0/24".parse().unwrap()
    }

    #[test]
    fn first_free_skips_used_and_starts_after_the_reserved_offsets() {
        let mut used = std::collections::HashSet::new();
        assert_eq!(pick_address(&cidr(), &used, None).unwrap(), "10.9.0.4");
        used.insert("10.9.0.4".to_string());
        assert_eq!(pick_address(&cidr(), &used, None).unwrap(), "10.9.0.5");
    }

    #[test]
    fn a_wanted_address_must_be_managed_and_free() {
        let mut used = std::collections::HashSet::new();
        assert_eq!(pick_address(&cidr(), &used, Some("10.9.0.20")).unwrap(), "10.9.0.20");
        used.insert("10.9.0.20".to_string());
        assert!(pick_address(&cidr(), &used, Some("10.9.0.20")).unwrap_err().contains("already reserved"));
        assert!(pick_address(&cidr(), &used, Some("10.9.0.2")).is_err(), "gateway/reserved offsets are not managed");
        assert!(pick_address(&cidr(), &used, Some("10.9.0.250")).is_err(), "DHCP half is not managed");
        assert!(pick_address(&cidr(), &used, Some("not-an-ip")).is_err());
    }
}

pub async fn list_addresses(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Address>>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    subnet_vpc(&mut conn, &actor, id, false).await?;
    Ok(Json(crate::db::query_as("SELECT id,subnet_id,request_key,address FROM cloud_ip_allocations WHERE subnet_id = ? ORDER BY address LIMIT 500").bind(id).fetch_all(&mut *conn).await?))
}
pub async fn release_address(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((id, allocation)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    let mut tx = crate::db::begin_write(&state.pool).await?;
    subnet_vpc(&mut tx, &actor, id, true).await?;
    let n = crate::db::query("DELETE FROM cloud_ip_allocations WHERE subnet_id = ? AND id = ?")
        .bind(id)
        .bind(allocation)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(ApiError::not_found("allocation not found"));
    }
    audit(&mut tx, &actor, "cloud.ip.release", allocation).await?;
    tx.commit().await?;
    Ok(Json(json!({"deleted":true})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeeringRequest {
    accepter_id: Uuid,
}
#[derive(Serialize, sqlx::FromRow)]
pub struct Peering {
    id: Uuid,
    requester_id: Uuid,
    accepter_id: Uuid,
    status: String,
}
pub async fn create_peering(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<PeeringRequest>,
) -> Result<Json<Peering>, ApiError> {
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let a = vpc(&mut tx, &actor, id, true).await?;
    // Target existence alone is insufficient: both VPCs must be visible to the
    // requester, and acceptance requires write permission on the target.
    let b = vpc(&mut tx, &actor, body.accepter_id, false).await?;
    if id == b.id
        || a.cidr
            .parse::<CloudCidr>()
            .map_err(invalid)?
            .overlaps(b.cidr.parse().map_err(invalid)?)
    {
        return Err(invalid("distinct VPCs with non-overlapping CIDRs required"));
    }
    let exists: bool = crate::db::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_peerings WHERE (requester_id=? AND accepter_id=?) OR (requester_id=? AND accepter_id=?))").bind(id).bind(b.id).bind(b.id).bind(id).fetch_one(&mut *tx).await?;
    if exists {
        return Err(conflict("peering already exists"));
    }
    let row = Peering {
        id: Uuid::new_v4(),
        requester_id: id,
        accepter_id: b.id,
        status: "pending_acceptance".into(),
    };
    crate::db::query("INSERT INTO cloud_peerings (id,requester_id,accepter_id) VALUES (?,?,?)")
        .bind(row.id)
        .bind(id)
        .bind(b.id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, &actor, "cloud.peering.request", row.id).await?;
    tx.commit().await?;
    Ok(Json(row))
}
pub async fn accept_peering(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let accepter: Uuid = crate::db::query_scalar("SELECT accepter_id FROM cloud_peerings WHERE id=?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    vpc(&mut tx, &actor, accepter, true).await?;
    crate::db::query("UPDATE cloud_peerings SET status='planned' WHERE id=?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, &actor, "cloud.peering.accept", id).await?;
    tx.commit().await?;
    Ok(Json(json!({"status":"planned","forwarding_active":false})))
}
pub async fn list_peerings(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Peering>>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    vpc(&mut conn, &actor, id, false).await?;
    Ok(Json(crate::db::query_as("SELECT id,requester_id,accepter_id,status FROM cloud_peerings WHERE requester_id=? OR accepter_id=? LIMIT 500").bind(id).bind(id).fetch_all(&mut *conn).await?))
}

#[derive(Deserialize, Serialize, sqlx::FromRow)]
#[serde(deny_unknown_fields)]
pub struct RouteBody {
    destination: String,
    target: String,
    target_id: Option<Uuid>,
}
#[derive(Serialize, sqlx::FromRow)]
pub struct Route {
    id: Uuid,
    vpc_id: Uuid,
    destination: String,
    target: String,
    target_id: Option<Uuid>,
}
pub async fn create_route(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<RouteBody>,
) -> Result<Json<Value>, ApiError> {
    let destination: CloudCidr = body.destination.parse().map_err(invalid)?;
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let a = vpc(&mut tx, &actor, id, true).await?;
    match body.target.as_str() {
        "blackhole" if body.target_id.is_none() => {}
        "local"
            if body.target_id.is_none()
                && a.cidr
                    .parse::<CloudCidr>()
                    .map_err(invalid)?
                    .contains(destination) => {}
        "peering" => {
            let pid = body
                .target_id
                .ok_or_else(|| invalid("peering target_id required"))?;
            let peer:Peering=crate::db::query_as("SELECT id,requester_id,accepter_id,status FROM cloud_peerings WHERE id=? AND (requester_id=? OR accepter_id=?) AND status='planned'").bind(pid).bind(id).bind(id).fetch_optional(&mut *tx).await?.ok_or_else(||invalid("accepted peering for this VPC required"))?;
            let other = if peer.requester_id == id {
                peer.accepter_id
            } else {
                peer.requester_id
            };
            let cidr: String = crate::db::query_scalar("SELECT cidr FROM cloud_vpcs WHERE id=?")
                .bind(other)
                .fetch_one(&mut *tx)
                .await?;
            if !cidr
                .parse::<CloudCidr>()
                .map_err(invalid)?
                .contains(destination)
            {
                return Err(invalid("destination must fit remote VPC"));
            }
        }
        _ => {
            return Err(invalid(
                "supported targets: local, blackhole, accepted peering",
            ))
        }
    }
    let exists: bool = crate::db::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM cloud_routes WHERE vpc_id=? AND destination=?)",
    )
    .bind(id)
    .bind(&body.destination)
    .fetch_one(&mut *tx)
    .await?;
    if exists {
        return Err(conflict("route destination already exists"));
    }
    let rid = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO cloud_routes (id,vpc_id,destination,target,target_id) VALUES (?,?,?,?,?)",
    )
    .bind(rid)
    .bind(id)
    .bind(&body.destination)
    .bind(&body.target)
    .bind(body.target_id)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, &actor, "cloud.route.create", rid).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"id":rid,"status":"planned","forwarding_active":false}),
    ))
}
pub async fn list_routes(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Route>>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    vpc(&mut conn, &actor, id, false).await?;
    Ok(Json(crate::db::query_as("SELECT id,vpc_id,destination,target,target_id FROM cloud_routes WHERE vpc_id=? ORDER BY destination LIMIT 500").bind(id).fetch_all(&mut *conn).await?))
}
pub async fn delete_route(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((id, rid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    let mut tx = crate::db::begin_write(&state.pool).await?;
    vpc(&mut tx, &actor, id, true).await?;
    if crate::db::query("DELETE FROM cloud_routes WHERE id=? AND vpc_id=?")
        .bind(rid)
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected()
        == 0
    {
        return Err(ApiError::not_found("route not found"));
    }
    audit(&mut tx, &actor, "cloud.route.delete", rid).await?;
    tx.commit().await?;
    Ok(Json(json!({"deleted":true})))
}
pub async fn plan(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let Json(vpc) = get_vpc(State(state.clone()), Extension(actor.clone()), Path(id)).await?;
    let Json(subnets) =
        list_subnets(State(state.clone()), Extension(actor.clone()), Path(id)).await?;
    let Json(routes) =
        list_routes(State(state.clone()), Extension(actor.clone()), Path(id)).await?;
    let Json(peerings) = list_peerings(State(state), Extension(actor), Path(id)).await?;
    Ok(Json(
        json!({"vpc":vpc,"subnets":subnets,"routes":routes,"peerings":peerings,"forwarding_active":false,"backend":"host-local-isolated","warnings":["Subnets are separate libvirt isolated networks. Cross-subnet routes and peerings are plans only.","Internet/NAT/transit gateways, private endpoints and IPv6 are not implemented by this backend.","IPAM reservations do not configure guest addresses; use the reserved address in guest configuration."]}),
    ))
}
