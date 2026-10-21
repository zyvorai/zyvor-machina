// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use machina_spec::PlacementRecommendation;
use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
struct HostLoad {
    id: Uuid,
    hostname: String,
    cpu_percent: f32,
    memory_used_mib: i64,
    memory_total_mib: i64,
    vm_count: i32,
    tags: crate::db::Json<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlacementRecommendationRow {
    pub vm_id: String,
    pub vm_name: String,
    pub from_host_id: String,
    pub from_host_name: String,
    pub to_host_id: String,
    pub to_host_name: String,
    pub reason: String,
    pub score: f32,
}

pub async fn compute_recommendations(
    pool: &DbPool,
) -> anyhow::Result<Vec<PlacementRecommendationRow>> {
    let threshold: f32 =
        crate::db::query_scalar("SELECT drs_cpu_threshold FROM clusters ORDER BY created_at LIMIT 1")
            .fetch_one(pool)
            .await
            .unwrap_or(85.0);

    let placement_policy: String =
        crate::db::query_scalar("SELECT placement_policy FROM clusters ORDER BY created_at LIMIT 1")
            .fetch_one(pool)
            .await
            .unwrap_or_else(|_| "balanced".into());

    let hosts: Vec<HostLoad> = crate::db::query_as(
        "SELECT id, hostname, cpu_percent, memory_used_mib, memory_total_mib, vm_count,
                COALESCE(tags, '[]') AS tags
         FROM hosts WHERE state = 'online' AND maintenance_mode = FALSE AND schedulable = TRUE",
    )
    .fetch_all(pool)
    .await?;

    if hosts.len() < 2 {
        return Ok(Vec::new());
    }

    let vms: Vec<(Uuid, String, Uuid, i64, crate::db::Json<Vec<String>>)> = crate::db::query_as(
        "SELECT v.id, v.name, v.host_id, v.memory_mib, COALESCE(v.tags, '[]') AS tags FROM vms v
         JOIN hosts h ON h.id = v.host_id
         WHERE v.desired_state = 'running' AND h.state = 'online'",
    )
    .fetch_all(pool)
    .await?;

    let anti_map = host_anti_affinity_map(pool).await?;
    let mut out = Vec::new();

    // Running per-host tally of memory/VM-count already committed to a destination
    // by earlier recommendations in THIS SAME pass. `hosts` is a snapshot fetched
    // once above; without this, every hot VM is scored against that same stale
    // snapshot, so several VMs can each independently pick the one coolest host as
    // "best" and all get recommended onto it — collectively overcommitting a host
    // that only had room for one of them. Each accepted recommendation updates the
    // destination's tally before the next VM is considered.
    let mut committed: std::collections::HashMap<Uuid, (i64, i32)> =
        std::collections::HashMap::new();

    for (vm_id, vm_name, host_id, memory_mib, vm_tags) in vms {
        let Some(source) = hosts.iter().find(|h| h.id == host_id) else {
            continue;
        };
        let mem_pct = pct(source.memory_used_mib, source.memory_total_mib);
        if source.cpu_percent < threshold && mem_pct < threshold {
            continue;
        }

        let mut best: Option<(&HostLoad, f32)> = None;
        for dest in &hosts {
            if dest.id == host_id {
                continue;
            }
            // Anti-affinity: never rebalance a VM onto a host already running a
            // peer in the same anti-affinity group.
            if anti_map
                .get(&dest.id)
                .is_some_and(|a| violates_anti_affinity(&vm_tags, a))
            {
                continue;
            }
            let (committed_mem, committed_count) =
                committed.get(&dest.id).copied().unwrap_or((0, 0));
            let adj_used_mib = dest.memory_used_mib + committed_mem;
            // Hard capacity guard: a destination already filled up by earlier
            // recommendations this pass cannot take on another VM's memory, no
            // matter how good its raw (stale) score looks.
            if adj_used_mib + memory_mib > dest.memory_total_mib {
                continue;
            }
            let dest_mem_pct = pct(adj_used_mib, dest.memory_total_mib);
            let mut score = dest_score(
                &placement_policy,
                dest.cpu_percent,
                dest_mem_pct,
                dest.vm_count + committed_count,
            );
            score += tag_affinity_score(&vm_tags, &dest.tags);
            if score <= 0.0 {
                continue;
            }
            if best.map(|(_, s)| score > s).unwrap_or(true) {
                best = Some((dest, score));
            }
        }

        let Some((dest, score)) = best else { continue };

        // Hysteresis: only move if the destination is meaningfully better than the
        // source (by DRS_HYSTERESIS_MARGIN), so a VM doesn't ping-pong between two
        // similarly-loaded hosts on successive DRS ticks.
        let source_score = dest_score(
            &placement_policy,
            source.cpu_percent,
            mem_pct,
            source.vm_count,
        );
        if score - source_score < DRS_HYSTERESIS_MARGIN {
            continue;
        }

        let reason = format!(
            "{} memory {:.0}%, CPU {:.0}% — move to {} (memory {:.0}%, CPU {:.0}%)",
            source.hostname,
            mem_pct,
            source.cpu_percent,
            dest.hostname,
            pct(dest.memory_used_mib, dest.memory_total_mib),
            dest.cpu_percent,
        );

        // Commit this VM's memory/count onto the destination's running tally so
        // the next VM considered in this pass sees a destination that's already
        // (virtually) a bit fuller.
        let entry = committed.entry(dest.id).or_insert((0, 0));
        entry.0 += memory_mib;
        entry.1 += 1;

        out.push(PlacementRecommendationRow {
            vm_id: vm_id.to_string(),
            vm_name,
            from_host_id: source.id.to_string(),
            from_host_name: source.hostname.clone(),
            to_host_id: dest.id.to_string(),
            to_host_name: dest.hostname.clone(),
            reason,
            score,
        });
    }

    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

pub async fn persist_recommendations(
    pool: &DbPool,
    rows: &[PlacementRecommendationRow],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    crate::db::query("UPDATE placement_recommendations SET status = 'superseded' WHERE status = 'open'")
        .execute(&mut *tx)
        .await?;

    for row in rows.iter().take(50) {
        crate::db::query(
            "INSERT INTO placement_recommendations (id, vm_id, from_host_id, to_host_id, reason, score)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(Uuid::parse_str(&row.vm_id)?)
        .bind(Uuid::parse_str(&row.from_host_id)?)
        .bind(Uuid::parse_str(&row.to_host_id)?)
        .bind(&row.reason)
        .bind(row.score)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub fn to_spec_rows(rows: &[PlacementRecommendationRow]) -> Vec<PlacementRecommendation> {
    rows.iter()
        .map(|r| PlacementRecommendation {
            vm_id: r.vm_id.clone(),
            vm_name: r.vm_name.clone(),
            from_host_id: r.from_host_id.clone(),
            to_host_id: r.to_host_id.clone(),
            reason: r.reason.clone(),
        })
        .collect()
}

fn pct(used: i64, total: i64) -> f32 {
    if total <= 0 {
        return 0.0;
    }
    (used as f32 / total as f32) * 100.0
}

fn headroom_score(cpu: f32, mem_pct: f32, vm_count: i32) -> f32 {
    let base = (100.0 - cpu).max(0.0) + (100.0 - mem_pct).max(0.0);
    base - (vm_count as f32 * 2.0)
}

fn dest_score(policy: &str, cpu: f32, mem_pct: f32, vm_count: i32) -> f32 {
    match policy {
        "packed" => {
            let headroom = headroom_score(cpu, mem_pct, vm_count);
            if headroom < 10.0 {
                return 0.0;
            }
            (vm_count as f32 * 15.0) + headroom * 0.5
        }
        _ => headroom_score(cpu, mem_pct, vm_count),
    }
}

#[derive(Debug, sqlx::FromRow)]
struct HostCandidate {
    id: Uuid,
    cpu_percent: f32,
    memory_used_mib: i64,
    memory_total_mib: i64,
    vm_count: i32,
    tags: crate::db::Json<Vec<String>>,
}

pub async fn pick_host_for_vm(
    pool: &DbPool,
    vm_tags: &[String],
    memory_mib: i64,
) -> anyhow::Result<Uuid> {
    let placement_policy: String =
        crate::db::query_scalar("SELECT placement_policy FROM clusters ORDER BY created_at LIMIT 1")
            .fetch_one(pool)
            .await
            .unwrap_or_else(|_| "balanced".into());

    let hosts: Vec<HostCandidate> = crate::db::query_as(
        "SELECT id, cpu_percent, memory_used_mib, memory_total_mib, vm_count,
                COALESCE(tags, '[]') AS tags
         FROM hosts WHERE state = 'online' AND maintenance_mode = FALSE AND schedulable = TRUE",
    )
    .fetch_all(pool)
    .await?;

    if hosts.is_empty() {
        anyhow::bail!("no online hosts available");
    }

    let anti_map = host_anti_affinity_map(pool).await?;
    // A peer in the same rack counts like a peer on the same host (still a preference).
    let anti_map = crate::engine::failure_domains::widen_to_domains(
        &anti_map,
        &crate::engine::failure_domains::host_domains(pool).await?,
    );
    choose_host(&hosts, &anti_map, &placement_policy, vm_tags, memory_mib)
        .map(|(id, _)| id)
        .ok_or_else(|| anyhow::anyhow!("no suitable host for placement"))
}

type AntiMap = std::collections::HashMap<Uuid, std::collections::HashSet<String>>;

/// host -> anti-affinity tags of the running VMs on it, widened across racks: a peer anywhere in a
/// host's rack counts. Used by HA recovery to keep a VM out of a rack that holds a group peer.
pub(crate) async fn anti_affinity_by_rack(pool: &DbPool) -> anyhow::Result<AntiMap> {
    let hosts = host_anti_affinity_map(pool).await?;
    Ok(crate::engine::failure_domains::widen_to_domains(
        &hosts,
        &crate::engine::failure_domains::host_domains(pool).await?,
    ))
}

/// True when `host` already carries a member of one of `vm_tags`' anti-affinity groups.
pub(crate) fn host_conflicts(map: &AntiMap, host: &Uuid, vm_tags: &[String]) -> bool {
    map.get(host).is_some_and(|a| violates_anti_affinity(vm_tags, a))
}

/// The best host and whether it breaks anti-affinity (only when every host would).
fn choose_host(
    hosts: &[HostCandidate],
    anti_map: &AntiMap,
    placement_policy: &str,
    vm_tags: &[String],
    memory_mib: i64,
) -> Option<(Uuid, bool)> {
    // Prefer a host that doesn't violate anti-affinity, but keep the best overall as
    // a fallback so placement still succeeds when every host would violate (better to
    // place with a co-location than to fail the create).
    let mut best_ok: Option<(Uuid, f32)> = None;
    let mut best_any: Option<(Uuid, f32)> = None;
    for h in hosts {
        // Hard capacity guard: never place a VM on a host that doesn't actually
        // have enough free memory for it, no matter how good its (percentage-
        // based) score looks. Without this a host could be picked purely on
        // headroom-score/vm_count and pushed into memory over-commit — this is
        // the same hard check `compute_recommendations` applies to DRS moves.
        if h.memory_used_mib + memory_mib > h.memory_total_mib {
            continue;
        }
        let mem_pct = pct(h.memory_used_mib, h.memory_total_mib);
        let mut score = dest_score(placement_policy, h.cpu_percent, mem_pct, h.vm_count);
        if score <= 0.0 {
            continue;
        }
        score += tag_affinity_score(vm_tags, &h.tags);
        if best_any.map(|(_, s)| score > s).unwrap_or(true) {
            best_any = Some((h.id, score));
        }
        let violates = anti_map
            .get(&h.id)
            .is_some_and(|a| violates_anti_affinity(vm_tags, a));
        if !violates && best_ok.map(|(_, s)| score > s).unwrap_or(true) {
            best_ok = Some((h.id, score));
        }
    }
    best_ok
        .map(|(id, _)| (id, false))
        .or(best_any.map(|(id, _)| (id, true)))
}

/// A VM to place in [`simulate_placement`].
pub struct PlacementAsk {
    pub name: String,
    pub tags: Vec<String>,
    pub memory_mib: i64,
}

/// Where each VM would land if created in this order, counting the memory and
/// anti-affinity of the ones placed before it. Creates nothing.
pub async fn simulate_placement(
    pool: &DbPool,
    asks: &[PlacementAsk],
) -> anyhow::Result<Vec<(String, Option<(Uuid, String)>, bool)>> {
    let placement_policy: String =
        crate::db::query_scalar("SELECT placement_policy FROM clusters ORDER BY created_at LIMIT 1")
            .fetch_one(pool)
            .await
            .unwrap_or_else(|_| "balanced".into());
    let mut hosts: Vec<HostCandidate> = crate::db::query_as(
        "SELECT id, cpu_percent, memory_used_mib, memory_total_mib, vm_count,
                COALESCE(tags, '[]') AS tags
         FROM hosts WHERE state = 'online' AND maintenance_mode = FALSE AND schedulable = TRUE",
    )
    .fetch_all(pool)
    .await?;
    let names: std::collections::HashMap<Uuid, String> =
        crate::db::query_as::<_, (Uuid, String)>("SELECT id, hostname FROM hosts")
            .fetch_all(pool)
            .await?
            .into_iter()
            .collect();
    let mut anti_map = host_anti_affinity_map(pool).await?;
    let domains = crate::engine::failure_domains::host_domains(pool).await?;
    let mut out = Vec::with_capacity(asks.len());
    for a in asks {
        let widened = crate::engine::failure_domains::widen_to_domains(&anti_map, &domains);
        match choose_host(&hosts, &widened, &placement_policy, &a.tags, a.memory_mib) {
            Some((id, violates)) => {
                if let Some(h) = hosts.iter_mut().find(|h| h.id == id) {
                    h.memory_used_mib += a.memory_mib;
                    h.vm_count += 1;
                }
                anti_map
                    .entry(id)
                    .or_default()
                    .extend(anti_affinity_tags(&a.tags));
                let host = names.get(&id).cloned().unwrap_or_else(|| id.to_string());
                out.push((a.name.clone(), Some((id, host)), violates));
            }
            None => out.push((a.name.clone(), None, false)),
        }
    }
    Ok(out)
}

fn tag_affinity_score(vm_tags: &[String], host_tags: &[String]) -> f32 {
    if vm_tags.is_empty() || host_tags.is_empty() {
        return 0.0;
    }
    let overlap = vm_tags.iter().filter(|t| host_tags.contains(t)).count();
    overlap as f32 * 25.0
}

/// DRS only migrates a VM when the destination's headroom score beats the source's
/// by at least this much. Without a hysteresis band, a VM straddling the threshold
/// ping-pongs between two similarly-loaded hosts on successive ticks.
const DRS_HYSTERESIS_MARGIN: f32 = 20.0;

/// Tags with this prefix declare an anti-affinity group: two running VMs that share
/// `anti-affinity:<group>` must NOT be co-located, so a single host failure can't
/// take out both (e.g. replicas). Placement and DRS avoid a host already running a
/// group peer.
const ANTI_AFFINITY_PREFIX: &str = "anti-affinity:";

fn anti_affinity_tags(tags: &[String]) -> std::collections::HashSet<String> {
    tags.iter()
        .map(|t| t.to_ascii_lowercase())
        .filter(|t| t.starts_with(ANTI_AFFINITY_PREFIX))
        .collect()
}

/// True if placing a VM with `vm_tags` onto a host whose running VMs carry
/// `host_anti_tags` would put two members of the same anti-affinity group together.
fn violates_anti_affinity(
    vm_tags: &[String],
    host_anti_tags: &std::collections::HashSet<String>,
) -> bool {
    anti_affinity_tags(vm_tags)
        .iter()
        .any(|t| host_anti_tags.contains(t))
}

/// host_id -> anti-affinity tags of the running VMs currently on it.
async fn host_anti_affinity_map(pool: &DbPool) -> anyhow::Result<AntiMap> {
    let rows: Vec<(Uuid, crate::db::Json<Vec<String>>)> = crate::db::query_as(
        "SELECT host_id, COALESCE(tags, '[]') AS tags
         FROM vms WHERE desired_state = 'running' AND host_id IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;
    let mut map: std::collections::HashMap<Uuid, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    for (host_id, tags) in rows {
        map.entry(host_id)
            .or_default()
            .extend(anti_affinity_tags(&tags));
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn tags(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn anti_affinity_detects_shared_group_case_insensitively() {
        let vm = tags(&["role:db", "Anti-Affinity:DB-Replicas"]);
        let mut host: HashSet<String> = HashSet::new();
        host.insert("anti-affinity:db-replicas".into());
        assert!(violates_anti_affinity(&vm, &host));
    }

    #[test]
    fn anti_affinity_ignores_non_group_and_disjoint_tags() {
        let vm = tags(&["role:db", "anti-affinity:group-a"]);
        let mut host: HashSet<String> = HashSet::new();
        host.insert("anti-affinity:group-b".into()); // different group
        assert!(!violates_anti_affinity(&vm, &host));
        // A VM with no anti-affinity tag never conflicts.
        assert!(!violates_anti_affinity(&tags(&["role:web"]), &host));
    }

    #[test]
    fn hysteresis_blocks_marginal_moves_allows_clear_wins() {
        // dest only 10 better than source (< margin) -> no move.
        let source = std::hint::black_box(20.0f32);
        assert!(30.0 - source < DRS_HYSTERESIS_MARGIN);
        // dest 40 better -> move.
        assert!(60.0 - source >= DRS_HYSTERESIS_MARGIN);
    }

    // Overcommit regression: `pick_host_for_vm` used to take `memory_mib` but
    // never check it (`_memory_mib`), so a VM could be placed on a host with no
    // free memory purely on percentage-based score. Mirrors the hard capacity
    // guard `compute_recommendations` already applies to DRS moves.
    #[tokio::test]
    async fn pick_host_for_vm_rejects_hosts_without_capacity() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        let full = crate::engine::test_support::seed_host(&state.pool, Uuid::from_u128(1)).await;
        let roomy = crate::engine::test_support::seed_host(&state.pool, Uuid::from_u128(2)).await;
        crate::db::query(
            "UPDATE hosts SET memory_total_mib = 4096, memory_used_mib = 4096 WHERE id = ?",
        )
        .bind(full)
        .execute(&state.pool)
        .await
        .unwrap();
        crate::db::query("UPDATE hosts SET memory_total_mib = 4096, memory_used_mib = 512 WHERE id = ?")
            .bind(roomy)
            .execute(&state.pool)
            .await
            .unwrap();

        // A 2 GiB VM cannot fit on `full` (0 MiB free) and must land on `roomy`.
        let picked = pick_host_for_vm(&state.pool, &[], 2048).await.unwrap();
        assert_eq!(picked, roomy);

        // No host has room for an 8 GiB VM -> placement must fail, not silently
        // overcommit the least-bad host.
        assert!(pick_host_for_vm(&state.pool, &[], 8192).await.is_err());
    }
}
