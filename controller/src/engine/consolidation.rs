// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Consolidation: find under-used hosts whose VMs all fit elsewhere, plan the
//! live migrations that empty them, and offer them for power-down. Applied
//! through the `drs.consolidate` approval action; every move still passes the
//! migration precheck when it runs.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::db::DbPool;
use uuid::Uuid;

use crate::api::ApiError;
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

pub const CONSOLIDATE_ACTION: &str = "drs.consolidate";
/// Hosts below this memory use are candidates to empty.
pub const UNDER_USED: f64 = 0.30;
/// No destination is filled past this.
pub const FILL_LIMIT: f64 = 0.80;

#[derive(Debug, Clone, Serialize)]
pub struct HostLoad {
    pub id: Uuid,
    pub name: String,
    pub memory_total_mib: i64,
    pub memory_used_mib: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct VmLoad {
    pub id: Uuid,
    pub name: String,
    pub host: Uuid,
    pub memory_mib: i64,
    /// Can't leave its host (host-local cloud network, local-only disk...).
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Move {
    pub vm_id: Uuid,
    pub vm: String,
    pub from: Uuid,
    pub to: Uuid,
    pub to_name: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Plan {
    pub moves: Vec<Move>,
    /// Hosts the moves leave with no VMs: candidates to power down.
    pub emptied: Vec<HostLoad>,
    /// Under-used hosts that can't be emptied, and why.
    pub kept: Vec<(String, String)>,
}

fn ratio(h: &HostLoad, used: i64) -> f64 {
    if h.memory_total_mib <= 0 {
        return 1.0;
    }
    used as f64 / h.memory_total_mib as f64
}

/// Empties the least-used hosts first. Each VM goes to the fullest host that
/// still stays under [`FILL_LIMIT`] with it (best fit), never to a host being
/// emptied. A host is only emptied when every one of its VMs can move; at
/// least one host is always kept.
pub fn plan(hosts: &[HostLoad], vms: &[VmLoad]) -> Plan {
    let mut used: std::collections::BTreeMap<Uuid, i64> =
        hosts.iter().map(|h| (h.id, h.memory_used_mib)).collect();
    let mut order: Vec<&HostLoad> = hosts
        .iter()
        .filter(|h| h.memory_total_mib > 0 && ratio(h, h.memory_used_mib) < UNDER_USED)
        .collect();
    order.sort_by(|a, b| {
        ratio(a, a.memory_used_mib)
            .total_cmp(&ratio(b, b.memory_used_mib))
            .then(a.name.cmp(&b.name))
    });
    let mut out = Plan::default();
    let mut emptied: Vec<Uuid> = Vec::new();
    for h in order {
        if hosts.len() - emptied.len() <= 1 {
            out.kept
                .push((h.name.clone(), "the last host stays on".into()));
            break;
        }
        let mine: Vec<&VmLoad> = vms.iter().filter(|v| v.host == h.id).collect();
        if let Some(p) = mine.iter().find(|v| v.pinned) {
            out.kept
                .push((h.name.clone(), format!("{} can't leave this host", p.name)));
            continue;
        }
        let mut trial = used.clone();
        let mut moves = Vec::new();
        let mut stuck = None;
        let mut big_first = mine.clone();
        big_first.sort_by_key(|a| std::cmp::Reverse(a.memory_mib));
        for v in big_first {
            let dest = hosts
                .iter()
                .filter(|d| d.id != h.id && !emptied.contains(&d.id) && d.memory_total_mib > 0)
                .filter(|d| ratio(d, trial[&d.id] + v.memory_mib) <= FILL_LIMIT)
                .max_by(|a, b| ratio(a, trial[&a.id]).total_cmp(&ratio(b, trial[&b.id])));
            match dest {
                Some(d) => {
                    *trial.get_mut(&d.id).expect("host") += v.memory_mib;
                    *trial.get_mut(&h.id).expect("host") -= v.memory_mib;
                    moves.push(Move {
                        vm_id: v.id,
                        vm: v.name.clone(),
                        from: h.id,
                        to: d.id,
                        to_name: d.name.clone(),
                    });
                }
                None => {
                    stuck = Some(v.name.clone());
                    break;
                }
            }
        }
        if let Some(name) = stuck {
            out.kept
                .push((h.name.clone(), format!("no room elsewhere for {name}")));
            continue;
        }
        used = trial;
        emptied.push(h.id);
        out.moves.extend(moves);
        out.emptied.push(h.clone());
    }
    out
}

pub async fn load(pool: &DbPool) -> anyhow::Result<(Vec<HostLoad>, Vec<VmLoad>)> {
    let hosts: Vec<(Uuid, String, i64, i64)> = crate::db::query_as(
        "SELECT id, hostname, memory_total_mib, memory_used_mib FROM hosts
         WHERE state = 'online' AND maintenance_mode = FALSE AND schedulable = TRUE
         ORDER BY hostname",
    )
    .fetch_all(pool)
    .await?;
    let hosts: Vec<HostLoad> = hosts
        .into_iter()
        .map(|(id, name, total, used)| HostLoad {
            id,
            name,
            memory_total_mib: total,
            memory_used_mib: used,
        })
        .collect();
    let rows: Vec<(Uuid, String, Uuid, i64)> = crate::db::query_as(
        "SELECT id, name, host_id, memory_mib FROM vms
         WHERE host_id IS NOT NULL AND observed_state = 'running' AND inventory_source = 'libvirt'",
    )
    .fetch_all(pool)
    .await?;
    let mut vms = Vec::new();
    for (id, name, host, memory_mib) in rows {
        if !hosts.iter().any(|h| h.id == host) {
            continue;
        }
        let pinned = crate::api::cloud::check_vm_host(pool, id, Uuid::nil())
            .await
            .is_err();
        vms.push(VmLoad {
            id,
            name,
            host,
            memory_mib,
            pinned,
        });
    }
    Ok((hosts, vms))
}

pub async fn current(pool: &DbPool) -> anyhow::Result<Plan> {
    let (hosts, vms) = load(pool).await?;
    Ok(plan(&hosts, &vms))
}

#[derive(Debug, Deserialize)]
struct ConsolidateRef {
    moves: Vec<Move>,
}

/// Enqueues the planned live migrations that still pass the precheck.
pub async fn execute(state: &AppState, object_ref: &Value) -> Result<Value, ApiError> {
    let r: ConsolidateRef = serde_json::from_value(object_ref.clone())
        .map_err(|e| ApiError::bad_request(format!("consolidation object_ref: {e}")))?;
    let mut queued = Vec::new();
    let mut skipped = Vec::new();
    for m in &r.moves {
        let host: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
            .bind(m.vm_id)
            .fetch_optional(&state.pool)
            .await?
            .flatten();
        if host != Some(m.from) {
            skipped.push(format!("{}: no longer on its planned source", m.vm));
            continue;
        }
        let pre =
            crate::engine::migrate_precheck::run_migrate_precheck(&state.pool, m.vm_id, m.to, true)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
        if !pre.ok {
            skipped.push(format!("{}: precheck failed", m.vm));
            continue;
        }
        let task = enqueue_task(
            state,
            "vm.migrate",
            json!({
                "vm_id": m.vm_id.to_string(),
                "dest_host_id": m.to.to_string(),
                "live": true,
                "undefine_source": true,
            }),
            Some("vm"),
            Some(m.vm_id),
            Some(m.from),
        )
        .await?;
        queued.push(task);
    }
    Ok(json!({
        "message": format!("{} migrations queued, {} skipped", queued.len(), skipped.len()),
        "task_ids": queued,
        "skipped": skipped,
    }))
}

pub async fn check(state: &AppState, object_ref: &Value) -> (&'static str, String) {
    let Ok(r) = serde_json::from_value::<ConsolidateRef>(object_ref.clone()) else {
        return (
            "unknown",
            "The consolidation plan could not be read.".into(),
        );
    };
    let mut done = 0;
    for m in &r.moves {
        let host: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
            .bind(m.vm_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten()
            .flatten();
        if host == Some(m.to) {
            done += 1;
        }
    }
    if done == r.moves.len() {
        ("ok", format!("All {done} VMs are on their new hosts."))
    } else {
        (
            "pending",
            format!("{done} of {} VMs moved so far.", r.moves.len()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(n: u128, name: &str, total: i64, used: i64) -> HostLoad {
        HostLoad {
            id: Uuid::from_u128(n),
            name: name.into(),
            memory_total_mib: total,
            memory_used_mib: used,
        }
    }

    fn vm(n: u128, host: u128, mem: i64) -> VmLoad {
        VmLoad {
            id: Uuid::from_u128(100 + n),
            name: format!("vm{n}"),
            host: Uuid::from_u128(host),
            memory_mib: mem,
            pinned: false,
        }
    }

    #[test]
    fn empties_the_quiet_host_into_the_fullest_that_fits() {
        let hosts = [
            host(1, "a", 64_000, 40_000),
            host(2, "b", 64_000, 30_000),
            host(3, "quiet", 64_000, 6_000),
        ];
        let vms = [vm(1, 3, 4_000), vm(2, 3, 2_000), vm(3, 1, 40_000)];
        let p = plan(&hosts, &vms);
        assert_eq!(p.emptied.len(), 1);
        assert_eq!(p.emptied[0].name, "quiet");
        // a (62.5%) is fullest and still fits both under 80%.
        assert!(p.moves.iter().all(|m| m.to_name == "a"), "{:?}", p.moves);
        assert_eq!(p.moves[0].vm, "vm1", "biggest first");
    }

    #[test]
    fn pinned_vms_and_full_fleets_keep_the_host() {
        let hosts = [host(1, "a", 10_000, 7_500), host(2, "quiet", 10_000, 1_000)];
        let mut vms = vec![vm(1, 2, 1_000)];
        let p = plan(&hosts, &vms);
        assert!(p.moves.is_empty());
        assert!(p.kept[0].1.contains("no room"), "{:?}", p.kept);
        vms[0].pinned = true;
        let p = plan(
            &[host(1, "a", 10_000, 2_000), host(2, "quiet", 10_000, 1_000)],
            &vms,
        );
        assert!(
            p.kept.iter().any(|k| k.1.contains("can't leave")),
            "{:?}",
            p.kept
        );
    }

    #[test]
    fn the_last_host_stays_on() {
        let hosts = [host(1, "a", 10_000, 500), host(2, "b", 10_000, 400)];
        let p = plan(&hosts, &[vm(1, 1, 500), vm(2, 2, 400)]);
        assert_eq!(p.emptied.len(), 1);
        assert_eq!(p.emptied[0].name, "b");
        assert!(p
            .kept
            .iter()
            .any(|k| k.0 == "a" && k.1.contains("last host")));
        assert!(plan(&[host(1, "solo", 10_000, 100)], &[])
            .emptied
            .is_empty());
    }
}
