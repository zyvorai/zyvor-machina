// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HaPolicy {
    pub enabled: bool,
    #[serde(default = "default_restart_attempts")]
    pub restart_attempts: u32,
    #[serde(default = "default_restart_priority")]
    pub restart_priority: String,
    #[serde(default)]
    pub anti_affinity: bool,
    #[serde(default)]
    pub fence_on_failure: bool,
}

fn default_restart_attempts() -> u32 {
    3
}
fn default_restart_priority() -> String {
    "medium".into()
}

impl Default for HaPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            restart_attempts: default_restart_attempts(),
            restart_priority: default_restart_priority(),
            anti_affinity: false,
            fence_on_failure: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlacementRecommendation {
    pub vm_id: String,
    pub vm_name: String,
    pub from_host_id: String,
    pub to_host_id: String,
    pub reason: String,
}
