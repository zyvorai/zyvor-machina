// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

pub mod api_error;
pub mod audit;
pub mod backup_replica;
pub mod audit_ship;
pub mod bpf_probe;
pub mod build_precheck;
pub mod cloud_hypervisor;
pub mod config;
pub mod firecracker;
pub mod firewall;
pub mod fleet_placement;
pub mod fluxvm;
pub mod fmt;
pub mod guest_os;
pub mod host_cockpit;
pub mod host_inventory;
pub mod host_linux_obs;
pub mod host_platform;
pub mod host_virt;
pub mod identity_settings;
pub mod iso_upload;
pub mod k3s_node;
pub mod k8s_top;
pub mod kubevirt;
pub mod ldap_role;
pub mod ldap_settings;
pub mod libvirt;
pub mod linux_audit;
pub mod metrics_history;
pub mod network;
pub mod obs_counters;
pub mod observability_settings;
pub mod oidc;
pub mod otlp;
pub mod prometheus_remote_write;
pub mod prometheus_text;
pub mod run_as_user;
pub mod sprite_net;
pub mod state;
pub mod system_accounts;
pub mod trace_context;
pub mod validate;
pub mod xml;

pub use api_error::{
    format_http_error_body, format_user_error, friendly_error_code, sanitize_error_text,
};
pub use config::{
    AuthConfig, FleetConfig, FleetPeer, FluxvmConfig, GuestkitConfig, KubeVirtConfig, LdapConfig,
    MachinaConfig, OidcConfig, OidcDefaultRole, RunAsUserConfig, SshTerminalConfig,
    SshTerminalTarget, VesselConfig, VmCreateBackend, DEFAULT_DAEMON_PORT,
};
pub use firewall::{
    apply_k8s_plan, apply_plan, builtin_profiles, cloud_sg_monthly_cost, compile_k8s_policies,
    compile_metal_plan, compile_profile_plan, compute_diff, compute_firewall_score, detect_backend,
    detect_k8s_backend, exposure_chargeback_tag, fleet_exposure_monthly, gather_cloud_inventory,
    gather_firewall_inventory, gather_metal_inventory, gpu_profile_exposure_cost,
    guest_ports_to_open_ports, idle_open_port_cost, is_cidr, is_public_bind, k8s_cluster_ready,
    metal_preset_temporary_bmc, metal_preset_temporary_pxe, mission_stack_network_cost,
    parse_zone_env, port_monthly_cost, profile_by_name, profile_exposure_multiplier,
    public_port_finops_alert, resolve_source_cidr, scan_guest_listening_ports, scan_ipmi_exposure,
    simulate_connectivity, storage_profile_exposure_cost, CloudFirewallInventory, CloudProvider,
    ConnectivityMatrix, ExposureRisk, FirewallBackend, FirewallInventory, FirewallPlanRequest,
    FirewallPlanResult, FirewallPosture, FirewallProfile, FirewallRule as ZeusFirewallRule,
    FirewallScore, GuestListeningPort, K8sPolicyManifest, MetalExposureScan, MetalServerInput,
    OpenPort, StealthLevel, GPU_EXPOSURE_MULTIPLIER, STORAGE_EXPOSURE_MULTIPLIER,
};
pub use fleet_placement::{fleet_capacity_score, placement_adjusted_score};
pub use identity_settings::{
    apply_oidc_patch, apply_saml_patch, oidc_settings_view_from_config,
    saml_settings_view_from_config, OidcSettingsPatch, OidcSettingsView, SamlSettingsPatch,
    SamlSettingsView,
};
pub use k8s_top::{parse_kubectl_top_line, K8sTopRow};
pub use kubevirt::{
    kubevirt_bundle_from_libvirt_vm, kubevirt_bundle_from_qcow2, resolve_guest_os, GuestOsFamily,
    KubeVirtBundle,
};
pub use ldap_settings::{
    apply_ldap_patch, ldap_settings_view_from_config, zyvorai_local_preset, LdapSettingsPatch,
    LdapSettingsView, LdapTestRequest, LdapTestResponse,
};
pub use libvirt::{LibvirtManager, LibvirtTarget};
pub use network::overlay::{
    compile_micro_segment_rules, default_segment_presets, ip_from_cidr_offset,
    segment_micro_seg_grade, validate_cidr, EastWestDefault, SegmentSpec, SegmentTier,
};
pub use observability_settings::{
    apply_observability_patch, settings_view_from_config, AuditObservabilityView,
    MetricsHistoryRemoteView, ObservabilitySettingsPatch, ObservabilitySettingsView,
    OtlpSettingsView,
};
pub use prometheus_remote_write::{
    decode_remote_write_body, encode_remote_write_body, encode_remote_write_v2_body,
    host_percents_from_remote_write, samples_from_write_request, write_request_v2_with_gauge,
    write_request_with_gauge, RemoteWriteDecodeResult,
};
pub use prometheus_text::{
    host_percents_from_samples, inject_peer_label, parse_prometheus_text, PrometheusSample,
};
pub use state::{
    AppState, AttachDiskRequest, AuditEvent, BackupInfo, BackupRequest, CloneVmRequest,
    ConfirmationDialog, CreateNetworkRequest, CreateSnapshotRequest, CreateVmRequest,
    CreateVolumeRequest, DashboardStats, DiskInfo, Focus, InputMode, InterfaceInfo, NetworkInfo,
    NodeInfo, NotifyLevel, ObjectTab, RenameVmRequest, ResourceView, RestoreRequest,
    SidebarCategory, SidebarItem, SnapshotInfo, SortColumn, SortDirection, StoragePoolInfo,
    StorageVolumeInfo, ViewMode, VmBlockDeviceMetrics, VmDetails, VmInfo, VmMetrics,
    VmNetDeviceMetrics, VmTemplate,
};
pub use trace_context::{format_traceparent, trace_context_from_headers, HttpTraceContext};

pub const UNKNOWN: &str = "unknown";

pub fn unknown_string() -> String {
    UNKNOWN.to_string()
}

#[derive(Debug, thiserror::Error)]
pub enum LibvirtError {
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Invalid input: {0}")]
    Invalid(String),
    #[error("Forbidden: {0}")]
    Forbidden(String),
    #[error("Operation failed: {0}")]
    Operation(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl LibvirtError {
    pub fn map_op<E: std::fmt::Display>(msg: &str) -> impl FnOnce(E) -> Self + '_ {
        move |e| Self::Operation(format!("{msg}: {e}"))
    }
}
