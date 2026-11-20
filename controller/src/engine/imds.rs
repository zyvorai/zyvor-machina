// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Pushes instance metadata to each host's agent (`imds.sync`), which serves it at 169.254.169.254 to the guest it belongs to.
//! The leader pushes every 30 s. A host that cannot be reached keeps its previous table until the next push.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{json, Value};
use crate::db::DbPool;
use uuid::Uuid;

use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

const TICK_SECS: u64 = 30;

/// The last field is the instance's metadata endpoint switch (`ModifyInstanceMetadataOptions`).
type Row = (Uuid, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Uuid, bool);

/// One agent entry from an instance row; None when the instance has no known address yet.
pub fn entry(row: &Row) -> Option<Value> {
    let (id, name, spec_json, guest_ip, guest_ips, flavor, project, _host, enabled) = row;
    let mut ips: Vec<String> = guest_ip.iter().filter(|a| !a.is_empty()).cloned().collect();
    for a in guest_ips.as_deref().and_then(|s| serde_json::from_str::<Vec<String>>(s).ok()).unwrap_or_default() {
        let a = a.split('/').next().unwrap_or(&a).to_string();
        if !a.is_empty() && !ips.contains(&a) {
            ips.push(a);
        }
    }
    if ips.is_empty() {
        return None;
    }
    let spec: Value = spec_json.as_deref().and_then(|s| serde_json::from_str(s).ok()).unwrap_or(Value::Null);
    let ci = &spec["spec"]["cloud_init"];
    let key = ci["ssh_pubkey"].as_str().filter(|k| !k.trim().is_empty()).map(|k| vec![k.trim().to_string()]).unwrap_or_default();
    Some(json!({
        "instance_id": ec2_id(Kind::Vm, *id),
        "hostname": name,
        "ips": ips,
        "instance_type": flavor.clone().unwrap_or_default(),
        "public_keys": key,
        "user_data": ci["user_data"].as_str(),
        "project": project.clone().unwrap_or_default(),
        "enabled": enabled,
    }))
}

/// host id → entries for the instances on it.
pub fn group_by_host(rows: &[Row]) -> BTreeMap<Uuid, Vec<Value>> {
    let mut out: BTreeMap<Uuid, Vec<Value>> = BTreeMap::new();
    for r in rows {
        if let Some(e) = entry(r) {
            out.entry(r.7).or_default().push(e);
        }
    }
    out
}

async fn push_all(pool: &DbPool) -> anyhow::Result<()> {
    let rows: Vec<Row> = crate::db::query_as(
        "SELECT v.id, v.name, v.spec_json, v.guest_ip, v.guest_ips, f.name, v.project, v.host_id, COALESCE(a.metadata_endpoint, TRUE) \
         FROM vms v LEFT JOIN flavors f ON f.id = v.flavor_id LEFT JOIN ec2_instance_attrs a ON a.vm_id = v.id \
         WHERE v.host_id IS NOT NULL AND COALESCE(v.inventory_source, 'libvirt') != 'kubevirt'",
    )
    .fetch_all(pool)
    .await?;
    let mut by_host = group_by_host(&rows);
    let hosts: Vec<(Uuid, String)> = crate::db::query_as("SELECT id, agent_grpc_addr FROM hosts WHERE state = 'online'").fetch_all(pool).await?;
    for (id, addr) in hosts {
        // An empty list still goes out: it clears entries of instances that were deleted or lost their address.
        let instances = by_host.remove(&id).unwrap_or_default();
        let payload = json!({ "instances": instances });
        match crate::agent_client::connect(&addr).await {
            Ok(mut client) => {
                if let Err(e) = crate::agent_client::host_libvirt_invoke(&mut client, "imds.sync", &payload).await {
                    tracing::debug!(host = %id, "imds sync: {e:#}");
                }
            }
            Err(e) => tracing::debug!(host = %id, "imds sync: {e:#}"),
        }
    }
    Ok(())
}

pub fn spawn(state: AppState) {
    if std::env::var("MACHINA_IMDS").as_deref() == Ok("0") {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(TICK_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = push_all(&state.pool).await {
                tracing::warn!("imds push: {e:#}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(ip: Option<&str>, ips: Option<&str>, spec: Option<&str>) -> Row {
        (
            Uuid::parse_str("0123456789abcdef0123456789abcdef").unwrap(),
            "web-1".into(),
            spec.map(String::from),
            ip.map(String::from),
            ips.map(String::from),
            Some("small".into()),
            Some("default".into()),
            Uuid::nil(),
            true,
        )
    }

    #[test]
    fn an_instance_without_an_address_gets_no_entry() {
        assert!(entry(&row(None, None, None)).is_none());
        assert!(entry(&row(Some(""), Some("[]"), None)).is_none());
    }

    #[test]
    fn addresses_keys_and_user_data_come_from_the_row() {
        let spec = r##"{"spec":{"cloud_init":{"user":"ubuntu","ssh_pubkey":"ssh-ed25519 AAAA k","user_data":"#!/bin/sh\necho hi"}}}"##;
        let e = entry(&row(Some("192.168.122.5"), Some(r#"["192.168.122.5","10.0.0.9/24"]"#), Some(spec))).unwrap();
        assert_eq!(e["instance_id"], "i-0123456789abcdef0");
        assert_eq!(e["ips"], json!(["192.168.122.5", "10.0.0.9"]));
        assert_eq!(e["public_keys"], json!(["ssh-ed25519 AAAA k"]));
        assert_eq!(e["user_data"], "#!/bin/sh\necho hi");
        assert_eq!(e["instance_type"], "small");
        assert_eq!(e["enabled"], true);
    }

    #[test]
    fn a_disabled_metadata_endpoint_is_sent_to_the_agent() {
        let mut r = row(Some("10.0.0.1"), None, None);
        r.8 = false;
        assert_eq!(entry(&r).unwrap()["enabled"], false);
    }

    #[test]
    fn entries_are_grouped_per_host() {
        let mut a = row(Some("10.0.0.1"), None, None);
        let mut b = row(Some("10.0.0.2"), None, None);
        a.7 = Uuid::from_u128(1);
        b.7 = Uuid::from_u128(2);
        let g = group_by_host(&[a.clone(), b, a]);
        assert_eq!(g[&Uuid::from_u128(1)].len(), 2);
        assert_eq!(g[&Uuid::from_u128(2)].len(), 1);
    }
}
