// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Migration Order Advisor: proposes an order and batches for moving machines that already run in Machina.
//!
//! Rules, all visible in the output so a person can disagree:
//! - machines that are off go first (cold copies are cheap and cannot fail on dirty memory);
//! - running machines follow, smallest memory first (shortest copy, quickest feedback);
//! - a batch never holds two running machines of the same application group, so a group is never
//!   moved all at once;
//! - a batch holds at most `max_parallel` machines.
//!
//! Optional, off unless asked:
//! - with `dest_host_id`, each machine goes through the convergence oracle for that destination. A machine
//!   that fails the precheck or will not converge is held out of the batches with the reason; of the rest,
//!   the safer predictions move first;
//! - with `use_interference`, machines that are noisy neighbours of others (per the noisy-neighbour graph)
//!   move earlier within the same convergence rank, so their victims get relief sooner.
//!
//! If either source cannot be read the advice says so and falls back to the base rules.
//!
//! It only knows machines already in Machina (not machines still being imported through Transiva) and is
//! read-only: it creates nothing.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::db::DbPool;

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub name: String,
    pub memory_mib: i64,
    pub running: bool,
    pub groups: Vec<String>,
    /// The oracle's verdict for the chosen destination, when one was asked for.
    pub convergence: Option<String>,
    /// Machines this one is a noisy neighbour of, when interference data was asked for.
    pub aggressor_of: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AdviseRequest {
    pub vm_names: Vec<String>,
    #[serde(default)]
    pub max_parallel: Option<u32>,
    /// Run each machine through the convergence oracle for this destination host.
    #[serde(default)]
    pub dest_host_id: Option<uuid::Uuid>,
    /// Move noisy neighbours earlier. Reads fleet telemetry, so it is opt-in.
    #[serde(default)]
    pub use_interference: bool,
}

/// A machine kept out of every batch, and why.
#[derive(Debug, Serialize, PartialEq)]
pub struct Held {
    pub vm: String,
    pub reason: String,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Batch {
    pub step: usize,
    pub vms: Vec<String>,
    pub why: String,
}

#[derive(Debug, Serialize)]
pub struct WaveAdvice {
    pub max_parallel: u32,
    pub batches: Vec<Batch>,
    /// Names that were not found, so they are not in any batch.
    pub unknown: Vec<String>,
    /// Machines the oracle says should not be moved yet.
    pub held: Vec<Held>,
    pub notes: Vec<String>,
}

/// 0 = safe to go early, higher = later. Unknown or absent verdicts count as safe.
fn convergence_rank(c: Option<&str>) -> u8 {
    match c {
        Some("marginal") => 1,
        Some("high_risk") => 2,
        _ => 0,
    }
}

/// Why a machine should be held back, from the oracle's precheck and verdict. `None` means it may go.
pub fn hold_reason(
    precheck_ok: bool,
    convergence: &str,
    first_risk: Option<&str>,
) -> Option<String> {
    if !precheck_ok {
        // The oracle lists failed precheck checks first, as "Precheck '<name>' failed: ...". Quote one of those;
        // any other first risk (a dirty-rate warning, say) is not the reason the precheck failed.
        return Some(match first_risk.filter(|r| r.starts_with("Precheck '")) {
            Some(r) => r.to_string(),
            None => "Migration precheck failed.".to_string(),
        });
    }
    match convergence {
        "blocked" => Some("No usable migration bandwidth to the destination.".to_string()),
        "will_not_converge" => Some(
            "Pre-copy will not converge: memory is dirtied faster than it can be copied. Stop it first, add bandwidth, or use post-copy."
                .to_string(),
        ),
        _ => None,
    }
}

pub fn plan(mut vms: Vec<Candidate>, max_parallel: u32) -> Vec<Batch> {
    let cap = max_parallel.max(1) as usize;
    // Off first, then by memory, then by name so the order is stable.
    vms.sort_by(|a, b| {
        a.running
            .cmp(&b.running)
            .then(
                convergence_rank(a.convergence.as_deref())
                    .cmp(&convergence_rank(b.convergence.as_deref())),
            )
            // Noisy neighbours first within a rank: non-empty (false) sorts before empty (true).
            .then(a.aggressor_of.is_empty().cmp(&b.aggressor_of.is_empty()))
            .then(a.memory_mib.cmp(&b.memory_mib))
            .then(a.name.cmp(&b.name))
    });
    let mut batches: Vec<(Vec<Candidate>, HashSet<String>)> = Vec::new();
    for vm in vms {
        let slot = batches.iter().position(|(members, groups)| {
            members.len() < cap && (!vm.running || vm.groups.iter().all(|g| !groups.contains(g)))
        });
        // A batch of only-off machines can take anyone off; keep running machines apart by group.
        let idx = match slot {
            Some(i) => i,
            None => {
                batches.push((Vec::new(), HashSet::new()));
                batches.len() - 1
            }
        };
        if vm.running {
            batches[idx].1.extend(vm.groups.iter().cloned());
        }
        batches[idx].0.push(vm);
    }
    batches
        .into_iter()
        .enumerate()
        .map(|(i, (members, _))| {
            let running = members.iter().filter(|m| m.running).count();
            let aggressors: Vec<&str> = members
                .iter()
                .filter(|m| !m.aggressor_of.is_empty())
                .map(|m| m.name.as_str())
                .collect();
            let careful = members
                .iter()
                .any(|m| convergence_rank(m.convergence.as_deref()) > 0);
            let mut why = if running == 0 {
                "Machines that are off: cold copies, lowest risk.".to_string()
            } else if running == members.len() {
                "Running machines, smallest memory first, no two from the same application group.".to_string()
            } else {
                "Off machines fill the batch alongside running ones from different groups.".to_string()
            };
            if !aggressors.is_empty() {
                why.push_str(&format!(" Moved early because they disturb other machines: {}.", aggressors.join(", ")));
            }
            if careful {
                why.push_str(" Includes machines the oracle rates marginal or high risk; consider a quiet period or more bandwidth.");
            }
            Batch {
                step: i + 1,
                vms: members.into_iter().map(|m| m.name).collect(),
                why,
            }
        })
        .collect()
}

pub async fn advise(pool: &DbPool, req: &AdviseRequest) -> anyhow::Result<WaveAdvice> {
    anyhow::ensure!(!req.vm_names.is_empty(), "vm_names must not be empty");
    anyhow::ensure!(req.vm_names.len() <= 200, "at most 200 machines per wave");
    let max_parallel = req.max_parallel.unwrap_or(3).clamp(1, 20);
    if let Some(dest) = req.dest_host_id {
        let known: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts WHERE id = ?")
            .bind(dest)
            .fetch_one(pool)
            .await?;
        anyhow::ensure!(known > 0, "destination host not found");
    }

    let mut found = Vec::new();
    let mut unknown = Vec::new();
    let mut held = Vec::new();
    let mut notes = vec![format!("Up to {max_parallel} machines per batch.")];
    let mut seen = HashSet::new();
    let mut oracle_failed = 0usize;
    for name in &req.vm_names {
        if !seen.insert(name.clone()) {
            continue;
        }
        let row: Option<(uuid::Uuid, i64, String)> = crate::db::query_as(
            "SELECT id, memory_mib, observed_state FROM vms WHERE name = ? LIMIT 1",
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;
        let Some((id, memory_mib, state)) = row else {
            unknown.push(name.clone());
            continue;
        };
        let running = state == "running";
        let mut convergence = None;
        if let Some(dest) = req.dest_host_id {
            let oracle_req = crate::engine::migration_oracle::MigrationOracleRequest {
                dest_host_id: dest,
                live: running,
                dirty_rate_mib_s: None,
                bandwidth_mbps: None,
                max_downtime_ms: None,
            };
            match crate::engine::migration_oracle::predict(pool, id, &oracle_req).await {
                Ok(rep) => {
                    if let Some(reason) = hold_reason(
                        rep.precheck_ok,
                        if running { &rep.convergence } else { "" },
                        rep.risks.first().map(String::as_str),
                    ) {
                        held.push(Held {
                            vm: name.clone(),
                            reason,
                        });
                        continue;
                    }
                    if running {
                        convergence = Some(rep.convergence);
                    }
                }
                Err(_) => {
                    oracle_failed += 1;
                }
            }
        }
        let groups: Vec<String> = crate::db::query_scalar(
            "SELECT g.name FROM application_group_vms agv
             JOIN application_groups g ON g.id = agv.group_id WHERE agv.vm_id = ?",
        )
        .bind(id)
        .fetch_all(pool)
        .await?;
        found.push(Candidate {
            name: name.clone(),
            memory_mib,
            running,
            groups,
            convergence,
            aggressor_of: Vec::new(),
        });
    }

    if req.dest_host_id.is_some() {
        if oracle_failed > 0 {
            notes.push(format!(
                "The convergence oracle could not assess {oracle_failed} machine(s) (for example no source host); they are ordered by the base rules."
            ));
        } else {
            notes.push(
                "Each machine was checked with the convergence oracle for the chosen destination."
                    .to_string(),
            );
        }
    }
    if req.use_interference {
        let q = crate::engine::ai::noisy_neighbor::NoisyNeighborQuery {
            host_id: None,
            victim_vm: None,
            limit: None,
        };
        match crate::engine::ai::noisy_neighbor::analyze(pool, &q).await {
            Ok(report) => {
                let mut n = 0;
                for c in &mut found {
                    let victims: Vec<String> = report
                        .top_edges
                        .iter()
                        .filter(|e| e.aggressor == c.name)
                        .map(|e| e.victim.clone())
                        .collect();
                    if !victims.is_empty() {
                        n += 1;
                    }
                    c.aggressor_of = victims;
                }
                notes.push(format!(
                    "Noisy-neighbour data: {n} machine(s) in this wave disturb others and move earlier."
                ));
            }
            Err(e) => notes.push(format!(
                "Noisy-neighbour data was not available ({e}); the base order is used."
            )),
        }
    }
    if !unknown.is_empty() {
        notes.push(format!(
            "{} name(s) were not found and are left out.",
            unknown.len()
        ));
    }
    if !held.is_empty() {
        notes.push(format!(
            "{} machine(s) are held back; see `held`.",
            held.len()
        ));
    }
    Ok(WaveAdvice {
        max_parallel,
        batches: plan(found, max_parallel),
        unknown,
        held,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str, mem: i64, running: bool, groups: &[&str]) -> Candidate {
        Candidate {
            name: name.into(),
            memory_mib: mem,
            running,
            groups: groups.iter().map(|g| g.to_string()).collect(),
            convergence: None,
            aggressor_of: Vec::new(),
        }
    }

    fn with_conv(mut c: Candidate, conv: &str) -> Candidate {
        c.convergence = Some(conv.to_string());
        c
    }

    fn aggressor(mut c: Candidate, victim: &str) -> Candidate {
        c.aggressor_of = vec![victim.to_string()];
        c
    }

    #[test]
    fn off_machines_go_first_then_small_running_ones() {
        let b = plan(
            vec![
                c("big", 8192, true, &[]),
                c("off", 4096, false, &[]),
                c("small", 1024, true, &[]),
            ],
            10,
        );
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].vms, vec!["off", "small", "big"]);
    }

    #[test]
    fn a_batch_never_holds_two_running_machines_of_one_group() {
        let b = plan(
            vec![
                c("a1", 1024, true, &["shop"]),
                c("a2", 2048, true, &["shop"]),
                c("x", 512, true, &["blog"]),
            ],
            10,
        );
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].vms, vec!["x", "a1"]);
        assert_eq!(b[1].vms, vec!["a2"]);
    }

    #[test]
    fn batches_respect_max_parallel() {
        let vms: Vec<_> = (0..5)
            .map(|i| c(&format!("v{i}"), 1024 + i, true, &[]))
            .collect();
        let b = plan(vms, 2);
        assert_eq!(
            b.iter().map(|x| x.vms.len()).collect::<Vec<_>>(),
            vec![2, 2, 1]
        );
        assert_eq!(b[2].step, 3);
    }

    #[test]
    fn zero_parallel_is_treated_as_one() {
        let b = plan(vec![c("a", 1, false, &[]), c("b", 2, false, &[])], 0);
        assert_eq!(b.len(), 2);
    }

    #[test]
    fn empty_input_gives_no_batches() {
        assert!(plan(vec![], 3).is_empty());
    }

    #[tokio::test]
    async fn unknown_machines_are_reported_not_planned() {
        let pool = crate::db::testing::pool().await;
        let req = AdviseRequest {
            vm_names: vec!["nope".into(), "nope".into()],
            max_parallel: Some(2),
            dest_host_id: None,
            use_interference: false,
        };
        let a = advise(&pool, &req).await.unwrap();
        assert_eq!(a.unknown, vec!["nope"]);
        assert!(a.batches.is_empty());
        let empty = AdviseRequest {
            vm_names: vec![],
            max_parallel: None,
            dest_host_id: None,
            use_interference: false,
        };
        assert!(advise(&pool, &empty).await.is_err());
    }

    #[test]
    fn safer_convergence_moves_first_among_running_machines() {
        let b = plan(
            vec![
                with_conv(c("risky", 512, true, &[]), "high_risk"),
                with_conv(c("meh", 512, true, &[]), "marginal"),
                with_conv(c("solid", 4096, true, &[]), "strong"),
            ],
            10,
        );
        assert_eq!(b[0].vms, vec!["solid", "meh", "risky"]);
        assert!(b[0].why.contains("marginal or high risk"));
    }

    #[test]
    fn noisy_neighbours_move_earlier_within_a_rank_but_not_ahead_of_safer_machines() {
        let b = plan(
            vec![
                c("quiet-small", 512, true, &[]),
                aggressor(c("loud-big", 8192, true, &[]), "victim"),
                with_conv(
                    aggressor(c("loud-risky", 256, true, &[]), "victim"),
                    "high_risk",
                ),
            ],
            10,
        );
        // loud-big beats quiet-small despite its size; loud-risky stays behind both (worse rank).
        assert_eq!(b[0].vms, vec!["loud-big", "quiet-small", "loud-risky"]);
        assert!(b[0].why.contains("disturb other machines"));
    }

    #[test]
    fn hold_reason_follows_the_precheck_and_the_verdict() {
        assert!(hold_reason(
            false,
            "strong",
            Some("Precheck 'destination' failed: destination is in maintenance")
        )
        .unwrap()
        .contains("maintenance"));
        // An unrelated first risk is not passed off as the precheck's reason.
        assert_eq!(
            hold_reason(
                false,
                "strong",
                Some("Dirty-page rate is at or above the transfer rate")
            )
            .unwrap(),
            "Migration precheck failed."
        );
        assert_eq!(
            hold_reason(false, "strong", None).unwrap(),
            "Migration precheck failed."
        );
        assert!(hold_reason(true, "will_not_converge", None)
            .unwrap()
            .contains("will not converge"));
        assert!(hold_reason(true, "blocked", None).is_some());
        assert!(hold_reason(true, "high_risk", None).is_none());
        assert!(hold_reason(true, "strong", None).is_none());
        // A machine that is off passes an empty verdict: only the precheck can hold it.
        assert!(hold_reason(true, "", None).is_none());
    }

    #[tokio::test]
    async fn an_oracle_that_cannot_assess_falls_back_and_says_so() {
        let pool = crate::db::testing::pool().await;
        let vm_id = uuid::Uuid::new_v4();
        crate::db::query(
            "INSERT INTO vms (id, name, project, spec_json, desired_state, lifecycle_phase, vcpus, memory_mib, tags)
             VALUES (?, 'no-host-vm', 'p', '{}', 'running', 'running', 1, 512, '[]')",
        )
        .bind(vm_id)
        .execute(&pool)
        .await
        .unwrap();
        let dest = uuid::Uuid::new_v4();
        crate::db::query("INSERT INTO hosts (id, hostname, state) VALUES (?, 'dest', 'online')")
            .bind(dest)
            .execute(&pool)
            .await
            .unwrap();
        let req = AdviseRequest {
            vm_names: vec!["no-host-vm".into()],
            max_parallel: Some(2),
            dest_host_id: Some(dest),
            use_interference: false,
        };
        let a = advise(&pool, &req).await.unwrap();
        assert_eq!(
            a.batches.len(),
            1,
            "an unassessable machine is still planned"
        );
        assert!(a.held.is_empty());
        assert!(a.notes.iter().any(|n| n.contains("could not assess")));
    }

    #[tokio::test]
    async fn an_unknown_destination_host_is_refused() {
        let pool = crate::db::testing::pool().await;
        let req = AdviseRequest {
            vm_names: vec!["whatever".into()],
            max_parallel: None,
            dest_host_id: Some(uuid::Uuid::new_v4()),
            use_interference: false,
        };
        let err = advise(&pool, &req).await.unwrap_err().to_string();
        assert!(err.contains("destination host not found"), "{err}");
    }
}
