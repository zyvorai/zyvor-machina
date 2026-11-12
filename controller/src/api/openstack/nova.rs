// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::flavors::{CreateFlavorBody, FlavorRow};
use crate::api::keypairs::CreateKeypairBody;
use crate::api::vms::{CreateFromTemplateBody, CreateVmBody, VmListQuery, VmRow};
use crate::state::AppState;
use machina_spec::VirtualMachine;

use super::auth::{require_operator, OsActor};
use super::ids::{nova_status, parse_uuid};
use super::OsError;

fn vm_list_query(project: &str) -> VmListQuery {
    VmListQuery {
        tag_key: None,
        tag_value: None,
        project: Some(project.to_string()),
        host_id: None,
        managed: None,
        tag: None,
        source: None,
        status: None,
    }
}

fn flavor_json(f: &FlavorRow) -> Value {
    json!({
        "id": f.id.to_string(),
        "name": f.name,
        "ram": f.memory_mib,
        "disk": f.disk_gib,
        "vcpus": f.vcpus,
        "rxtx_factor": 1.0,
        "OS-FLV-EXT-DATA:ephemeral": 0,
        "swap": "",
        "os-flavor-access:is_public": f.is_public,
        "description": f.description,
        "links": [],
    })
}

fn server_json(vm: &VmRow) -> Value {
    let addrs = match &vm.guest_ip {
        Some(ip) if !ip.is_empty() => json!({
            "private": [{"addr": ip, "version": 4, "OS-EXT-IPS:type": "fixed"}]
        }),
        _ => json!({}),
    };
    json!({
        "id": vm.id.to_string(),
        "name": vm.name,
        "status": nova_status(&vm.observed_state),
        "tenant_id": vm.project.clone().unwrap_or_else(|| "default".into()),
        "user_id": "",
        "metadata": {},
        "hostId": vm.host_id.map(|h| h.to_string()).unwrap_or_default(),
        "image": Value::Null,
        "flavor": {"id": vm.flavor_id.map(|f| f.to_string()).unwrap_or_default()},
        "created": "",
        "updated": "",
        "addresses": addrs,
        "accessIPv4": vm.guest_ip.clone().unwrap_or_default(),
        "accessIPv6": "",
        "links": [],
        "OS-EXT-STS:power_state": if vm.observed_state == "running" { 1 } else { 4 },
        "OS-EXT-STS:vm_state": nova_status(&vm.observed_state).to_lowercase(),
        "key_name": Value::Null,
        "security_groups": [],
    })
}

pub async fn list_flavors(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let Json(rows) = crate::api::flavors::list_flavors(
        State(state),
        Extension(actor.user.clone()),
    )
    .await?;
    let flavors: Vec<Value> = rows
        .iter()
        .map(|f| json!({"id": f.id.to_string(), "name": f.name, "links": []}))
        .collect();
    Ok(Json(json!({ "flavors": flavors })))
}

pub async fn list_flavors_detail(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let Json(rows) = crate::api::flavors::list_flavors(
        State(state),
        Extension(actor.user.clone()),
    )
    .await?;
    Ok(Json(json!({ "flavors": rows.iter().map(flavor_json).collect::<Vec<_>>() })))
}

pub async fn get_flavor(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let Json(row) =
        crate::api::flavors::get_flavor(State(state), Extension(actor.user.clone()), Path(id))
            .await?;
    Ok(Json(json!({ "flavor": flavor_json(&row) })))
}

pub async fn create_flavor(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Json(body): Json<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>), OsError> {
    crate::auth::require_admin(&actor.user).map_err(OsError::from)?;
    let flavor = body
        .get("flavor")
        .cloned()
        .ok_or_else(|| OsError::bad_request("flavor object required"))?;
    let name = flavor
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OsError::bad_request("name required"))?
        .to_string();
    let vcpus = flavor.get("vcpus").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
    let ram = flavor.get("ram").and_then(|v| v.as_i64()).unwrap_or(512);
    let disk = flavor.get("disk").and_then(|v| v.as_i64()).unwrap_or(1);
    let Json(row) = crate::api::flavors::create_flavor(
        State(state),
        Extension(actor.user.clone()),
        Json(CreateFlavorBody {
            name,
            vcpus,
            memory_mib: ram,
            disk_gib: disk,
            description: flavor
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .into(),
            is_public: flavor
                .get("os-flavor-access:is_public")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
        }),
    )
    .await?;
    Ok((
        axum::http::StatusCode::ACCEPTED,
        Json(json!({ "flavor": flavor_json(&row) })),
    ))
}

pub async fn delete_flavor(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, OsError> {
    crate::auth::require_admin(&actor.user).map_err(OsError::from)?;
    let id = parse_uuid(&id)?;
    let _ = crate::api::flavors::delete_flavor(
        State(state),
        Extension(actor.user.clone()),
        Path(id),
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn list_servers(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let Json(rows) = crate::api::vms::list_vms(
        State(state),
        axum::extract::Query(vm_list_query(&actor.project_name)),
    )
    .await?;
    let servers: Vec<Value> = rows
        .iter()
        .map(|v| json!({"id": v.id.to_string(), "name": v.name, "links": []}))
        .collect();
    Ok(Json(json!({ "servers": servers })))
}

pub async fn list_servers_detail(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let Json(rows) = crate::api::vms::list_vms(
        State(state),
        axum::extract::Query(vm_list_query(&actor.project_name)),
    )
    .await?;
    Ok(Json(
        json!({ "servers": rows.iter().map(server_json).collect::<Vec<_>>() }),
    ))
}

pub async fn get_server(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let Json(row) = crate::api::vms::get_vm(State(state), Path(id)).await?;
    Ok(Json(json!({ "server": server_json(&row) })))
}

#[derive(Debug, Deserialize)]
pub struct CreateServerWrapper {
    pub server: CreateServerBody,
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case, dead_code)]
pub struct CreateServerBody {
    pub name: String,
    #[serde(default)]
    pub flavorRef: Option<String>,
    #[serde(default)]
    pub imageRef: Option<String>,
    #[serde(default)]
    pub key_name: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub user_data: Option<String>,
    #[serde(default)]
    pub networks: Option<Vec<Value>>,
    #[serde(default)]
    pub min_count: Option<i64>,
    #[serde(default)]
    pub max_count: Option<i64>,
}

pub async fn create_server(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Json(body): Json<CreateServerWrapper>,
) -> Result<(axum::http::StatusCode, Json<Value>), OsError> {
    require_operator(&actor)?;
    let s = body.server;
    let flavor_id = match s.flavorRef.as_deref() {
        Some(r) => Some(parse_uuid(r)?),
        None => None,
    };
    let network_name = s.networks.as_ref().and_then(|n| {
        n.first().and_then(|n| {
            n.get("uuid")
                .or_else(|| n.get("name"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
    });

    let server_id = if let Some(image) = s.imageRef.clone() {
        let template_ref = if let Ok(uid) = Uuid::parse_str(&image) {
            crate::db::query_scalar::<_, String>("SELECT name FROM templates WHERE id = ?")
                .bind(uid)
                .fetch_optional(&state.pool)
                .await?
                .unwrap_or(image)
        } else {
            image
        };
        let _ = crate::api::vms::create_from_template(
            State(state.clone()),
            Extension(actor.user.clone()),
            Json(CreateFromTemplateBody {
                template_ref,
                name: s.name.clone(),
                memory: "4Gi".into(),
                host_id: None,
                desired_state: "running".into(),
                cloud_init_user: None,
                cloud_init_password: None,
                cloud_init_ssh_pubkey: None,
                key_name: s.key_name.clone(),
                cloud_init_user_data: s.user_data.clone(),
                template_vars: Default::default(),
                flavor_id,
                network: network_name,
                sleep_after_minutes: None,
                preemptible: false,
                preempt_priority: 0,
            }),
        )
        .await?;
        crate::db::query_scalar::<_, Uuid>("SELECT id FROM vms WHERE name = ? ORDER BY created_at DESC LIMIT 1")
            .bind(&s.name)
            .fetch_optional(&state.pool)
            .await?
            .unwrap_or_else(Uuid::new_v4)
    } else {
        let (vcpus, memory_mib, disk_gib) = if let Some(fid) = flavor_id {
            let Json(f) = crate::api::flavors::get_flavor(
                State(state.clone()),
                Extension(actor.user.clone()),
                Path(fid),
            )
            .await?;
            (f.vcpus as u32, f.memory_mib, f.disk_gib)
        } else {
            (1, 1024, 10)
        };
        let mut vm = VirtualMachine::new(&s.name, format!("{memory_mib}Mi"));
        vm.metadata.project = Some(actor.project_name.clone());
        vm.spec.cpu.cores = vcpus.max(1);
        if let Some(vol) = vm.spec.storage.first_mut() {
            vol.size = format!("{disk_gib}Gi");
        }
        if let Some(net) = network_name {
            if let Some(n) = vm.spec.network.first_mut() {
                n.network = net;
            }
        }
        if let Some(ud) = &s.user_data {
            vm.spec.cloud_init = Some(machina_spec::CloudInitSpec {
                user: "ubuntu".into(),
                password: None,
                ssh_pubkey: None,
                user_data: Some(ud.clone()),
            });
        }
        let _ = crate::api::vms::create_vm(
            State(state.clone()),
            Extension(actor.user.clone()),
            Json(CreateVmBody {
                vm,
                host_id: None,
                tags: Vec::new(),
                desired_state: "running".into(),
                atlas_root_disk: false,
                flavor_id,
                atlas_policy: None,
                preemptible: false,
                preempt_priority: 0,
            }),
        )
        .await?;
        crate::db::query_scalar::<_, Uuid>("SELECT id FROM vms WHERE name = ? ORDER BY created_at DESC LIMIT 1")
            .bind(&s.name)
            .fetch_optional(&state.pool)
            .await?
            .unwrap_or_else(Uuid::new_v4)
    };

    Ok((
        axum::http::StatusCode::ACCEPTED,
        Json(json!({
            "server": {
                "id": server_id.to_string(),
                "name": s.name,
                "status": "BUILD",
                "links": [],
                "adminPass": "",
                "security_groups": [],
            }
        })),
    ))
}

pub async fn delete_server(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let _ = crate::api::vms::delete_vm(
        State(state),
        Extension(actor.user.clone()),
        Path(id),
        None,
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn server_action(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<axum::http::StatusCode, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let user = Extension(actor.user.clone());
    if body.get("os-start").is_some() {
        let _ = crate::api::vms::start_vm(State(state), user, Path(id)).await?;
        return Ok(axum::http::StatusCode::ACCEPTED);
    }
    if body.get("os-stop").is_some() {
        let _ = crate::api::vms::stop_vm(State(state), user, Path(id)).await?;
        return Ok(axum::http::StatusCode::ACCEPTED);
    }
    if body.get("reboot").is_some() {
        let _ = crate::api::vms::reboot_vm(State(state), user, Path(id), None).await?;
        return Ok(axum::http::StatusCode::ACCEPTED);
    }
    if body.get("pause").is_some() || body.get("suspend").is_some() {
        let _ = crate::api::vms::pause_vm(State(state), user, Path(id)).await?;
        return Ok(axum::http::StatusCode::ACCEPTED);
    }
    if body.get("unpause").is_some() || body.get("resume").is_some() {
        let _ = crate::api::vms::resume_vm(State(state), user, Path(id)).await?;
        return Ok(axum::http::StatusCode::ACCEPTED);
    }
    Err(OsError::not_implemented(
        "supported actions: os-start, os-stop, reboot, pause, unpause, suspend, resume",
    ))
}

pub async fn list_keypairs(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let Json(rows) =
        crate::api::keypairs::list_keypairs(State(state), Extension(actor.user.clone())).await?;
    let keypairs: Vec<Value> = rows
        .into_iter()
        .map(|k| {
            json!({
                "keypair": {
                    "name": k.name,
                    "public_key": k.public_key,
                    "fingerprint": k.fingerprint,
                    "user_id": "",
                    "type": "ssh",
                }
            })
        })
        .collect();
    Ok(Json(json!({ "keypairs": keypairs })))
}

pub async fn get_keypair(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(name): Path<String>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let Json(rows) =
        crate::api::keypairs::list_keypairs(State(state), Extension(actor.user.clone())).await?;
    let k = rows
        .into_iter()
        .find(|k| k.name == name)
        .ok_or_else(|| OsError::not_found("keypair not found"))?;
    Ok(Json(json!({
        "keypair": {
            "name": k.name,
            "public_key": k.public_key,
            "fingerprint": k.fingerprint,
            "user_id": "",
            "type": "ssh",
        }
    })))
}

pub async fn create_keypair(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Json(body): Json<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>), OsError> {
    require_operator(&actor)?;
    let kp = body
        .get("keypair")
        .cloned()
        .ok_or_else(|| OsError::bad_request("keypair object required"))?;
    let name = kp
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OsError::bad_request("name required"))?
        .to_string();
    let public_key = kp
        .get("public_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OsError::bad_request("public_key required (import only; no private key generation)"))?
        .to_string();
    let Json(row) = crate::api::keypairs::create_keypair(
        State(state),
        Extension(actor.user.clone()),
        Json(CreateKeypairBody {
            name,
            public_key,
            project_id: Some(actor.project_id),
        }),
    )
    .await?;
    Ok((
        axum::http::StatusCode::OK,
        Json(json!({
            "keypair": {
                "name": row.name,
                "public_key": row.public_key,
                "fingerprint": row.fingerprint,
                "user_id": "",
                "type": "ssh",
            }
        })),
    ))
}

pub async fn delete_keypair(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(name): Path<String>,
) -> Result<axum::http::StatusCode, OsError> {
    require_operator(&actor)?;
    let Json(rows) = crate::api::keypairs::list_keypairs(
        State(state.clone()),
        Extension(actor.user.clone()),
    )
    .await?;
    let k = rows
        .into_iter()
        .find(|k| k.name == name)
        .ok_or_else(|| OsError::not_found("keypair not found"))?;
    let _ = crate::api::keypairs::delete_keypair(
        State(state),
        Extension(actor.user.clone()),
        Path(k.id),
    )
    .await?;
    Ok(axum::http::StatusCode::ACCEPTED)
}

pub async fn list_server_groups(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    // Anti-affinity groups are derived from VM tags `anti-affinity:<name>`.
    let Json(rows) = crate::api::vms::list_vms(
        State(state),
        axum::extract::Query(vm_list_query(&actor.project_name)),
    )
    .await?;
    let mut groups: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for vm in rows {
        for tag in vm.tags.0.iter() {
            if let Some(name) = tag.strip_prefix("anti-affinity:") {
                groups
                    .entry(name.to_string())
                    .or_default()
                    .push(vm.id.to_string());
            }
        }
    }
    let server_groups: Vec<Value> = groups
        .into_iter()
        .map(|(name, members)| {
            json!({
                "id": name,
                "name": name,
                "policies": ["anti-affinity"],
                "members": members,
                "project_id": actor.project_id.to_string(),
                "user_id": "",
                "metadata": {},
            })
        })
        .collect();
    Ok(Json(json!({ "server_groups": server_groups })))
}

pub async fn create_server_group(
    State(_state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Json(body): Json<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>), OsError> {
    require_operator(&actor)?;
    let sg = body
        .get("server_group")
        .cloned()
        .ok_or_else(|| OsError::bad_request("server_group object required"))?;
    let name = sg
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OsError::bad_request("name required"))?
        .to_string();
    let policies = sg
        .get("policies")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if !policies.iter().any(|p| p.as_str() == Some("anti-affinity")) {
        return Err(OsError::bad_request(
            "only anti-affinity policy is supported",
        ));
    }
    // Groups materialise when VMs are tagged; return an empty group handle.
    Ok((
        axum::http::StatusCode::OK,
        Json(json!({
            "server_group": {
                "id": name,
                "name": name,
                "policies": ["anti-affinity"],
                "members": [],
                "project_id": actor.project_id.to_string(),
                "user_id": "",
                "metadata": {},
            }
        })),
    ))
}

