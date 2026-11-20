// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    Podman,
    Docker,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineCapabilities {
    pub pods: bool,
    pub logs: bool,
    pub stats: bool,
}

impl EngineCapabilities {
    pub fn for_engine(kind: &EngineKind) -> Self {
        Self {
            pods: matches!(kind, EngineKind::Podman),
            logs: true,
            stats: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    pub name: String,
    pub engine: EngineKind,
    pub version: String,
    pub os: String,
    pub arch: String,
    pub cpus: u32,
    pub memory_total: u64,
    pub capabilities: EngineCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ContainerStatus {
    Created,
    Running,
    Paused,
    Restarting,
    Removing,
    Exited,
    Dead,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMapping {
    pub ip: Option<String>,
    pub private_port: u16,
    pub public_port: Option<u16>,
    pub typ: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerSummary {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: ContainerStatus,
    pub state: String,
    pub created: DateTime<Utc>,
    pub ports: Vec<PortMapping>,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStats {
    pub cpu_percent: f64,
    pub memory_usage: u64,
    pub memory_limit: u64,
    pub network_rx: u64,
    pub network_tx: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PodStatus {
    Created,
    Running,
    Stopped,
    Exited,
    Degraded,
    Dead,
    Paused,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodSummary {
    pub id: String,
    pub name: String,
    pub status: PodStatus,
    pub infra_container_id: Option<String>,
    pub containers: Vec<String>,
    pub created: DateTime<Utc>,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePodRequest {
    pub name: String,
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePodResponse {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateContainerRequest {
    pub name: String,
    pub image: String,
    #[serde(default)]
    pub command: Vec<String>,
    /// Start immediately after create (default true).
    #[serde(default = "default_start_container")]
    pub start: bool,
}

fn default_start_container() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateContainerResponse {
    pub id: String,
    pub name: String,
}

pub fn parse_container_status(state: &str) -> ContainerStatus {
    let s = state.to_ascii_lowercase();
    if s.contains("running") {
        ContainerStatus::Running
    } else if s.contains("paused") {
        ContainerStatus::Paused
    } else if s.contains("restarting") {
        ContainerStatus::Restarting
    } else if s.contains("removing") {
        ContainerStatus::Removing
    } else if s.contains("dead") {
        ContainerStatus::Dead
    } else if s.contains("created") {
        ContainerStatus::Created
    } else if s.contains("exited") || s.contains("stopped") {
        ContainerStatus::Exited
    } else {
        ContainerStatus::Unknown
    }
}

pub fn parse_pod_status(status: &str) -> PodStatus {
    match status.to_ascii_lowercase().as_str() {
        "created" => PodStatus::Created,
        "running" => PodStatus::Running,
        "stopped" => PodStatus::Stopped,
        "exited" => PodStatus::Exited,
        "degraded" => PodStatus::Degraded,
        "dead" => PodStatus::Dead,
        "paused" => PodStatus::Paused,
        _ => PodStatus::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_running_and_exited() {
        assert_eq!(parse_container_status("running"), ContainerStatus::Running);
        assert_eq!(
            parse_container_status("Exited (0) 2 days ago"),
            ContainerStatus::Exited
        );
        assert_eq!(parse_pod_status("Degraded"), PodStatus::Degraded);
    }
}
