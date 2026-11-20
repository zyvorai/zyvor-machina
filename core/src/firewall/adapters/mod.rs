// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::types::{FirewallPosture, FirewallRule, StealthLevel};
use crate::LibvirtError;

pub trait FirewallAdapter {
    fn read_posture(&self) -> Result<FirewallPosture, LibvirtError>;
    fn read_rules(&self) -> Result<Vec<FirewallRule>, LibvirtError>;
    fn snapshot_state(&self) -> Result<serde_json::Value, LibvirtError>;
}

pub mod firewalld;
pub mod iptables;
pub mod k8s;
pub mod nftables;
pub mod ufw;

pub fn adapter_for(backend: super::types::FirewallBackend) -> Box<dyn FirewallAdapter> {
    match backend {
        super::types::FirewallBackend::Firewalld => Box::new(firewalld::FirewalldAdapter),
        super::types::FirewallBackend::Ufw => Box::new(ufw::UfwAdapter),
        super::types::FirewallBackend::Nftables => Box::new(nftables::NftablesAdapter),
        super::types::FirewallBackend::K8sNetworkPolicy => Box::new(k8s::K8sNetworkPolicyAdapter),
        _ => Box::new(iptables::IptablesAdapter),
    }
}

pub fn default_posture(backend: super::types::FirewallBackend) -> FirewallPosture {
    FirewallPosture {
        enabled: false,
        backend,
        profile: None,
        stealth_level: StealthLevel::Off,
        default_inbound: None,
        default_outbound: None,
        backend_zone: None,
        status_line: None,
        drift_detected: false,
        last_changed: None,
    }
}
