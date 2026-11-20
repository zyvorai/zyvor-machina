// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Network interfaces. A port is an ENI (`eni-`). Creating one with `InstanceId` attaches it; an existing one can be attached
//! to and detached from an instance (the REST handlers' NIC attach and detach tasks are awaited). Secondary private
//! addresses are reserved in the subnet's address pool and listed on the interface; like every IPAM reservation they are not
//! configured inside the guest.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

use super::more::{api_err, resolve};
use super::{indexed, xml_escape, Ec2Error};

type Params = BTreeMap<String, String>;

fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

fn need(p: &Params, k: &str) -> Result<String, Ec2Error> {
    p.get(k).cloned().ok_or_else(|| bad("MissingParameter", format!("The request must contain the parameter {k}")))
}

async fn network_for_subnet(state: &AppState, subnet: &str) -> Result<Uuid, Ec2Error> {
    let id = resolve(state, Kind::Subnet, subnet, "InvalidSubnetID.NotFound").await?;
    let network: Option<Uuid> = crate::db::query_scalar("SELECT network_id FROM cloud_subnets WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    network.ok_or_else(|| bad("InvalidSubnetID.NotFound", "the subnet has no network"))
}

pub async fn create_network_interface(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let network_id = if let Some(subnet) = p.get("SubnetId") {
        network_for_subnet(state, subnet).await?
    } else {
        need(p, "NetworkId")?.parse().map_err(|_| bad("InvalidParameterValue", "NetworkId must be a network UUID"))?
    };
    let vm_id = match p.get("InstanceId") {
        Some(id) => Some(resolve(state, Kind::Vm, id, "InvalidInstanceID.NotFound").await?),
        None => None,
    };
    let sg = match p.get("GroupId.1") {
        Some(id) => Some(resolve(state, Kind::SecurityGroup, id, "InvalidGroup.NotFound").await?),
        None => None,
    };
    let body = crate::api::networking::CreatePortBody {
        network_id,
        vm_id,
        security_group_id: sg,
        project_id: p.get("ProjectId").and_then(|s| Uuid::parse_str(s).ok()),
        private_ip: p.get("PrivateIpAddress").cloned(),
        description: p.get("Description").cloned().unwrap_or_default(),
    };
    let Json(row) = crate::api::networking::create_port(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(api_err)?;
    let instance = row.vm_id.map(|id| format!("<attachment><instanceId>{}</instanceId><status>attached</status></attachment>", ec2_id(Kind::Vm, id))).unwrap_or_default();
    Ok(format!(
        "<networkInterface><networkInterfaceId>{}</networkInterfaceId><status>{}</status><macAddress>{}</macAddress><privateIpAddress>{}</privateIpAddress>{instance}</networkInterface>",
        ec2_id(Kind::Port, row.id),
        xml_escape(&row.status),
        xml_escape(row.mac_address.as_deref().unwrap_or("")),
        xml_escape(row.private_ip.as_deref().unwrap_or(""))
    ))
}

pub async fn delete_network_interface(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let id = resolve(state, Kind::Port, &need(p, "NetworkInterfaceId")?, "InvalidNetworkInterfaceID.NotFound").await?;
    let _ = crate::api::networking::delete_port(State(state.clone()), Extension(actor.clone()), Path(id)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}

/// The attachment id shown for an interface that is attached to an instance.
fn attachment_id(port: Uuid) -> String {
    format!("eni-attach-{}", &port.simple().to_string()[..17])
}

pub async fn attach_network_interface(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let Some(raw) = p.get("NetworkInterfaceId") else {
        return create_network_interface(state, actor, p).await;
    };
    let port = resolve(state, Kind::Port, raw, "InvalidNetworkInterfaceID.NotFound").await?;
    let vm = resolve(state, Kind::Vm, &need(p, "InstanceId")?, "InvalidInstanceID.NotFound").await?;
    need(p, "DeviceIndex")?;
    let (network_id, current, private_ip): (Uuid, Option<Uuid>, Option<String>) =
        crate::db::query_as("SELECT network_id, vm_id, private_ip FROM ports WHERE id = ?").bind(port).fetch_one(&state.pool).await?;
    if current.is_some() {
        return Err(bad("InvalidParameterValue", "the network interface is already attached to an instance"));
    }
    let network: String = crate::db::query_scalar("SELECT name FROM networks WHERE id = ?").bind(network_id).fetch_one(&state.pool).await?;
    let task = crate::api::vms::attach_vm_nic(
        State(state.clone()),
        Extension(actor.clone()),
        Path(vm),
        Json(crate::api::vms::AttachNicBody { network: network.clone(), model: "virtio".into() }),
    )
    .await
    .map_err(api_err)?;
    crate::api::volumes::wait_for_task(&state.pool, &task.0.task_id).await.map_err(api_err)?;
    let nics = crate::api::vms::list_vm_nics(State(state.clone()), Path(vm)).await.map_err(api_err)?;
    let mac = nics.0.iter().rev().find(|n| n.network == network).map(|n| n.mac_address.clone());
    let pinned = match (&mac, &private_ip) {
        (Some(mac), Some(ip)) => crate::api::networking::pin_dhcp(state, network_id, mac, ip, true).await.is_ok(),
        _ => false,
    };
    crate::db::query("UPDATE ports SET vm_id = ?, mac_address = ?, status = 'ACTIVE', dhcp_pinned = ? WHERE id = ?")
        .bind(vm)
        .bind(&mac)
        .bind(pinned)
        .bind(port)
        .execute(&state.pool)
        .await?;
    Ok(format!("<attachmentId>{}</attachmentId>", attachment_id(port)))
}

pub async fn detach_network_interface(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let attachment = need(p, "AttachmentId")?;
    let hex = attachment.strip_prefix("eni-attach-").ok_or_else(|| bad("InvalidAttachmentID.Malformed", format!("The attachment id '{attachment}' is not valid")))?;
    let port = resolve(state, Kind::Port, &format!("eni-{hex}"), "InvalidAttachmentID.NotFound").await?;
    let (network_id, vm, mac, private_ip, pinned): (Uuid, Option<Uuid>, Option<String>, Option<String>, bool) =
        crate::db::query_as("SELECT network_id, vm_id, mac_address, private_ip, dhcp_pinned FROM ports WHERE id = ?").bind(port).fetch_one(&state.pool).await?;
    let (Some(vm), Some(mac)) = (vm, mac) else {
        return Err(bad("InvalidAttachmentID.NotFound", "the network interface is not attached"));
    };
    if let (true, Some(ip)) = (pinned, private_ip.as_deref()) {
        if let Err(e) = crate::api::networking::pin_dhcp(state, network_id, &mac, ip, false).await {
            tracing::warn!(%port, "could not unpin the reserved address: {e}");
        }
    }
    let task = crate::api::vms::detach_vm_nic(State(state.clone()), Extension(actor.clone()), Path((vm, mac))).await.map_err(api_err)?;
    crate::api::volumes::wait_for_task(&state.pool, &task.0.task_id).await.map_err(api_err)?;
    crate::db::query("UPDATE ports SET vm_id = NULL, mac_address = NULL, status = 'DOWN', dhcp_pinned = FALSE WHERE id = ?").bind(port).execute(&state.pool).await?;
    Ok("<return>true</return>".into())
}

pub async fn modify_network_interface_attribute(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let port = resolve(state, Kind::Port, &need(p, "NetworkInterfaceId")?, "InvalidNetworkInterfaceID.NotFound").await?;
    if p.keys().any(|k| k.starts_with("Attachment.")) {
        return Err(bad("UnsupportedOperation", "Attachment.* attributes are not supported"));
    }
    if let Some(v) = p.get("SourceDestCheck.Value") {
        if v != "true" {
            return Err(bad("UnsupportedOperation", "source/destination checking cannot be turned off"));
        }
    }
    let groups = indexed(p, "SecurityGroupId");
    if groups.len() > 1 {
        return Err(bad("UnsupportedOperation", "an interface carries one security group"));
    }
    let mut changed = false;
    if let Some(desc) = p.get("Description.Value") {
        if desc.len() > 255 {
            return Err(bad("InvalidParameterValue", "the description is limited to 255 characters"));
        }
        crate::db::query("UPDATE ports SET description = ? WHERE id = ?").bind(desc).bind(port).execute(&state.pool).await?;
        changed = true;
    }
    if let Some(g) = groups.first() {
        let sg = resolve(state, Kind::SecurityGroup, g, "InvalidGroup.NotFound").await?;
        crate::db::query("UPDATE ports SET security_group_id = ? WHERE id = ?").bind(sg).bind(port).execute(&state.pool).await?;
        changed = true;
    }
    if !changed && !p.contains_key("SourceDestCheck.Value") {
        return Err(bad("MissingParameter", "specify Description.Value, SecurityGroupId.1 or SourceDestCheck.Value"));
    }
    Ok("<return>true</return>".into())
}

pub async fn describe_network_interface_attribute(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let port = resolve(state, Kind::Port, &need(p, "NetworkInterfaceId")?, "InvalidNetworkInterfaceID.NotFound").await?;
    let (desc, vm, sg): (Option<String>, Option<Uuid>, Option<Uuid>) =
        crate::db::query_as("SELECT description, vm_id, security_group_id FROM ports WHERE id = ?").bind(port).fetch_one(&state.pool).await?;
    let eid = ec2_id(Kind::Port, port);
    let body = match need(p, "Attribute")?.as_str() {
        "description" => format!("<description><value>{}</value></description>", xml_escape(desc.as_deref().unwrap_or(""))),
        "sourceDestCheck" => "<sourceDestCheck><value>true</value></sourceDestCheck>".to_string(),
        "groupSet" => {
            let g = sg.map(|g| format!("<item><groupId>{}</groupId></item>", ec2_id(Kind::SecurityGroup, g))).unwrap_or_default();
            format!("<groupSet>{g}</groupSet>")
        }
        "attachment" => match vm {
            Some(v) => format!(
                "<attachment><attachmentId>{}</attachmentId><instanceId>{}</instanceId><status>attached</status></attachment>",
                attachment_id(port),
                ec2_id(Kind::Vm, v)
            ),
            None => "<attachment/>".to_string(),
        },
        other => return Err(bad("InvalidParameterValue", format!("The attribute '{other}' is not valid for a network interface"))),
    };
    Ok(format!("<networkInterfaceId>{eid}</networkInterfaceId>{body}"))
}

async fn port_subnet(state: &AppState, port: Uuid) -> Result<(Uuid, Option<String>), Ec2Error> {
    let (subnet, ip): (Option<String>, Option<String>) =
        crate::db::query_as("SELECT subnet_id, private_ip FROM ports WHERE id = ?").bind(port).fetch_one(&state.pool).await?;
    let subnet = subnet.and_then(|s| Uuid::parse_str(&s).ok()).ok_or_else(|| bad("InvalidParameterValue", "the interface is not on a cloud subnet, so it has no managed addresses"))?;
    Ok((subnet, ip))
}

pub async fn assign_private_ip_addresses(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let port = resolve(state, Kind::Port, &need(p, "NetworkInterfaceId")?, "InvalidNetworkInterfaceID.NotFound").await?;
    let (subnet, _) = port_subnet(state, port).await?;
    let wanted = indexed(p, "PrivateIpAddress");
    let count: usize = match p.get("SecondaryPrivateIpAddressCount") {
        Some(c) => c.parse().map_err(|_| bad("InvalidParameterValue", "SecondaryPrivateIpAddressCount must be a number"))?,
        None => 0,
    };
    if wanted.is_empty() && count == 0 {
        return Err(bad("MissingParameter", "specify PrivateIpAddress.N or SecondaryPrivateIpAddressCount"));
    }
    if !wanted.is_empty() && count > 0 {
        return Err(bad("InvalidParameterCombination", "specify either PrivateIpAddress.N or SecondaryPrivateIpAddressCount"));
    }
    if count > 30 {
        return Err(bad("PrivateIpAddressLimitExceeded", "at most 30 secondary addresses per interface"));
    }
    let mut assigned = Vec::new();
    let requests: Vec<Option<String>> = if wanted.is_empty() { vec![None; count] } else { wanted.into_iter().map(Some).collect() };
    for want in requests {
        let key = format!("eni-{port}-{}", Uuid::new_v4().simple());
        let address = crate::api::cloud::reserve_address(&state.pool, subnet, &key, want.as_deref()).await.map_err(api_err)?;
        crate::db::query("INSERT OR IGNORE INTO ec2_eni_ips (port_id, address) VALUES (?, ?)").bind(port).bind(&address).execute(&state.pool).await?;
        assigned.push(address);
    }
    let set: String = assigned.iter().map(|a| format!("<item><privateIpAddress>{}</privateIpAddress></item>", xml_escape(a))).collect();
    Ok(format!(
        "<networkInterfaceId>{}</networkInterfaceId><assignedPrivateIpAddressesSet>{set}</assignedPrivateIpAddressesSet>",
        ec2_id(Kind::Port, port)
    ))
}

pub async fn unassign_private_ip_addresses(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let port = resolve(state, Kind::Port, &need(p, "NetworkInterfaceId")?, "InvalidNetworkInterfaceID.NotFound").await?;
    let (subnet, primary) = port_subnet(state, port).await?;
    let addresses = indexed(p, "PrivateIpAddress");
    if addresses.is_empty() {
        return Err(bad("MissingParameter", "The request must contain the parameter PrivateIpAddress"));
    }
    for a in &addresses {
        if primary.as_deref() == Some(a.as_str()) {
            return Err(bad("InvalidParameterValue", "the primary private address cannot be unassigned"));
        }
        let done = crate::db::query("DELETE FROM ec2_eni_ips WHERE port_id = ? AND address = ?").bind(port).bind(a).execute(&state.pool).await?.rows_affected();
        if done == 0 {
            return Err(bad("InvalidParameterValue", format!("{a} is not a secondary address of this interface")));
        }
        crate::db::query("DELETE FROM cloud_ip_allocations WHERE subnet_id = ? AND address = ? AND request_key LIKE ?")
            .bind(subnet)
            .bind(a)
            .bind(format!("eni-{port}-%"))
            .execute(&state.pool)
            .await?;
    }
    Ok("<return>true</return>".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_ids_follow_the_interface() {
        let port = Uuid::parse_str("0123456789abcdef0123456789abcdef").unwrap();
        assert_eq!(attachment_id(port), "eni-attach-0123456789abcdef0");
    }
}
