// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Line protocol between machina-bpfd and its `machina-scx` child: commands on
//! stdin, one JSON object per line on stdout. Closing stdin stops the
//! scheduler (the kernel moves its tasks back to CFS).

use serde::{Deserialize, Serialize};

pub const STATE_PATH: &str = "/sys/kernel/sched_ext/state";

/// One vCPU thread handed to the scheduler.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub tid: u32,
    pub tgid: u32,
    pub vm: u64,
    #[serde(default = "default_weight")]
    pub weight: u32,
    #[serde(default = "default_slice")]
    pub slice_ns: u64,
    #[serde(default)]
    pub latency_target_ns: u64,
}

fn default_weight() -> u32 {
    100
}

fn default_slice() -> u64 {
    1_000_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
    /// Replace the full profile set.
    Profiles { profiles: Vec<Profile> },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmStats {
    pub enqueues: u64,
    pub direct_dispatches: u64,
    pub shared_dispatches: u64,
    pub running_calls: u64,
    pub runtime_ns: u64,
    pub queue_delay_ns: u64,
    pub queue_delay_max_ns: u64,
    pub latency_violations: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Output {
    Ready {
        ready: bool,
    },
    Error {
        error: String,
    },
    /// (vm key, stats); a list because untagged enums cannot take integer map keys.
    Stats {
        stats: Vec<(u64, VmStats)>,
        exit_kind: u64,
    },
}

pub fn kernel_state() -> Option<String> {
    std::fs::read_to_string(STATE_PATH)
        .ok()
        .map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_roundtrip() {
        let c: Command =
            serde_json::from_str(r#"{"op":"profiles","profiles":[{"tid":5,"tgid":4,"vm":1}]}"#)
                .unwrap();
        let Command::Profiles { profiles } = c;
        assert_eq!(profiles[0].weight, 100);
        assert_eq!(profiles[0].slice_ns, 1_000_000);
        let o: Output = serde_json::from_str(r#"{"ready":true}"#).unwrap();
        assert_eq!(o, Output::Ready { ready: true });
        let stats = vec![(
            3,
            VmStats {
                enqueues: 9,
                ..Default::default()
            },
        )];
        let s = serde_json::to_string(&Output::Stats {
            stats: stats.clone(),
            exit_kind: 0,
        })
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Output>(&s).unwrap(),
            Output::Stats {
                stats,
                exit_kind: 0
            }
        );
        assert!(matches!(
            serde_json::from_str::<Output>(r#"{"error":"x"}"#).unwrap(),
            Output::Error { .. }
        ));
    }
}
