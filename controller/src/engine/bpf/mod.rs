// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Native eBPF fleet layer: talks to each host's machina-bpfd through its
//! agent (`BpfCall` / `BpfSyncPolicies`).

pub mod policies;
pub mod telemetry;

use futures_util::future::join_all;
use machina_bpf::api::{BpfStatus, Request};
use serde::Serialize;
use serde_json::Value;
use crate::db::DbPool;
use uuid::Uuid;

use crate::agent_client;

/// Prefix of every policy id the controller owns on a host's bpfd. Policies
/// without it (added locally on the host) are never pruned by fleet sync.
pub const OWNED_PREFIX: &str = "ctl-";

/// Pseudo host id used when no agents are registered and the controller
/// talks to a co-located machina-bpfd socket directly.
pub const LOCAL_HOST_ID: &str = "local";

#[derive(Debug, Clone, Serialize)]
pub struct HostRef {
    pub id: String,
    pub hostname: String,
    #[serde(skip)]
    pub addr: String,
    pub state: String,
}

impl HostRef {
    pub(crate) fn is_local(&self) -> bool {
        self.id == LOCAL_HOST_ID
    }
}

fn local_host() -> HostRef {
    HostRef {
        id: LOCAL_HOST_ID.into(),
        hostname: std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".into()),
        addr: String::new(),
        state: "online".into(),
    }
}

/// Registered hosts (all states). Falls back to the local bpfd socket when
/// no host is registered and the socket exists.
pub async fn hosts(pool: &DbPool) -> Vec<HostRef> {
    let rows: Vec<(Uuid, String, String, String)> =
        crate::db::query_as("SELECT id, hostname, agent_grpc_addr, state FROM hosts ORDER BY hostname")
            .fetch_all(pool)
            .await
            .unwrap_or_default();
    let mut out: Vec<HostRef> = rows
        .into_iter()
        .map(|(id, hostname, addr, state)| HostRef {
            id: id.to_string(),
            hostname,
            addr,
            state,
        })
        .collect();
    if out.is_empty() && machina_bpf::BpfdClient::from_env().reachable() {
        out.push(local_host());
    }
    out
}

pub async fn online_hosts(pool: &DbPool) -> Vec<HostRef> {
    hosts(pool)
        .await
        .into_iter()
        .filter(|h| h.state == "online" && (h.is_local() || !h.addr.is_empty()))
        .collect()
}

pub async fn host(pool: &DbPool, host_id: &str) -> Option<HostRef> {
    hosts(pool).await.into_iter().find(|h| h.id == host_id)
}

/// One bpfd request on one host.
pub async fn call(host: &HostRef, req: &Request) -> anyhow::Result<Value> {
    if host.is_local() {
        return machina_bpf::BpfdClient::from_env().call(req).await;
    }
    agent_client::bpf_call(&host.addr, req).await
}

/// The same request on every online host, concurrently.
pub async fn fan_out(pool: &DbPool, req: &Request) -> Vec<(HostRef, anyhow::Result<Value>)> {
    let hosts = online_hosts(pool).await;
    let results = join_all(hosts.iter().map(|h| call(h, req))).await;
    hosts.into_iter().zip(results).collect()
}

/// Successful results only, each JSON array item tagged with its host.
pub async fn fan_out_items(pool: &DbPool, req: &Request) -> Vec<Value> {
    let mut out = Vec::new();
    for (host, res) in fan_out(pool, req).await {
        let Ok(Value::Array(items)) = res else {
            continue;
        };
        for mut item in items {
            if let Some(obj) = item.as_object_mut() {
                obj.insert("host_id".into(), Value::String(host.id.clone()));
                obj.insert("hostname".into(), Value::String(host.hostname.clone()));
            }
            out.push(item);
        }
    }
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct HostBpfStatus {
    pub host_id: String,
    pub hostname: String,
    pub reachable: bool,
    pub error: Option<String>,
    pub status: Option<BpfStatus>,
}

pub async fn host_statuses(pool: &DbPool) -> Vec<HostBpfStatus> {
    fan_out(pool, &Request::Status)
        .await
        .into_iter()
        .map(|(h, res)| {
            let (status, error) =
                match res.and_then(|v| Ok(serde_json::from_value::<BpfStatus>(v)?)) {
                    Ok(s) => (Some(s), None),
                    Err(e) => (None, Some(format!("{e:#}"))),
                };
            HostBpfStatus {
                host_id: h.id,
                hostname: h.hostname,
                reachable: status.is_some(),
                error,
                status,
            }
        })
        .collect()
}

/// Fleet-level native eBPF summary for status panels.
#[derive(Debug, Clone, Serialize)]
pub struct FleetBpfStatus {
    pub enabled: bool,
    pub reachable: bool,
    pub hosts_total: usize,
    pub hosts_reachable: usize,
    pub enforcing_hosts: usize,
    pub version: String,
    pub summary: String,
}

pub async fn fleet_status(pool: &DbPool) -> FleetBpfStatus {
    let statuses = host_statuses(pool).await;
    let reachable = statuses.iter().filter(|s| s.reachable).count();
    let enforcing = statuses
        .iter()
        .filter_map(|s| s.status.as_ref())
        .filter(|s| s.mode.mode == machina_bpf::api::Mode::Enforce)
        .count();
    FleetBpfStatus {
        enabled: true,
        reachable: reachable > 0,
        hosts_total: statuses.len(),
        hosts_reachable: reachable,
        enforcing_hosts: enforcing,
        version: machina_bpf::VERSION.into(),
        summary: format!(
            "native eBPF on {reachable}/{} host(s) · {enforcing} enforcing",
            statuses.len()
        ),
    }
}
