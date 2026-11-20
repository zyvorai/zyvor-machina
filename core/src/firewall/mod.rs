// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Zeus Firewall — unified machine firewall abstraction.

pub mod adapters;
pub mod apply;
pub mod cloud;
pub mod connectivity;
pub mod detect;
pub mod diff;
pub mod finops;
pub mod guest_ports;
pub mod inventory;
pub mod k8s_apply;
pub mod metal;
pub mod profiles;
pub mod scan;
pub mod score;
pub mod types;
pub mod zones;

pub use apply::{apply_plan, compile_profile_plan};
pub use cloud::{
    gather_cloud_inventory, CloudFirewallInventory, CloudProvider, CloudSecurityGroupRule,
};
pub use connectivity::{simulate_connectivity, ConnectivityMatrix};
pub use detect::detect_backend;
pub use diff::compute_diff;
pub use finops::{
    cloud_sg_monthly_cost, exposure_chargeback_tag, fleet_exposure_monthly,
    gpu_profile_exposure_cost, idle_open_port_cost, is_public_bind, mission_stack_network_cost,
    port_monthly_cost, profile_exposure_multiplier, public_port_finops_alert,
    storage_profile_exposure_cost, GPU_EXPOSURE_MULTIPLIER, IDLE_PORT_MONTHLY_USD,
    STORAGE_EXPOSURE_MULTIPLIER,
};
pub use guest_ports::{guest_ports_to_open_ports, scan_guest_listening_ports, GuestListeningPort};
pub use inventory::gather_firewall_inventory;
pub use k8s_apply::{
    apply_k8s_plan, compile_k8s_policies, detect_k8s_backend, k8s_cluster_ready, K8sPolicyManifest,
};
pub use metal::{
    compile_metal_plan, gather_metal_inventory, metal_preset_temporary_bmc,
    metal_preset_temporary_pxe, scan_ipmi_exposure, MetalExposureScan, MetalServerInput,
};
pub use profiles::{builtin_profiles, profile_by_name};
pub use score::compute_firewall_score;
pub use types::*;
pub use zones::{is_cidr, parse_zone_env, resolve_source_cidr};
