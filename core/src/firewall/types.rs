// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirewallBackend {
    Firewalld,
    Ufw,
    Nftables,
    Iptables,
    /// Enforced on-node by machina-cni.
    #[serde(alias = "cilium")]
    K8sNetworkPolicy,
    Policy,
    Unknown,
}

impl FirewallBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Firewalld => "firewalld",
            Self::Ufw => "ufw",
            Self::Nftables => "nftables",
            Self::Iptables => "iptables",
            Self::K8sNetworkPolicy => "k8s_network_policy",
            Self::Policy => "policy",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StealthLevel {
    #[default]
    Off,
    Standard,
    Strict,
    Emergency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExposureRisk {
    Safe,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallPosture {
    pub enabled: bool,
    pub backend: FirewallBackend,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub stealth_level: StealthLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_inbound: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_outbound: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_zone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_line: Option<String>,
    pub drift_detected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_changed: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenPort {
    pub port: u16,
    pub protocol: String,
    pub service_name: String,
    pub bind_address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,
    #[serde(default)]
    pub allowed_from: Vec<String>,
    pub risk: ExposureRisk,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllowedService {
    pub name: String,
    pub port: u16,
    pub protocol: String,
    pub allowed_from: String,
    pub status: ExposureRisk,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallRule {
    pub id: String,
    pub direction: String,
    pub protocol: String,
    pub ports: String,
    pub sources: Vec<String>,
    pub targets: Vec<String>,
    pub action: String,
    #[serde(default)]
    pub temporary: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallProfileRule {
    pub name: String,
    pub direction: String,
    pub protocol: String,
    pub ports: String,
    pub sources: Vec<String>,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallProfile {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub default_inbound: String,
    pub default_outbound: String,
    pub stealth_level: StealthLevel,
    pub rules: Vec<FirewallProfileRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdownItem {
    pub category: String,
    pub status: String,
    pub points: i32,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreRecommendation {
    pub label: String,
    pub points: i32,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallScore {
    pub score: u32,
    pub breakdown: Vec<ScoreBreakdownItem>,
    pub recommendations: Vec<ScoreRecommendation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallDiffEntry {
    pub change: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallDiff {
    pub entries: Vec<FirewallDiffEntry>,
    pub risk_before: ExposureRisk,
    pub risk_after: ExposureRisk,
    pub warnings: Vec<String>,
    pub rollback_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallEvent {
    pub id: String,
    pub kind: String,
    pub summary: String,
    pub source: String,
    pub target_port: u16,
    pub risk: ExposureRisk,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallActivitySummary {
    pub blocked_today: u64,
    pub allowed_today: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_blocked_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_target_port: Option<u16>,
    pub suspicious_scans: u64,
    pub new_open_ports: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallInventory {
    pub hostname: String,
    pub posture: FirewallPosture,
    pub rules: Vec<FirewallRule>,
    pub open_ports: Vec<OpenPort>,
    pub services: Vec<AllowedService>,
    pub score: FirewallScore,
    #[serde(default)]
    pub profiles_available: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nftables_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<FirewallActivitySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallPlanRequest {
    pub profile: Option<String>,
    pub enable: Option<bool>,
    pub stealth_level: Option<StealthLevel>,
    pub preset: Option<String>,
    pub dry_run: bool,
    /// Zone name → CIDR map (e.g. `admin-network` → `10.0.0.0/8`), injected by
    /// the controller from `MACHINA_FIREWALL_ZONES` just before dispatch — not
    /// meant to be populated by API callers. Resolves symbolic
    /// `FirewallProfileRule::sources` into real source restrictions; when a
    /// source isn't found here it's applied unrestricted with a warning
    /// pushed onto `FirewallPlanResult::diff.warnings` rather than silently
    /// dropped.
    #[serde(default)]
    pub zone_cidrs: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallPlanResult {
    pub diff: FirewallDiff,
    pub operations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineFirewallPolicySpec {
    pub name: String,
    pub default_inbound: String,
    pub default_outbound: String,
    pub allow: Vec<FirewallProfileRule>,
}
