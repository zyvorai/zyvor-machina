// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! EC2-style key/value tags on any taggable resource (`/api/v1/tags/{type}/{id}`), plus id resolution
//! (`/api/v1/ids/{ec2_id}`). A resource may be named by UUID or by its EC2-style id (`i-0123456789abcdef0`).
//!
//! Who may tag follows who may change the resource: writes need the operator role, and cloud resources (and anything under
//! enforced project scoping) additionally need membership of the owning project.

use std::collections::BTreeMap;

use axum::extract::{Path, Query, State};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use crate::db::DbConn;
use uuid::Uuid;

use super::cloud;
use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{self, Kind, Lookup};
use crate::state::AppState;
use crate::tasks::enqueue::write_audit;

pub const MAX_TAGS: usize = 50;
const MAX_KEY: usize = 128;
const MAX_VALUE: usize = 256;

pub type TagMap = BTreeMap<String, String>;

/// A tag key is 1–128 characters from a conservative set, and may not use a reserved namespace.
pub fn validate_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.chars().count() > MAX_KEY {
        return Err(format!("tag keys are 1–{MAX_KEY} characters"));
    }
    if !key
        .chars()
        .all(|c| c.is_alphanumeric() || " _.:/=+-@".contains(c))
    {
        return Err(format!(
            "tag key '{key}' has characters outside letters, digits and _ . : / = + - @ space"
        ));
    }
    let lower = key.to_ascii_lowercase();
    if lower.starts_with("aws:") || lower.starts_with("machina:") {
        return Err("keys starting with aws: or machina: are reserved".into());
    }
    Ok(())
}

pub fn validate_value(value: &str) -> Result<(), String> {
    if value.chars().count() > MAX_VALUE {
        return Err(format!("tag values are at most {MAX_VALUE} characters"));
    }
    if value.chars().any(|c| c.is_control()) {
        return Err("tag values may not contain control characters".into());
    }
    Ok(())
}

pub fn validate_tags(tags: &TagMap) -> Result<(), String> {
    if tags.is_empty() {
        return Err("no tags given".into());
    }
    for (k, v) in tags {
        validate_key(k)?;
        validate_value(v)?;
    }
    Ok(())
}

fn rid(id: Uuid) -> String {
    id.simple().to_string()
}

pub async fn get_tag_map(
    conn: &mut DbConn,
    kind: Kind,
    id: Uuid,
) -> Result<TagMap, sqlx::Error> {
    let rows: Vec<(String, String)> =
        crate::db::query_as("SELECT key, value FROM resource_tags WHERE resource_type = ? AND resource_id = ? ORDER BY key")
            .bind(kind.type_name())
            .bind(rid(id))
            .fetch_all(conn)
            .await?;
    Ok(rows.into_iter().collect())
}

#[derive(Debug, PartialEq, Eq)]
pub enum PutOutcome {
    Ok(TagMap),
    /// Merging would leave the resource with more than [`MAX_TAGS`] tags.
    TooMany(usize),
}

/// Merge `tags` into the resource's tags (an existing key is overwritten). Run inside a write transaction.
pub async fn put_tag_map(
    conn: &mut DbConn,
    kind: Kind,
    id: Uuid,
    tags: &TagMap,
) -> Result<PutOutcome, sqlx::Error> {
    let mut current = get_tag_map(conn, kind, id).await?;
    for (k, v) in tags {
        current.insert(k.clone(), v.clone());
    }
    if current.len() > MAX_TAGS {
        return Ok(PutOutcome::TooMany(current.len()));
    }
    for (k, v) in tags {
        crate::db::query(
            "INSERT INTO resource_tags (resource_type, resource_id, key, value) VALUES (?, ?, ?, ?)
             ON CONFLICT(resource_type, resource_id, key) DO UPDATE SET value = excluded.value",
        )
        .bind(kind.type_name())
        .bind(rid(id))
        .bind(k)
        .bind(v)
        .execute(&mut *conn)
        .await?;
    }
    Ok(PutOutcome::Ok(current))
}

pub async fn delete_tag_keys(
    conn: &mut DbConn,
    kind: Kind,
    id: Uuid,
    keys: &[String],
) -> Result<TagMap, sqlx::Error> {
    for k in keys {
        crate::db::query(
            "DELETE FROM resource_tags WHERE resource_type = ? AND resource_id = ? AND key = ?",
        )
        .bind(kind.type_name())
        .bind(rid(id))
        .bind(k)
        .execute(&mut *conn)
        .await?;
    }
    get_tag_map(conn, kind, id).await
}

/// The owning cloud project of a resource, if it has one.
async fn project_of(
    conn: &mut DbConn,
    kind: Kind,
    id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    let sql = match kind {
        Kind::Vm => "SELECT p.id FROM vms v JOIN projects p ON p.name = COALESCE(v.project, 'default') WHERE v.id = ?",
        Kind::Subnet => "SELECT v.project_id FROM cloud_subnets s JOIN cloud_vpcs v ON v.id = s.vpc_id WHERE s.id = ?",
        Kind::Snapshot => "SELECT vo.project_id FROM volume_snapshots sn JOIN volumes vo ON vo.id = sn.volume_id WHERE sn.id = ?",
        Kind::Volume => "SELECT project_id FROM volumes WHERE id = ?",
        Kind::SecurityGroup => "SELECT project_id FROM security_groups WHERE id = ?",
        Kind::KeyPair => "SELECT project_id FROM keypairs WHERE id = ?",
        Kind::Port => "SELECT project_id FROM ports WHERE id = ?",
        Kind::Vpc => "SELECT project_id FROM cloud_vpcs WHERE id = ?",
        Kind::InstanceGroup => "SELECT project_id FROM cloud_instance_groups WHERE id = ?",
        Kind::LaunchTemplate => "SELECT project_id FROM cloud_launch_templates WHERE id = ?",
        Kind::InternetGateway => "SELECT COALESCE(g.project_id, v.project_id) FROM ec2_internet_gateways g LEFT JOIN cloud_vpcs v ON v.id = g.vpc_id WHERE g.id = ?",
        Kind::NatGateway => "SELECT v.project_id FROM ec2_nat_gateways n JOIN cloud_vpcs v ON v.id = n.vpc_id WHERE n.id = ?",
        Kind::RouteTable => "SELECT v.project_id FROM ec2_route_tables r JOIN cloud_vpcs v ON v.id = r.vpc_id WHERE r.id = ?",
        Kind::RouteTableAssociation => "SELECT v.project_id FROM ec2_route_table_assocs a JOIN ec2_route_tables r ON r.id = a.route_table_id JOIN cloud_vpcs v ON v.id = r.vpc_id WHERE a.id = ?",
        Kind::NetworkAcl => "SELECT v.project_id FROM ec2_network_acls n JOIN cloud_vpcs v ON v.id = n.vpc_id WHERE n.id = ?",
        Kind::NetworkAclAssociation => "SELECT v.project_id FROM ec2_acl_assocs a JOIN ec2_network_acls n ON n.id = a.acl_id JOIN cloud_vpcs v ON v.id = n.vpc_id WHERE a.id = ?",
        Kind::DhcpOptions => "SELECT project_id FROM ec2_dhcp_options WHERE id = ?",
        Kind::SecurityGroupRule => "SELECT g.project_id FROM security_group_rules r JOIN security_groups g ON g.id = r.security_group_id WHERE r.id = ?",
        Kind::Image | Kind::PlacementGroup | Kind::SpotRequest | Kind::Fleet => return Ok(None),
    };
    Ok(crate::db::query_scalar::<_, Option<Uuid>>(sql)
        .bind(id)
        .fetch_optional(conn)
        .await?
        .flatten())
}

/// Project membership is required for cloud resources always, and for everything else when project scoping is enforced.
async fn authorize(
    state: &AppState,
    actor: &AuthUser,
    kind: Kind,
    id: Uuid,
    write: bool,
) -> Result<(), ApiError> {
    if write {
        require_operator(actor)?;
    }
    let scoped =
        kind.is_cloud() || crate::project_rbac::mode() == crate::project_rbac::Mode::Enforce;
    if !scoped {
        return Ok(());
    }
    let mut conn = state.pool.acquire().await?;
    if let Some(project) = project_of(&mut conn, kind, id).await? {
        cloud::access(&mut conn, actor, project, write).await?;
    }
    Ok(())
}

async fn locate(state: &AppState, resource_type: &str, id: &str) -> Result<(Kind, Uuid), ApiError> {
    let kind = Kind::from_type(resource_type).ok_or_else(|| {
        ApiError::bad_request(format!(
            "unknown resource type '{resource_type}' (use one of: {})",
            resource_ids::ALL
                .iter()
                .map(|k| k.type_name())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })?;
    let mut conn = state.pool.acquire().await?;
    match resource_ids::locate(&mut conn, kind, id).await? {
        Lookup::Found(u) => Ok((kind, u)),
        Lookup::NotFound => Err(ApiError::not_found(format!(
            "{resource_type} '{id}' not found"
        ))),
        Lookup::Ambiguous => Err(ApiError::conflict(
            "that id matches more than one resource",
            "Use the full UUID instead.",
        )),
    }
}

#[derive(Debug, Serialize)]
pub struct TagsResponse {
    pub resource_type: &'static str,
    pub id: Uuid,
    pub ec2_id: String,
    pub tags: TagMap,
}

fn response(kind: Kind, id: Uuid, tags: TagMap) -> Json<TagsResponse> {
    Json(TagsResponse {
        resource_type: kind.type_name(),
        id,
        ec2_id: resource_ids::ec2_id(kind, id),
        tags,
    })
}

pub async fn get_tags(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((resource_type, id)): Path<(String, String)>,
) -> Result<Json<TagsResponse>, ApiError> {
    let (kind, uuid) = locate(&state, &resource_type, &id).await?;
    authorize(&state, &actor, kind, uuid, false).await?;
    let mut conn = state.pool.acquire().await?;
    Ok(response(
        kind,
        uuid,
        get_tag_map(&mut conn, kind, uuid).await?,
    ))
}

#[derive(Debug, Deserialize)]
pub struct PutTagsBody {
    pub tags: TagMap,
}

pub async fn put_tags(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((resource_type, id)): Path<(String, String)>,
    Json(body): Json<PutTagsBody>,
) -> Result<Json<TagsResponse>, ApiError> {
    require_operator(&actor)?;
    validate_tags(&body.tags).map_err(ApiError::bad_request)?;
    let (kind, uuid) = locate(&state, &resource_type, &id).await?;
    authorize(&state, &actor, kind, uuid, true).await?;
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let tags = match put_tag_map(&mut tx, kind, uuid, &body.tags).await? {
        PutOutcome::Ok(t) => t,
        PutOutcome::TooMany(n) => {
            return Err(ApiError::bad_request(format!(
                "that would give the resource {n} tags; the limit is {MAX_TAGS}"
            )));
        }
    };
    tx.commit().await?;
    let _ = write_audit(
        &state,
        &actor.username,
        "tags.put",
        kind.type_name(),
        Some(uuid),
        serde_json::json!({ "keys": body.tags.keys().collect::<Vec<_>>() }),
    )
    .await;
    Ok(response(kind, uuid, tags))
}

#[derive(Debug, Deserialize)]
pub struct DeleteTagsBody {
    pub keys: Vec<String>,
}

pub async fn delete_tags(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((resource_type, id)): Path<(String, String)>,
    Json(body): Json<DeleteTagsBody>,
) -> Result<Json<TagsResponse>, ApiError> {
    require_operator(&actor)?;
    if body.keys.is_empty() || body.keys.len() > MAX_TAGS {
        return Err(ApiError::bad_request(format!(
            "give 1–{MAX_TAGS} keys to delete"
        )));
    }
    let (kind, uuid) = locate(&state, &resource_type, &id).await?;
    authorize(&state, &actor, kind, uuid, true).await?;
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let tags = delete_tag_keys(&mut tx, kind, uuid, &body.keys).await?;
    tx.commit().await?;
    let _ = write_audit(
        &state,
        &actor.username,
        "tags.delete",
        kind.type_name(),
        Some(uuid),
        serde_json::json!({ "keys": body.keys }),
    )
    .await;
    Ok(response(kind, uuid, tags))
}

#[derive(Debug, Deserialize)]
pub struct ListTagsQuery {
    #[serde(default, rename = "type")]
    pub resource_type: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TagRow {
    pub resource_type: String,
    pub id: Uuid,
    pub ec2_id: Option<String>,
    pub key: String,
    pub value: String,
}

/// Find resources by tag (EC2's DescribeTags): optional `type`, `key`, `value` filters. Operator-level read.
pub async fn list_tags(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ListTagsQuery>,
) -> Result<Json<Vec<TagRow>>, ApiError> {
    require_operator(&actor)?;
    let rows: Vec<(String, String, String, String)> = crate::db::query_as(
        "SELECT resource_type, resource_id, key, value FROM resource_tags
         WHERE (?1 IS NULL OR resource_type = ?1) AND (?2 IS NULL OR key = ?2) AND (?3 IS NULL OR value = ?3)
         ORDER BY resource_type, key, value, resource_id LIMIT 1000",
    )
    .bind(q.resource_type.as_deref())
    .bind(q.key.as_deref())
    .bind(q.value.as_deref())
    .fetch_all(&state.pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for (ty, rid_hex, key, value) in rows {
        let Ok(id) = Uuid::parse_str(&rid_hex) else {
            continue;
        };
        let kind = Kind::from_type(&ty);
        out.push(TagRow {
            ec2_id: kind.map(|k| resource_ids::ec2_id(k, id)),
            resource_type: ty,
            id,
            key,
            value,
        });
    }
    Ok(Json(out))
}

#[derive(Debug, Serialize)]
pub struct ResolvedId {
    pub resource_type: &'static str,
    pub id: Uuid,
    pub ec2_id: String,
}

/// Turn an EC2-style id into the resource it names.
pub async fn resolve_ec2_id(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(ec2_id): Path<String>,
) -> Result<Json<ResolvedId>, ApiError> {
    require_operator(&actor)?;
    let (kind, hex) = resource_ids::parse(&ec2_id).ok_or_else(|| {
        ApiError::bad_request("not an EC2-style id (expected e.g. i-0123456789abcdef0)")
    })?;
    let mut conn = state.pool.acquire().await?;
    match resource_ids::resolve(&mut conn, kind, &hex).await? {
        Lookup::Found(id) => Ok(Json(ResolvedId {
            resource_type: kind.type_name(),
            id,
            ec2_id,
        })),
        Lookup::NotFound => Err(ApiError::not_found("no resource has that id")),
        Lookup::Ambiguous => Err(ApiError::conflict(
            "that id matches more than one resource",
            "Use the full UUID instead.",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_values_are_validated() {
        for ok in [
            "Name",
            "env",
            "team/payments",
            "cost-center:42",
            "a b",
            "k=v",
            "ünï",
        ] {
            assert!(validate_key(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "aws:foo",
            "AWS:Foo",
            "machina:x",
            "bad;key",
            "semi;colon",
            &"x".repeat(129),
        ] {
            assert!(validate_key(bad).is_err(), "{bad}");
        }
        assert!(validate_value("").is_ok());
        assert!(validate_value(&"v".repeat(256)).is_ok());
        assert!(validate_value(&"v".repeat(257)).is_err());
        assert!(validate_value("line\nbreak").is_err());
        assert!(validate_tags(&TagMap::new()).is_err());
    }

    #[cfg(feature = "sqlite")]
    async fn conn_with_schema() -> crate::db::DbPool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE resource_tags (resource_type TEXT NOT NULL, resource_id TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, PRIMARY KEY (resource_type, resource_id, key))",
            "CREATE TABLE vms (id TEXT NOT NULL PRIMARY KEY, name TEXT)",
        ] {
            crate::db::query(ddl).execute(&pool).await.unwrap();
        }
        pool
    }

    fn map(pairs: &[(&str, &str)]) -> TagMap {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn put_merges_overwrites_and_delete_removes() {
        let pool = conn_with_schema().await;
        let mut c = pool.acquire().await.unwrap();
        let id = Uuid::new_v4();
        let PutOutcome::Ok(t) = put_tag_map(
            &mut c,
            Kind::Vm,
            id,
            &map(&[("env", "prod"), ("team", "a")]),
        )
        .await
        .unwrap() else {
            panic!()
        };
        assert_eq!(t, map(&[("env", "prod"), ("team", "a")]));
        let PutOutcome::Ok(t) = put_tag_map(
            &mut c,
            Kind::Vm,
            id,
            &map(&[("env", "staging"), ("owner", "x")]),
        )
        .await
        .unwrap() else {
            panic!()
        };
        assert_eq!(t, map(&[("env", "staging"), ("owner", "x"), ("team", "a")]));
        let t = delete_tag_keys(&mut c, Kind::Vm, id, &["team".into(), "missing".into()])
            .await
            .unwrap();
        assert_eq!(t, map(&[("env", "staging"), ("owner", "x")]));
        // other resources and other types are untouched
        assert!(get_tag_map(&mut c, Kind::Vm, Uuid::new_v4())
            .await
            .unwrap()
            .is_empty());
        assert!(get_tag_map(&mut c, Kind::Volume, id)
            .await
            .unwrap()
            .is_empty());
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn the_tag_limit_is_enforced_on_the_merged_total() {
        let pool = conn_with_schema().await;
        let mut c = pool.acquire().await.unwrap();
        let id = Uuid::new_v4();
        let many: TagMap = (0..MAX_TAGS)
            .map(|i| (format!("k{i}"), "v".to_string()))
            .collect();
        assert!(matches!(
            put_tag_map(&mut c, Kind::Vm, id, &many).await.unwrap(),
            PutOutcome::Ok(_)
        ));
        // overwriting an existing key at the limit is fine
        assert!(matches!(
            put_tag_map(&mut c, Kind::Vm, id, &map(&[("k0", "new")]))
                .await
                .unwrap(),
            PutOutcome::Ok(_)
        ));
        // a new key is not
        assert_eq!(
            put_tag_map(&mut c, Kind::Vm, id, &map(&[("extra", "x")]))
                .await
                .unwrap(),
            PutOutcome::TooMany(MAX_TAGS + 1)
        );
        assert_eq!(
            get_tag_map(&mut c, Kind::Vm, id).await.unwrap().len(),
            MAX_TAGS
        );
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn ec2_ids_resolve_back_to_the_resource_and_unknown_ones_do_not() {
        let pool = conn_with_schema().await;
        let mut c = pool.acquire().await.unwrap();
        let id = Uuid::new_v4();
        crate::db::query("INSERT INTO vms (id, name) VALUES (?, 'a')")
            .bind(id)
            .execute(&mut *c)
            .await
            .unwrap();
        let short = resource_ids::ec2_id(Kind::Vm, id);
        assert_eq!(
            resource_ids::locate(&mut c, Kind::Vm, &short)
                .await
                .unwrap(),
            Lookup::Found(id)
        );
        assert_eq!(
            resource_ids::locate(&mut c, Kind::Vm, &id.to_string())
                .await
                .unwrap(),
            Lookup::Found(id)
        );
        // the right digits under the wrong kind's prefix are not a match
        let as_volume = resource_ids::ec2_id(Kind::Volume, id);
        assert_eq!(
            resource_ids::locate(&mut c, Kind::Vm, &as_volume)
                .await
                .unwrap(),
            Lookup::NotFound
        );
        assert_eq!(
            resource_ids::locate(&mut c, Kind::Vm, "i-00000000000000000")
                .await
                .unwrap(),
            Lookup::NotFound
        );
        assert_eq!(
            resource_ids::locate(&mut c, Kind::Vm, &Uuid::new_v4().to_string())
                .await
                .unwrap(),
            Lookup::NotFound
        );
    }
}
