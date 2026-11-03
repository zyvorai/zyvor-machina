// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Migration Wave Copilot: proposes an order and batches for a migration wave.
//!
//! Rules, all visible in the output so a person can disagree:
//! - machines that are off go first (cold copies are cheap and cannot fail on dirty memory);
//! - running machines follow, smallest memory first (shortest copy, quickest feedback);
//! - a batch never holds two running machines of the same application group, so a group is never
//!   moved all at once;
//! - a batch holds at most `max_parallel` machines.
//!
//! It does not call the convergence oracle or the noisy-neighbour graph yet. Read-only: the result is
//! a suggestion for `POST /api/v1/migration-waves`, which the person still creates.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::db::DbPool;

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub name: String,
    pub memory_mib: i64,
    pub running: bool,
    pub groups: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AdviseRequest {
    pub vm_names: Vec<String>,
    #[serde(default)]
    pub max_parallel: Option<u32>,
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
    pub notes: Vec<String>,
}

pub fn plan(mut vms: Vec<Candidate>, max_parallel: u32) -> Vec<Batch> {
    let cap = max_parallel.max(1) as usize;
    // Off first, then by memory, then by name so the order is stable.
    vms.sort_by(|a, b| {
        a.running
            .cmp(&b.running)
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
            let why = if running == 0 {
                "Machines that are off: cold copies, lowest risk.".to_string()
            } else if running == members.len() {
                "Running machines, smallest memory first, no two from the same application group.".to_string()
            } else {
                "Off machines fill the batch alongside running ones from different groups.".to_string()
            };
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

    let mut found = Vec::new();
    let mut unknown = Vec::new();
    let mut seen = HashSet::new();
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
            running: state == "running",
            groups,
        });
    }

    let mut notes = vec![format!("Up to {max_parallel} machines per batch.")];
    if !unknown.is_empty() {
        notes.push(format!("{} name(s) were not found and are left out.", unknown.len()));
    }
    Ok(WaveAdvice {
        max_parallel,
        batches: plan(found, max_parallel),
        unknown,
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
        }
    }

    #[test]
    fn off_machines_go_first_then_small_running_ones() {
        let b = plan(
            vec![c("big", 8192, true, &[]), c("off", 4096, false, &[]), c("small", 1024, true, &[])],
            10,
        );
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].vms, vec!["off", "small", "big"]);
    }

    #[test]
    fn a_batch_never_holds_two_running_machines_of_one_group() {
        let b = plan(
            vec![c("a1", 1024, true, &["shop"]), c("a2", 2048, true, &["shop"]), c("x", 512, true, &["blog"])],
            10,
        );
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].vms, vec!["x", "a1"]);
        assert_eq!(b[1].vms, vec!["a2"]);
    }

    #[test]
    fn batches_respect_max_parallel() {
        let vms: Vec<_> = (0..5).map(|i| c(&format!("v{i}"), 1024 + i, true, &[])).collect();
        let b = plan(vms, 2);
        assert_eq!(b.iter().map(|x| x.vms.len()).collect::<Vec<_>>(), vec![2, 2, 1]);
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
        let req = AdviseRequest { vm_names: vec!["nope".into(), "nope".into()], max_parallel: Some(2) };
        let a = advise(&pool, &req).await.unwrap();
        assert_eq!(a.unknown, vec!["nope"]);
        assert!(a.batches.is_empty());
        assert!(advise(&pool, &AdviseRequest { vm_names: vec![], max_parallel: None }).await.is_err());
    }
}
