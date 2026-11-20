// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum HostState {
    #[default]
    Unknown,
    Online,
    Offline,
    Maintenance,
    Draining,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostRegistration {
    pub hostname: String,
    pub address: String,
    pub agent_version: String,
    pub libvirt_uri: String,
    #[serde(default)]
    pub cluster_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostHeartbeat {
    pub host_id: String,
    pub state: HostState,
    pub cpu_percent: f32,
    pub memory_used_mib: u64,
    pub memory_total_mib: u64,
    pub vm_count: u32,
}
