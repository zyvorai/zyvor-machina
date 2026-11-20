// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Elastic IP actions on `POST /ec2`. Allocation ids are the ones `elastic_ips::allocation_id`
//! already returns. An association id is `eipassoc-` plus the same 17 hex digits: the address
//! row is the association, and disassociate looks the row up either way.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use uuid::Uuid;

use crate::api::elastic_ips::{self, allocation_id};
use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

use super::more::api_err;
use super::{indexed, xml_escape, Ec2Error};

type Params = BTreeMap<String, String>;

fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

pub fn association_id(id: Uuid) -> String {
    format!("eipassoc-{}", &id.simple().to_string()[..17])
}

fn address_item(address: &str, id: Uuid, vm: Option<Uuid>, private_ip: Option<&str>) -> String {
    let alloc = allocation_id(id);
    let assoc = vm.map(|_| format!("<associationId>{}</associationId>", association_id(id))).unwrap_or_default();
    let instance = vm.map(|v| format!("<instanceId>{}</instanceId>", ec2_id(Kind::Vm, v))).unwrap_or_default();
    let private_ip = private_ip.map(|a| format!("<privateIpAddress>{}</privateIpAddress>", xml_escape(a))).unwrap_or_default();
    format!(
        "<item><publicIp>{}</publicIp><allocationId>{alloc}</allocationId>{assoc}<domain>vpc</domain>{instance}{private_ip}</item>",
        xml_escape(address)
    )
}

pub async fn describe_addresses(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    let Json(rows) = elastic_ips::list(State(state.clone()), Extension(actor.clone())).await.map_err(api_err)?;
    let wanted_alloc = indexed(p, "AllocationId");
    let wanted_ip = indexed(p, "PublicIp");
    let mut items: Vec<(String, String)> = Vec::new();
    for row in rows {
        let alloc = allocation_id(row.id);
        if !wanted_alloc.is_empty() && !wanted_alloc.contains(&alloc) {
            continue;
        }
        if !wanted_ip.is_empty() && !wanted_ip.iter().any(|ip| ip == &row.address) {
            continue;
        }
        let private_ip = match row.vm_id {
            Some(vm) => crate::db::query_scalar::<_, Option<String>>("SELECT guest_ip FROM vms WHERE id = ?")
                .bind(vm)
                .fetch_optional(&state.pool)
                .await?
                .flatten(),
            None => None,
        };
        items.push((alloc, address_item(&row.address, row.id, row.vm_id, private_ip.as_deref())));
    }
    items.sort_by(|a, b| a.0.cmp(&b.0));
    let limit = super::page::max_results(p.get("MaxResults").map(String::as_str)).map_err(|m| bad("InvalidParameterValue", m))?;
    let ids: Vec<String> = items.iter().map(|(id, _)| id.clone()).collect();
    let (slice, token) = super::page::page(&ids, p.get("NextToken").map(String::as_str), limit).map_err(|m| bad("InvalidParameterValue", m))?;
    let body: String = items.iter().filter(|(id, _)| slice.contains(id)).map(|(_, xml)| xml.clone()).collect();
    Ok(format!("<addressesSet>{body}</addressesSet>{}", super::page::token_xml(&token)))
}

pub async fn allocate_address(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    match p.get("Domain").map(String::as_str) {
        None | Some("vpc") => {}
        Some(_) => return Err(bad("UnsupportedOperation", "only Domain=vpc is supported")),
    }
    let pool = p.get("PoolName").cloned().or_else(|| p.get("Pool").cloned());
    let body = elastic_ips::Allocate { pool };
    let Json(row) = elastic_ips::allocate(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(api_err)?;
    if let Some(want) = p.get("Address") {
        if want != &row.address {
            let _ = elastic_ips::release(State(state.clone()), Extension(actor.clone()), Path(row.allocation_id.clone())).await;
            return Err(bad("InvalidAddress.NotFound", "the pool's next free address is not the Address you asked for"));
        }
    }
    Ok(format!(
        "<publicIp>{}</publicIp><allocationId>{}</allocationId><domain>vpc</domain>",
        xml_escape(&row.address),
        xml_escape(&row.allocation_id)
    ))
}

pub async fn associate_address(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let alloc = p.get("AllocationId").cloned().ok_or_else(|| bad("MissingParameter", "The request must contain the parameter AllocationId"))?;
    let instance = p.get("InstanceId").cloned().ok_or_else(|| bad("MissingParameter", "The request must contain the parameter InstanceId"))?;
    let vm = super::more::resolve(state, Kind::Vm, &instance, "InvalidInstanceID.NotFound").await?;
    let Json(row) = elastic_ips::associate(
        State(state.clone()),
        Extension(actor.clone()),
        Path(alloc),
        Json(elastic_ips::Associate { vm_id: vm }),
    )
    .await
    .map_err(api_err)?;
    Ok(format!("<return>true</return><associationId>{}</associationId>", association_id(row.id)))
}

pub async fn disassociate_address(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let reference = if let Some(id) = p.get("AssociationId") {
        id.strip_prefix("eipassoc-").unwrap_or(id).to_string()
    } else if let Some(ip) = p.get("PublicIp") {
        ip.clone()
    } else {
        return Err(bad("MissingParameter", "The request must contain AssociationId or PublicIp"));
    };
    let _ = elastic_ips::disassociate(State(state.clone()), Extension(actor.clone()), Path(reference)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}

pub async fn release_address(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let reference = p
        .get("AllocationId")
        .cloned()
        .or_else(|| p.get("PublicIp").cloned())
        .ok_or_else(|| bad("MissingParameter", "The request must contain AllocationId or PublicIp"))?;
    let _ = elastic_ips::release(State(state.clone()), Extension(actor.clone()), Path(reference)).await.map_err(api_err)?;
    Ok("<return>true</return>".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn association_id_shares_the_allocation_prefix() {
        let id = Uuid::nil();
        assert!(association_id(id).starts_with("eipassoc-"));
        assert_eq!(&association_id(id)[9..], &allocation_id(id)[9..]);
    }
}
