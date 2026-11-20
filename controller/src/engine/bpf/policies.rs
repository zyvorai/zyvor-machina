// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Controller-owned runtime enforcement policies, persisted in `bpf_policies`
//! and reconciled onto every host's machina-bpfd. Response shapes match the
//! previous `/zeus-security/enforcement/*` contract.

use futures_util::future::join_all;
use machina_bpf::api::{Mode, Policy, Request, Scope, POLICY_KINDS};
use serde_json::{json, Value};
use crate::db::DbPool;

use super::{call, online_hosts, HostRef, OWNED_PREFIX};
use crate::agent_client;
use crate::config::ControllerConfig;

#[derive(Debug, Clone)]
pub struct StoredPolicy {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub match_value: String,
    pub enabled: bool,
    pub scope: String,
    pub description: String,
    pub applied_hosts: Vec<String>,
    pub created_at: String,
}

type Row = (
    String,
    String,
    String,
    String,
    bool,
    String,
    String,
    String,
    String,
);

fn from_row(r: Row) -> StoredPolicy {
    StoredPolicy {
        id: r.0,
        name: r.1,
        kind: r.2,
        match_value: r.3,
        enabled: r.4,
        scope: r.5,
        description: r.6,
        applied_hosts: serde_json::from_str(&r.7).unwrap_or_default(),
        created_at: r.8,
    }
}

const COLUMNS: &str =
    "id, name, kind, match_value, enabled, scope, description, applied_hosts, created_at";

pub fn to_json(p: &StoredPolicy) -> Value {
    json!({
        "id": p.id,
        "name": p.name,
        "kind": p.kind,
        "match": p.match_value,
        "enabled": p.enabled,
        "scope": p.scope,
        "description": p.description,
        "applied_hosts": p.applied_hosts,
        "created_at": p.created_at,
        "backend": "machina-bpf",
    })
}

pub async fn list(pool: &DbPool) -> anyhow::Result<Vec<StoredPolicy>> {
    let rows: Vec<Row> = crate::db::query_as(&format!(
        "SELECT {COLUMNS} FROM bpf_policies ORDER BY created_at, id"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(from_row).collect())
}

pub async fn get(pool: &DbPool, id: &str) -> anyhow::Result<Option<StoredPolicy>> {
    let row: Option<Row> =
        crate::db::query_as(&format!("SELECT {COLUMNS} FROM bpf_policies WHERE id = ?"))
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(from_row))
}

/// `fleet`, `host:<id>`, `vm:<name>`, `cgroup:<path>`.
fn parse_scope(scope: &str) -> Result<(), String> {
    match scope.split_once(':') {
        None if scope == "fleet" => Ok(()),
        Some(("host" | "vm" | "cgroup", v)) if !v.is_empty() => Ok(()),
        _ => Err(format!(
            "invalid scope {scope:?} (expected fleet, host:<id>, vm:<name> or cgroup:<path>)"
        )),
    }
}

/// Validate kind + match up front so a bad rule is rejected rather than
/// stored and silently skipped on every host.
fn validate(kind: &str, match_value: &str, scope: &str) -> Result<(), String> {
    if !POLICY_KINDS.contains(&kind) {
        return Err(format!(
            "unsupported policy kind {kind:?} (supported: {})",
            POLICY_KINDS.join(", ")
        ));
    }
    machina_bpf::policy::compile(kind, match_value)?;
    parse_scope(scope)
}

/// The bpfd policy for `p` on `host_id`, or `None` when out of scope there.
pub fn to_bpfd(p: &StoredPolicy, host_id: &str) -> Option<Policy> {
    let scope = match p.scope.split_once(':') {
        Some(("host", h)) if h == host_id => Scope::Host,
        Some(("host", _)) => return None,
        Some(("vm", name)) => Scope::Vm { name: name.into() },
        Some(("cgroup", path)) => Scope::Cgroup { path: path.into() },
        _ => Scope::Host,
    };
    Some(Policy {
        id: format!("{OWNED_PREFIX}{}", p.id),
        name: p.name.clone(),
        kind: p.kind.clone(),
        match_value: p.match_value.clone(),
        enabled: p.enabled,
        scope,
        description: p.description.clone(),
        created_at: Some(p.created_at.clone()),
    })
}

async fn sync_one(host: &HostRef, desired: &[Policy]) -> anyhow::Result<Value> {
    if host.id == super::LOCAL_HOST_ID {
        // Same reconcile as the agent's BpfSyncPolicies, against the local socket.
        let client = machina_bpf::BpfdClient::from_env();
        let current: Vec<Policy> = client.call_as(&Request::ListPolicies).await?;
        for p in current.iter().filter(|p| p.id.starts_with(OWNED_PREFIX)) {
            if !desired.iter().any(|d| d.id == p.id) {
                client
                    .call(&Request::RemovePolicy { id: p.id.clone() })
                    .await?;
            }
        }
        for p in desired {
            client
                .call(&Request::ApplyPolicy { policy: p.clone() })
                .await?;
        }
        return Ok(json!({ "applied": desired.len() }));
    }
    agent_client::bpf_sync_policies(&host.addr, desired, OWNED_PREFIX).await
}

/// Reconcile every online host (or just `only`) with the stored policy set.
pub async fn sync_hosts(pool: &DbPool, only: Option<&[String]>) -> anyhow::Result<Vec<Value>> {
    let all = list(pool).await?;
    let hosts: Vec<HostRef> = online_hosts(pool)
        .await
        .into_iter()
        .filter(|h| only.is_none_or(|ids| ids.contains(&h.id)))
        .collect();
    let futs = hosts.iter().map(|h| {
        let desired: Vec<Policy> = all.iter().filter_map(|p| to_bpfd(p, &h.id)).collect();
        async move {
            let res = sync_one(h, &desired).await;
            json!({
                "host_id": h.id,
                "hostname": h.hostname,
                "ok": res.is_ok(),
                "result": res.as_ref().ok(),
                "error": res.as_ref().err().map(|e| format!("{e:#}")),
                "policies": desired.len(),
            })
        }
    });
    Ok(join_all(futs).await)
}

fn ok_count(results: &[Value]) -> usize {
    results.iter().filter(|r| r["ok"] == true).count()
}

async fn set_mode(
    pool: &DbPool,
    host_ids: &[String],
    mode: Mode,
    lease_secs: Option<u64>,
) -> Vec<Value> {
    let hosts: Vec<HostRef> = online_hosts(pool)
        .await
        .into_iter()
        .filter(|h| host_ids.is_empty() || host_ids.contains(&h.id))
        .collect();
    let req = Request::SetMode { mode, lease_secs };
    let results = join_all(hosts.iter().map(|h| call(h, &req))).await;
    hosts
        .iter()
        .zip(results)
        .map(|(h, r)| {
            json!({
                "host_id": h.id,
                "hostname": h.hostname,
                "ok": r.is_ok(),
                "mode": r.as_ref().ok(),
                "error": r.as_ref().err().map(|e| format!("{e:#}")),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// /zeus-security/enforcement/* operations
// ---------------------------------------------------------------------------

pub async fn enforcement_status(pool: &DbPool) -> Value {
    let policies = list(pool).await.unwrap_or_default();
    let statuses = super::host_statuses(pool).await;
    let mut applied: Vec<String> = policies
        .iter()
        .flat_map(|p| p.applied_hosts.clone())
        .collect();
    applied.sort();
    applied.dedup();
    let enforcing: Vec<&super::HostBpfStatus> = statuses
        .iter()
        .filter(|s| {
            s.status
                .as_ref()
                .is_some_and(|st| st.mode.mode == Mode::Enforce)
        })
        .collect();
    let blocked: u64 = statuses
        .iter()
        .filter_map(|s| s.status.as_ref())
        .map(|s| s.counters.drops)
        .sum();
    let reachable = statuses.iter().filter(|s| s.reachable).count();
    let hosts: Vec<Value> = statuses
        .iter()
        .map(|s| {
            json!({
                "host_id": s.host_id,
                "hostname": s.hostname,
                "reachable": s.reachable,
                "error": s.error,
                "mode": s.status.as_ref().map(|st| &st.mode),
                "policies": s.status.as_ref().map(|st| st.policies_total),
                "counters": s.status.as_ref().map(|st| &st.counters),
            })
        })
        .collect();
    json!({
        "mode": if enforcing.is_empty() { "observe" } else { "enforce" },
        "policies_total": policies.len(),
        "policies_enabled": policies.iter().filter(|p| p.enabled).count(),
        "applied_hosts": applied,
        "blocked_events": blocked,
        "attached": !enforcing.is_empty(),
        "reachable": reachable > 0,
        "hosts": hosts,
        "kinds": POLICY_KINDS,
        "api_mode": "native",
        "summary": format!(
            "Native eBPF · {} polic(ies) · {}/{} host(s) reachable · {} enforcing",
            policies.len(),
            reachable,
            statuses.len(),
            enforcing.len()
        ),
    })
}

pub async fn enforcement_policies(pool: &DbPool) -> Value {
    let policies = list(pool).await.unwrap_or_default();
    json!({
        "policies": policies.iter().map(to_json).collect::<Vec<_>>(),
        "kinds": POLICY_KINDS,
        "api_mode": "native",
    })
}

pub async fn create_enforcement_policy(pool: &DbPool, body: &Value) -> Value {
    let s = |k: &str, d: &str| {
        body.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or(d)
            .trim()
            .to_string()
    };
    let name = s("name", "policy");
    let kind = s("kind", "");
    let match_value = s("match", "");
    let mut scope = s("scope", "fleet");
    let host_ids: Vec<String> = body
        .get("host_ids")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|h| h.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if scope == "fleet" && host_ids.len() == 1 {
        scope = format!("host:{}", host_ids[0]);
    }
    let enabled = body
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    if let Err(e) = validate(&kind, &match_value, &scope) {
        return json!({ "ok": false, "error": e, "api_mode": "native" });
    }
    let id = uuid::Uuid::new_v4().to_string();
    let res = crate::db::query(
        "INSERT INTO bpf_policies (id, name, kind, match_value, enabled, scope, description)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&kind)
    .bind(&match_value)
    .bind(enabled)
    .bind(&scope)
    .bind(s("description", ""))
    .execute(pool)
    .await;
    if let Err(e) = res {
        return json!({ "ok": false, "error": e.to_string(), "api_mode": "native" });
    }
    let sync = sync_hosts(pool, None).await.unwrap_or_default();
    let policy = get(pool, &id).await.ok().flatten();
    json!({
        "ok": true,
        "policy": policy.as_ref().map(to_json),
        "sync": sync,
        "api_mode": "native",
        "note": format!(
            "Policy installed on {} host(s) in observe mode — apply to start an enforce lease",
            ok_count(&sync)
        ),
    })
}

pub async fn apply_enforcement_policy(
    pool: &DbPool,
    cfg: &ControllerConfig,
    policy_id: &str,
    host_ids: &[String],
) -> Value {
    let Some(policy) = get(pool, policy_id).await.ok().flatten() else {
        return json!({ "ok": false, "error": "policy not found", "api_mode": "native" });
    };
    let only = (!host_ids.is_empty()).then_some(host_ids);
    let sync = sync_hosts(pool, only).await.unwrap_or_default();
    let modes = set_mode(
        pool,
        host_ids,
        Mode::Enforce,
        Some(cfg.bpf_enforce_lease_secs),
    )
    .await;
    let enforced: Vec<String> = modes
        .iter()
        .filter(|m| m["ok"] == true)
        .filter_map(|m| m["host_id"].as_str().map(String::from))
        .collect();
    let mut applied = policy.applied_hosts.clone();
    for h in &enforced {
        if !applied.contains(h) {
            applied.push(h.clone());
        }
    }
    let _ = crate::db::query(
        "UPDATE bpf_policies SET applied_hosts = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(serde_json::to_string(&applied).unwrap_or_else(|_| "[]".into()))
    .bind(policy_id)
    .execute(pool)
    .await;
    let ok = !enforced.is_empty() && ok_count(&sync) == sync.len();
    json!({
        "ok": ok,
        "api_mode": "native",
        "host_ids": enforced,
        "sync": sync,
        "modes": modes,
        "lease_secs": cfg.bpf_enforce_lease_secs,
        "summary": if ok {
            format!(
                "Enforce lease ({}s) active on {} host(s) — fails open to observe when it lapses",
                cfg.bpf_enforce_lease_secs,
                enforced.len()
            )
        } else {
            "Enforcement not fully applied — see per-host results".to_string()
        },
    })
}

pub async fn patch_enforcement_policy(pool: &DbPool, policy_id: &str, body: &Value) -> Value {
    let Some(mut p) = get(pool, policy_id).await.ok().flatten() else {
        return json!({ "ok": false, "error": "policy not found", "api_mode": "native" });
    };
    if let Some(e) = body.get("enabled").and_then(|v| v.as_bool()) {
        p.enabled = e;
    }
    if let Some(m) = body.get("match").and_then(|v| v.as_str()) {
        p.match_value = m.trim().to_string();
    }
    if let Some(d) = body.get("description").and_then(|v| v.as_str()) {
        p.description = d.to_string();
    }
    if let Err(e) = validate(&p.kind, &p.match_value, &p.scope) {
        return json!({ "ok": false, "error": e, "api_mode": "native" });
    }
    if let Err(e) = crate::db::query(
        "UPDATE bpf_policies SET enabled = ?, match_value = ?, description = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(p.enabled)
    .bind(&p.match_value)
    .bind(&p.description)
    .bind(policy_id)
    .execute(pool)
    .await
    {
        return json!({ "ok": false, "error": e.to_string(), "api_mode": "native" });
    }
    let sync = sync_hosts(pool, None).await.unwrap_or_default();
    json!({
        "ok": true,
        "policy": to_json(&p),
        "sync": sync,
        "api_mode": "native",
    })
}

pub async fn delete_enforcement_policy(pool: &DbPool, policy_id: &str) -> Value {
    let Some(p) = get(pool, policy_id).await.ok().flatten() else {
        return json!({ "ok": false, "error": "policy not found", "api_mode": "native" });
    };
    if let Err(e) = crate::db::query("DELETE FROM bpf_policies WHERE id = ?")
        .bind(policy_id)
        .execute(pool)
        .await
    {
        return json!({ "ok": false, "error": e.to_string(), "api_mode": "native" });
    }
    let sync = sync_hosts(pool, None).await.unwrap_or_default();
    json!({
        "ok": ok_count(&sync) == sync.len(),
        "removed_from_hosts": p.applied_hosts,
        "sync": sync,
        "api_mode": "native",
    })
}

/// The compiled per-host form of a policy (replaces the TracingPolicy view).
pub async fn policy_document(pool: &DbPool, policy_id: &str) -> Value {
    let Some(p) = get(pool, policy_id).await.ok().flatten() else {
        return json!({ "ok": false, "error": "policy not found", "api_mode": "native" });
    };
    let rules = machina_bpf::policy::compile(&p.kind, &p.match_value)
        .map(|r| r.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>())
        .unwrap_or_default();
    json!({
        "policy_id": policy_id,
        "native_policy": to_bpfd(&p, "").or_else(|| {
            // host-scoped: show it as it lands on its host
            p.scope.strip_prefix("host:").and_then(|h| to_bpfd(&p, h))
        }),
        "datapath_rules": rules,
        "api_mode": "native",
    })
}

pub async fn attach_enforcement(pool: &DbPool, cfg: &ControllerConfig) -> Value {
    let modes = set_mode(pool, &[], Mode::Enforce, Some(cfg.bpf_enforce_lease_secs)).await;
    let ok = !modes.is_empty() && ok_count(&modes) == modes.len();
    json!({
        "ok": ok,
        "attached": ok_count(&modes) > 0,
        "hosts": modes,
        "api_mode": "native",
        "note": format!("Enforce lease {}s requested on every online host", cfg.bpf_enforce_lease_secs),
    })
}

pub async fn detach_enforcement(pool: &DbPool) -> Value {
    let modes = set_mode(pool, &[], Mode::Observe, None).await;
    json!({
        "ok": ok_count(&modes) == modes.len(),
        "attached": false,
        "hosts": modes,
        "api_mode": "native",
        "note": "Reverted to observe (fail-open) on every reachable host",
    })
}

pub async fn sync_enforcement(pool: &DbPool) -> Value {
    let sync = sync_hosts(pool, None).await.unwrap_or_default();
    json!({
        "ok": ok_count(&sync) == sync.len(),
        "sync": sync,
        "api_mode": "native",
        "note": format!("Policy set reconciled on {}/{} host(s)", ok_count(&sync), sync.len()),
    })
}

pub async fn host_enforcement(pool: &DbPool, host_id: &str) -> Value {
    let all = list(pool).await.unwrap_or_default();
    let desired: Vec<Value> = all
        .iter()
        .filter(|p| to_bpfd(p, host_id).is_some())
        .map(to_json)
        .collect();
    let live = match super::host(pool, host_id).await {
        Some(h) => match call(&h, &Request::Status).await {
            Ok(v) => json!({ "reachable": true, "status": v }),
            Err(e) => json!({ "reachable": false, "error": format!("{e:#}") }),
        },
        None => json!({ "reachable": false, "error": "host not found" }),
    };
    json!({
        "host_id": host_id,
        "policies": desired,
        "live": live,
        "api_mode": "native",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pol(scope: &str) -> StoredPolicy {
        StoredPolicy {
            id: "p1".into(),
            name: "n".into(),
            kind: "deny_ip".into(),
            match_value: "203.0.113.5".into(),
            enabled: true,
            scope: scope.into(),
            description: String::new(),
            applied_hosts: vec![],
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn scopes_map_to_bpfd() {
        assert_eq!(to_bpfd(&pol("fleet"), "h1").unwrap().scope, Scope::Host);
        assert_eq!(to_bpfd(&pol("fleet"), "h1").unwrap().id, "ctl-p1");
        assert!(to_bpfd(&pol("host:h2"), "h1").is_none());
        assert_eq!(to_bpfd(&pol("host:h1"), "h1").unwrap().scope, Scope::Host);
        assert_eq!(
            to_bpfd(&pol("vm:web"), "h1").unwrap().scope,
            Scope::Vm { name: "web".into() }
        );
    }

    #[test]
    fn validation_fails_closed() {
        assert!(validate("tc_allow", "443/tcp", "fleet").is_err());
        assert!(validate("deny_namespace", "x", "fleet").is_err());
        assert!(validate("deny_ip", "10.0.0.0/8", "bogus").is_err());
        assert!(validate("deny_ip", "10.0.0.0/8", "vm:web").is_ok());
    }
}
