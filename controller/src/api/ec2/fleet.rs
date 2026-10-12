// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Account, zone, and launch-template actions. A zone is a host. A launch template is a
//! `cloud_launch_templates` row (`lt-`); creating one needs `ProjectId` because templates are
//! project-scoped and the EC2 call has no account-default project.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::{Extension, Json};
use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
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

pub fn zone_id(host: Uuid) -> String {
    format!("az-{}", &host.simple().to_string()[..17])
}

pub async fn describe_availability_zones(state: &AppState, _actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    let rows: Vec<(Uuid, String, String, bool)> = crate::db::query_as(
        "SELECT id, hostname, state, COALESCE(maintenance_mode, FALSE) FROM hosts ORDER BY hostname",
    )
    .fetch_all(&state.pool)
    .await?;
    let items: String = rows
        .into_iter()
        .map(|(id, hostname, state, maintenance)| {
            let available = !maintenance && matches!(state.as_str(), "online" | "up" | "ready" | "active");
            let zone_state = if available { "available" } else { "impaired" };
            format!(
                "<item><zoneName>{}</zoneName><zoneId>{}</zoneId><regionName>machina</regionName><zoneState>{zone_state}</zoneState></item>",
                xml_escape(&hostname),
                zone_id(id)
            )
        })
        .collect();
    Ok(format!("<availabilityZoneInfo>{items}</availabilityZoneInfo>"))
}

pub async fn describe_account_attributes(state: &AppState, _actor: &AuthUser, _p: &Params) -> Result<String, Ec2Error> {
    let instances: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vms").fetch_one(&state.pool).await?;
    let addresses: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM elastic_ips").fetch_one(&state.pool).await?;
    let default_vpc: Option<String> = crate::db::query_scalar("SELECT id FROM cloud_vpcs ORDER BY name LIMIT 1")
        .fetch_optional(&state.pool)
        .await?
        .map(|id: Uuid| ec2_id(Kind::Vpc, id));
    Ok(format!(
        "<accountAttributeSet>\
<item><attributeName>supported-platforms</attributeName><attributeValueSet><item><attributeValue>VPC</attributeValue></item></attributeValueSet></item>\
<item><attributeName>used-instances</attributeName><attributeValueSet><item><attributeValue>{instances}</attributeValue></item></attributeValueSet></item>\
<item><attributeName>used-elastic-ips</attributeName><attributeValueSet><item><attributeValue>{addresses}</attributeValue></item></attributeValueSet></item>\
<item><attributeName>default-vpc</attributeName><attributeValueSet><item><attributeValue>{}</attributeValue></item></attributeValueSet></item>\
</accountAttributeSet>",
        xml_escape(default_vpc.as_deref().unwrap_or("none"))
    ))
}

/// The VM a launch template stores: one core, 1 GiB, the default 10 GiB root volume, the image, UEFI, no HA. Built from
/// `VirtualMachine::new` so it carries what the validator requires (a storage volume, the current API version).
pub(super) fn template_vm(name: &str, image: &str) -> serde_json::Value {
    let mut vm = machina_spec::VirtualMachine::new(name, "1Gi");
    vm.spec.template_ref = Some(image.to_string());
    vm.spec.firmware = "uefi".into();
    vm.spec.ha.enabled = false;
    serde_json::to_value(&vm).unwrap_or_default()
}

/// Stores a launch template through the REST handler (project access, validation and audit stay there).
/// Returns `(id, project, name)`.
pub(super) async fn create_template_row(
    state: &AppState,
    actor: &AuthUser,
    project: Uuid,
    name: &str,
    vm: serde_json::Value,
) -> Result<(Uuid, Uuid, String), Ec2Error> {
    let body: crate::api::cloud::elastic::CreateTemplate =
        serde_json::from_value(serde_json::json!({ "name": name, "vm": vm })).map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
    let Json(row) = crate::api::cloud::elastic::create_template(State(state.clone()), Extension(actor.clone()), Path(project), Json(body))
        .await
        .map_err(api_err)?;
    Ok((row.id, row.project_id, row.name))
}

pub async fn delete_launch_template(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let id = super::more::resolve(state, Kind::LaunchTemplate, &need(p, "LaunchTemplateId")?, "InvalidLaunchTemplateId.NotFound").await?;
    let _ = crate::api::cloud::elastic::delete_template(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}

/// Where a `RunInstances` call with `SubnetId` launches: the libvirt network the subnet's `networks` row names
/// (`mc-<subnet uuid>`, the name the host agent defined it under) and the host that VPC lives on.
pub struct SubnetTarget {
    pub network: String,
    pub host_id: Uuid,
}

/// Subnet id → network name and host, for `RunInstances`. `None` when the call did not ask. The subnet's own `name` is
/// only a label (default `subnet-xxxxxxxx`) and is not a libvirt network, so it is never used as the VM's network.
/// A subnet that is not `ready` has no network on its host yet: refuse instead of launching into the void.
pub async fn subnet_for_run(state: &AppState, p: &Params) -> Result<Option<SubnetTarget>, Ec2Error> {
    let Some(subnet) = p.get("SubnetId").filter(|s| !s.is_empty()) else { return Ok(None) };
    let id = super::more::resolve(state, Kind::Subnet, subnet, "InvalidSubnetID.NotFound").await?;
    let row: Option<(String, String, Uuid)> = crate::db::query_as(
        "SELECT n.name, s.status, v.host_id FROM cloud_subnets s JOIN networks n ON n.id = s.network_id JOIN cloud_vpcs v ON v.id = s.vpc_id WHERE s.id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let (network, status, host_id) = row.ok_or_else(|| bad("InvalidSubnetID.NotFound", format!("The subnet '{subnet}' does not exist")))?;
    if status != "ready" {
        return Err(bad("IncorrectState", format!("The subnet '{subnet}' is {status}, not ready: instances can be launched into it once it is available")));
    }
    Ok(Some(SubnetTarget { network, host_id }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_id_is_stable() {
        let id = Uuid::nil();
        assert_eq!(zone_id(id), "az-00000000000000000");
    }

    use crate::auth::AuthUser;
    use axum::extract::{Path, State};
    use axum::{Extension, Json};

    fn admin() -> AuthUser {
        AuthUser { username: "t".into(), role: "admin".into(), auth_source: None }
    }

    /// An online host, a VPC on it and a subnet the way the REST handlers build them (status `pending`).
    async fn subnet_env() -> (AppState, tokio::sync::mpsc::UnboundedReceiver<crate::tasks::TaskMessage>, Uuid, Uuid, Uuid) {
        let (state, rx) = crate::engine::test_support::test_state().await;
        let (cluster, host, project) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        crate::db::query("INSERT INTO clusters(id,name) VALUES (?,'sn-test')").bind(cluster).execute(&state.pool).await.unwrap();
        crate::db::query("INSERT INTO hosts(id,cluster_id,hostname,state) VALUES (?,?,'sn-host','online')").bind(host).bind(cluster).execute(&state.pool).await.unwrap();
        crate::db::query("INSERT INTO projects(id,name) VALUES (?,'sn-proj')").bind(project).execute(&state.pool).await.unwrap();
        let vpc_body: crate::api::cloud::network::CreateVpc =
            serde_json::from_value(serde_json::json!({"name":"private","cidr":"10.40.0.0/16","host_id":host})).unwrap();
        let Json(vpc) = crate::api::cloud::network::create_vpc(State(state.clone()), Extension(admin()), Path(project), Json(vpc_body)).await.unwrap();
        let vpc = serde_json::to_value(&vpc).unwrap()["id"].as_str().and_then(|s| Uuid::parse_str(s).ok()).unwrap();
        let sb: crate::api::cloud::network::CreateSubnet = serde_json::from_value(serde_json::json!({"name":"subnet-1c620ca2","cidr":"10.40.1.0/24"})).unwrap();
        let Json(sub) = crate::api::cloud::network::create_subnet(State(state.clone()), Extension(admin()), Path(vpc), Json(sb)).await.unwrap();
        let sub = sub["id"].as_str().and_then(|s| Uuid::parse_str(s).ok()).unwrap();
        (state, rx, host, vpc, sub)
    }

    fn params(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[tokio::test]
    async fn no_subnet_means_no_target() {
        let (state, _rx, ..) = subnet_env().await;
        assert!(subnet_for_run(&state, &params(&[])).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn ready_subnet_resolves_to_the_mc_network_and_the_vpc_host() {
        let (state, _rx, host, _vpc, sub) = subnet_env().await;
        crate::db::query("UPDATE cloud_subnets SET status = 'ready' WHERE id = ?").bind(sub).execute(&state.pool).await.unwrap();
        let t = subnet_for_run(&state, &params(&[("SubnetId", &ec2_id(Kind::Subnet, sub))])).await.unwrap().unwrap();
        // the libvirt network is mc-<full subnet uuid>, not the subnet's label
        assert_eq!(t.network, format!("mc-{sub}"));
        assert_ne!(t.network, "subnet-1c620ca2");
        assert_eq!(t.host_id, host);
    }

    #[tokio::test]
    async fn subnet_that_is_not_ready_is_refused() {
        let (state, _rx, _host, _vpc, sub) = subnet_env().await;
        let sid = ec2_id(Kind::Subnet, sub);
        let e = subnet_for_run(&state, &params(&[("SubnetId", &sid)])).await.err().unwrap();
        assert_eq!(e.code, "IncorrectState");
        crate::db::query("UPDATE cloud_subnets SET status = 'error' WHERE id = ?").bind(sub).execute(&state.pool).await.unwrap();
        assert_eq!(subnet_for_run(&state, &params(&[("SubnetId", &sid)])).await.err().unwrap().code, "IncorrectState");
    }

    #[tokio::test]
    async fn unknown_subnet_is_not_found() {
        let (state, _rx, ..) = subnet_env().await;
        let e = subnet_for_run(&state, &params(&[("SubnetId", "subnet-00000000000000000")])).await.err().unwrap();
        assert_eq!(e.code, "InvalidSubnetID.NotFound");
    }
}
