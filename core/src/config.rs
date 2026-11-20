// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MachinaConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub libvirt: LibvirtConfig,
    #[serde(default)]
    pub backup: BackupConfig,
    #[serde(default)]
    pub tls: TlsConfig,
    /// PAM service name (file in `/etc/pam.d/`) for web UI and API session login.
    #[serde(default)]
    pub auth: AuthConfig,
    /// Browser SSH terminal: short-lived sessions, optional host allowlist (`targets`), PTY + system `ssh`.
    #[serde(default)]
    pub ssh_terminal: SshTerminalConfig,
    /// Defaults for `GET /api/v1/vms/{name}/kubevirt-bundle` (libvirt qcow2 → KubeVirt manifest generation).
    #[serde(default)]
    pub kubevirt: KubeVirtConfig,
    /// Optional FluxVM backend (`fluxvm-api`) alongside libvirt.
    #[serde(default)]
    pub fluxvm: FluxvmConfig,
    /// Optional GuestKit worker proxy (`/api/v1/guestkit/*`) for offline disk assurance jobs.
    #[serde(default)]
    pub guestkit: GuestkitConfig,
    /// Local Podman/Docker container engine (Vessel).
    #[serde(default)]
    pub vessel: VesselConfig,
    /// Periodic snapshots of host hardware inventory (JSON Lines under `/var/lib/machina/hardware-inventory.jsonl`).
    #[serde(default)]
    pub inventory_history: InventoryHistoryConfig,
    /// Optional append-only JSON Lines of Kubernetes cluster inventory (`/var/lib/machina/k8s-cluster-inventory.jsonl`).
    #[serde(default)]
    pub k8s_inventory_history: K8sInventoryHistoryConfig,
    /// Optional multi-node Machina daemons (aggregate health + VM lists; proxy lifecycle when configured).
    #[serde(default)]
    pub fleet: FleetConfig,
    /// In-memory ring buffer of host + VM metric samples (served at `GET /api/v1/metrics/history`).
    #[serde(default)]
    pub metrics_history: MetricsHistoryConfig,
    /// Persistent audit log rotation under `/var/lib/machina/audit.log`.
    #[serde(default)]
    pub audit: AuditLogConfig,
    /// OTLP export, Linux auditd probes, and related integrations.
    #[serde(default)]
    pub observability: ObservabilityConfig,
}

/// OpenTelemetry Protocol (HTTP) export and host audit integration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ObservabilityConfig {
    #[serde(default)]
    pub otlp: OtlpExportConfig,
    #[serde(default)]
    pub linux_audit: LinuxAuditConfig,
}

/// Push metrics (and optional audit logs) to an OTLP/HTTP collector (e.g. Grafana Alloy, otelcol).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpExportConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Base URL, e.g. `http://127.0.0.1:4318` (paths `/v1/metrics` and `/v1/logs` are appended).
    #[serde(default)]
    pub endpoint: String,
    #[serde(default = "default_otlp_interval_secs")]
    pub interval_secs: u64,
    #[serde(default = "default_otlp_export_metrics")]
    pub export_metrics: bool,
    #[serde(default = "default_otlp_export_logs")]
    pub export_logs: bool,
    #[serde(default = "default_otlp_export_traces")]
    pub export_traces: bool,
    /// Optional `Authorization` header value (e.g. `Bearer …`).
    #[serde(default)]
    pub authorization: String,
}

fn default_otlp_interval_secs() -> u64 {
    60
}

fn default_otlp_export_metrics() -> bool {
    true
}

fn default_otlp_export_logs() -> bool {
    true
}

fn default_otlp_export_traces() -> bool {
    true
}

impl Default for OtlpExportConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
            interval_secs: default_otlp_interval_secs(),
            export_metrics: default_otlp_export_metrics(),
            export_logs: default_otlp_export_logs(),
            export_traces: default_otlp_export_traces(),
            authorization: String::new(),
        }
    }
}

impl OtlpExportConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled && !self.endpoint.trim().is_empty()
    }
}

/// Read recent Linux auditd events (`/var/log/audit/audit.log` or `ausearch`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinuxAuditConfig {
    #[serde(default = "default_linux_audit_enabled")]
    pub enabled: bool,
    #[serde(default = "default_linux_audit_max_events")]
    pub max_events: usize,
    /// Emit `/health/problems` when AVC count in the probe window exceeds this (0 = off).
    #[serde(default)]
    pub health_avc_threshold: u32,
}

fn default_linux_audit_enabled() -> bool {
    true
}

fn default_linux_audit_max_events() -> usize {
    200
}

impl Default for LinuxAuditConfig {
    fn default() -> Self {
        Self {
            enabled: default_linux_audit_enabled(),
            max_events: default_linux_audit_max_events(),
            health_avc_threshold: 0,
        }
    }
}

/// Rotation and optional remote shipping for `/var/lib/machina/audit.log`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogConfig {
    /// Rotate when the active log exceeds this size (0 = no rotation).
    #[serde(default = "default_audit_max_file_mb")]
    pub max_file_mb: u64,
    /// Number of rotated files to retain (`audit.log.1` … `audit.log.N`).
    #[serde(default = "default_audit_rotate_keep")]
    pub rotate_keep: u32,
    /// Mirror each event to syslog (`LOG_AUTHPRIV`).
    #[serde(default)]
    pub syslog_enabled: bool,
    /// POST JSON `AuditEvent` to this URL (S3 pre-signed PUT, Loki, Splunk HEC, etc.).
    #[serde(default)]
    pub http_webhook_url: String,
    /// Optional `Authorization` header for the webhook.
    #[serde(default)]
    pub webhook_authorization: String,
    /// Append `sha256:<hex>` prefix to each audit line for tamper detection.
    #[serde(default)]
    pub sign_lines: bool,
}

fn default_audit_max_file_mb() -> u64 {
    64
}

fn default_audit_rotate_keep() -> u32 {
    5
}

impl Default for AuditLogConfig {
    fn default() -> Self {
        Self {
            max_file_mb: default_audit_max_file_mb(),
            rotate_keep: default_audit_rotate_keep(),
            syslog_enabled: false,
            http_webhook_url: String::new(),
            webhook_authorization: String::new(),
            sign_lines: false,
        }
    }
}

/// Ring buffer of recent host/VM metrics; optional JSON Lines persistence across restarts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsHistoryConfig {
    #[serde(default = "default_metrics_history_enabled")]
    pub enabled: bool,
    /// Seconds between samples (minimum 15 when enabled).
    #[serde(default = "default_metrics_history_interval_secs")]
    pub interval_secs: u64,
    /// Maximum points retained in memory (oldest dropped).
    #[serde(default = "default_metrics_history_max_points")]
    pub max_points: usize,
    /// Append samples to `/var/lib/machina/metrics-history.jsonl`.
    #[serde(default = "default_metrics_history_persist")]
    pub persist: bool,
    /// Trim persisted file when it exceeds this size (only when `persist` is true).
    #[serde(default = "default_metrics_history_max_file_mb")]
    pub max_file_mb: u64,
    /// Optional POST destination for metrics history samples (Machina JSON `MetricsHistoryPoint`).
    /// For Prometheus Snappy protobuf, POST to `/api/v1/metrics/ingest/remote-write` instead.
    #[serde(default)]
    pub remote_write_url: String,
    #[serde(default)]
    pub remote_write_authorization: String,
}

fn default_metrics_history_enabled() -> bool {
    true
}

fn default_metrics_history_interval_secs() -> u64 {
    30
}

fn default_metrics_history_max_points() -> usize {
    120
}

fn default_metrics_history_persist() -> bool {
    true
}

fn default_metrics_history_max_file_mb() -> u64 {
    32
}

impl Default for MetricsHistoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_metrics_history_enabled(),
            interval_secs: default_metrics_history_interval_secs(),
            max_points: default_metrics_history_max_points(),
            persist: default_metrics_history_persist(),
            max_file_mb: default_metrics_history_max_file_mb(),
            remote_write_url: String::new(),
            remote_write_authorization: String::new(),
        }
    }
}

/// Snapshots from `GET /k8s/cluster-inventory` for drift / audit (same machine as other Machina state).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sInventoryHistoryConfig {
    #[serde(default = "default_k8s_inv_hist_enabled")]
    pub enabled: bool,
    #[serde(default = "default_k8s_inv_hist_max_mb")]
    pub max_file_mb: u64,
}

fn default_k8s_inv_hist_enabled() -> bool {
    false
}

fn default_k8s_inv_hist_max_mb() -> u64 {
    32
}

impl Default for K8sInventoryHistoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_k8s_inv_hist_enabled(),
            max_file_mb: default_k8s_inv_hist_max_mb(),
        }
    }
}

/// Append-only hardware inventory history on disk (vCenter-style audit trail of platform identity / topology).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryHistoryConfig {
    /// When true, the daemon appends a full JSON snapshot on each interval.
    #[serde(default = "default_inventory_history_enabled")]
    pub enabled: bool,
    /// Wall time between snapshots. Minimum 60 seconds when `enabled` is true.
    #[serde(default = "default_inventory_history_interval_secs")]
    pub interval_secs: u64,
    /// When the JSONL file exceeds this size, the oldest lines are dropped (roughly 85% of this budget is kept).
    #[serde(default = "default_inventory_history_max_file_mb")]
    pub max_file_mb: u64,
}

fn default_inventory_history_enabled() -> bool {
    true
}

fn default_inventory_history_interval_secs() -> u64 {
    3600
}

fn default_inventory_history_max_file_mb() -> u64 {
    64
}

impl Default for InventoryHistoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_inventory_history_enabled(),
            interval_secs: default_inventory_history_interval_secs(),
            max_file_mb: default_inventory_history_max_file_mb(),
        }
    }
}

/// Tuning for generated KubeVirt + CDI YAML ([`crate::kubevirt`]) and optional `kubectl` / `virtctl` execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KubeVirtConfig {
    /// Namespace in generated manifests when the client does not override `?namespace=`.
    #[serde(default = "default_kubevirt_namespace")]
    pub default_namespace: String,
    /// Optional `storageClassName` on the upload DataVolume PVC (empty = cluster default).
    #[serde(default)]
    pub default_storage_class: String,
    /// Extra gibibytes added on top of the source image size (or memory-based fallback).
    #[serde(default = "default_kubevirt_datavolume_padding_gi")]
    pub datavolume_padding_gi: u32,
    /// `containerDisk` image for virtio-win CDROM in the guest (KubeVirt pulls this; analogous to hyper2kvm/libvirt `virtio-win.iso` on disk).
    #[serde(default = "default_kubevirt_virtio_container_disk_image")]
    pub virtio_container_disk_image: String,
    /// `spec.template.spec.domain.machine.type` (e.g. q35).
    #[serde(default = "default_kubevirt_machine_type")]
    pub machine_type: String,
    /// When true, `POST /api/v1/vms/{name}/kubevirt/apply|upload|start` may run `kubectl` / `virtctl` on the daemon host.
    #[serde(default)]
    pub exec_enabled: bool,
    #[serde(default = "default_kubevirt_kubectl")]
    pub kubectl_binary: String,
    #[serde(default = "default_kubevirt_virtctl")]
    pub virtctl_binary: String,
    /// If set, exported as `KUBECONFIG` for cluster commands.
    #[serde(default)]
    pub kubeconfig_path: String,
    /// Passed to `virtctl image-upload --upload-image-timeout=…m`.
    #[serde(default = "default_kubevirt_upload_timeout_mins")]
    pub upload_timeout_minutes: u64,
}

fn default_kubevirt_namespace() -> String {
    "default".to_string()
}

fn default_kubevirt_datavolume_padding_gi() -> u32 {
    5
}

fn default_kubevirt_virtio_container_disk_image() -> String {
    "quay.io/kubevirt/virtio-container-disk:latest".to_string()
}

fn default_kubevirt_machine_type() -> String {
    "q35".to_string()
}

fn default_kubevirt_kubectl() -> String {
    "kubectl".to_string()
}

fn default_kubevirt_virtctl() -> String {
    "virtctl".to_string()
}

fn default_kubevirt_upload_timeout_mins() -> u64 {
    120
}

impl Default for KubeVirtConfig {
    fn default() -> Self {
        Self {
            default_namespace: default_kubevirt_namespace(),
            default_storage_class: String::new(),
            datavolume_padding_gi: default_kubevirt_datavolume_padding_gi(),
            virtio_container_disk_image: default_kubevirt_virtio_container_disk_image(),
            machine_type: default_kubevirt_machine_type(),
            exec_enabled: false,
            kubectl_binary: default_kubevirt_kubectl(),
            virtctl_binary: default_kubevirt_virtctl(),
            kubeconfig_path: String::new(),
            upload_timeout_minutes: default_kubevirt_upload_timeout_mins(),
        }
    }
}

fn default_fluxvm_base_url() -> String {
    "http://127.0.0.1:7788".to_string()
}

fn default_fluxvm_backend() -> String {
    "auto".to_string()
}

/// FluxVM (`fluxvm-api`) as a second VM backend next to libvirt. VMs are
/// chosen per VM (`backend = "fluxvm"` on create, `?backend=fluxvm` on actions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FluxvmConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_fluxvm_base_url")]
    pub base_url: String,
    /// Bearer token for `[[auth.tokens]]` on the FluxVM side. Empty works only
    /// against a loopback `fluxvm-api` with auth off.
    #[serde(default)]
    pub token: String,
    /// Read the token from this file instead (takes precedence over `token`).
    #[serde(default)]
    pub token_file: String,
    /// Skip TLS certificate verification (lab / self-signed). Secure by default.
    #[serde(default)]
    pub insecure_tls: bool,
    /// FluxVM hypervisor used when a create request doesn't pick one:
    /// `auto`, `qemu`, `cloud-hypervisor`, `firecracker`, `flux-vm`.
    #[serde(default = "default_fluxvm_backend")]
    pub default_backend: String,
}

impl Default for FluxvmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: default_fluxvm_base_url(),
            token: String::new(),
            token_file: String::new(),
            insecure_tls: false,
            default_backend: default_fluxvm_backend(),
        }
    }
}

/// Proxy settings for GuestKit worker (distributed offline disk jobs on :8080 by default).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestkitConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_guestkit_base_url")]
    pub base_url: String,
    #[serde(default = "default_guestkit_insecure_tls")]
    pub insecure_tls: bool,
}

fn default_guestkit_base_url() -> String {
    "http://127.0.0.1:8080".into()
}

fn default_guestkit_insecure_tls() -> bool {
    // Secure by default (matches LdapConfig). A deployment terminating GuestKit
    // over HTTPS with a self-signed cert must opt in explicitly. The default
    // base_url is HTTP, where this flag is a no-op.
    false
}

impl Default for GuestkitConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: default_guestkit_base_url(),
            insecure_tls: default_guestkit_insecure_tls(),
        }
    }
}

/// Local container engine (Podman / Docker) via Vessel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VesselConfig {
    /// When true, daemon attempts to connect on start (fail-soft if socket missing).
    #[serde(default = "default_vessel_enabled")]
    pub enabled: bool,
    /// Optional unix socket override (`/run/user/…/podman/podman.sock` or `unix://…`).
    #[serde(default)]
    pub socket: String,
    /// Prefer auto-discovery when `socket` is empty.
    #[serde(default = "default_vessel_auto_discover")]
    pub auto_discover: bool,
}

fn default_vessel_enabled() -> bool {
    true
}

fn default_vessel_auto_discover() -> bool {
    true
}

impl Default for VesselConfig {
    fn default() -> Self {
        Self {
            enabled: default_vessel_enabled(),
            socket: String::new(),
            auto_discover: default_vessel_auto_discover(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TlsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub cert_path: String,
    #[serde(default)]
    pub key_path: String,
}

impl TlsConfig {
    /// Mirrors the `tls_enabled` gate in `daemon/src/main.rs` that decides between
    /// `axum_server::bind_rustls` and a plain TCP listener: `enabled` alone isn't enough,
    /// the cert/key paths must actually be set or the daemon falls back to plain HTTP.
    /// Session-cookie `Secure` attribution (see `AuthConfig::tls_enabled`) must agree with
    /// this or browsers will silently drop the cookie (Secure set, served over HTTP) or we
    /// leak it in the clear (Secure unset, actually served over HTTPS).
    pub fn is_effectively_enabled(&self) -> bool {
        self.enabled && !self.cert_path.is_empty() && !self.key_path.is_empty()
    }
}

/// Execute allow-listed host commands as the OIDC-mapped local user (`docs/oidc-run-as-user.md`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum RunAsUserMode {
    #[serde(alias = "disabled")]
    #[default]
    Disabled,
    /// Run allow-listed host commands as `effective_linux_user` via `sudo -n -u <user> -- …`.
    #[serde(alias = "sudo")]
    Sudo,
    #[serde(alias = "polkit")]
    Polkit,
    #[serde(alias = "setuid_helper")]
    SetuidHelper,
}

/// Per-session UNIX impersonation for allow-listed host commands (OS user lifecycle today).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunAsUserConfig {
    /// When true, supported routes run allow-listed host programs as `effective_linux_user`.
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub mode: RunAsUserMode,
    /// Setuid-root helper binary (`run-as-user-helper` crate). Used when `mode = "setuid_helper"`.
    #[serde(default = "default_setuid_helper_path")]
    pub setuid_helper_path: String,
    /// When dual libvirt is enabled, default VM create to `qemu:///session` for impersonated OIDC users.
    #[serde(default)]
    pub prefer_session_libvirt_on_impersonation: bool,
}

fn default_setuid_helper_path() -> String {
    "/usr/local/libexec/machina-run-as-user".to_string()
}

impl Default for RunAsUserConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: RunAsUserMode::Disabled,
            setuid_helper_path: default_setuid_helper_path(),
            prefer_session_libvirt_on_impersonation: false,
        }
    }
}

impl RunAsUserConfig {
    pub fn wants_impersonation(&self) -> bool {
        self.enabled && self.mode != RunAsUserMode::Disabled
    }

    pub fn sudo_impersonation_active(&self) -> bool {
        self.enabled && self.mode == RunAsUserMode::Sudo
    }

    pub fn polkit_impersonation_active(&self) -> bool {
        self.enabled && self.mode == RunAsUserMode::Polkit
    }

    pub fn setuid_helper_impersonation_active(&self) -> bool {
        self.enabled && self.mode == RunAsUserMode::SetuidHelper
    }

    pub fn impersonation_active(&self) -> bool {
        self.sudo_impersonation_active()
            || self.polkit_impersonation_active()
            || self.setuid_helper_impersonation_active()
    }
}

/// LDAP / Active Directory password authentication (simple bind + optional search).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LdapConfig {
    #[serde(default)]
    pub enabled: bool,
    /// e.g. `ldap://dc.example.com:389` or `ldaps://dc.example.com:636`
    #[serde(default)]
    pub url: String,
    /// Base DN for user search, e.g. `dc=example,dc=com`
    #[serde(default)]
    pub base_dn: String,
    /// Filter with `{username}` placeholder, e.g. `(sAMAccountName={username})`
    #[serde(default = "default_ldap_user_filter")]
    pub user_filter: String,
    /// Optional service bind DN (search before user bind). Empty = direct bind as `user_dn_template`.
    #[serde(default)]
    pub bind_dn: String,
    #[serde(default)]
    pub bind_password: String,
    /// When set, `user_dn` = template with `{username}`, e.g. `uid={username},ou=people,dc=example,dc=com`
    #[serde(default)]
    pub user_dn_template: String,
    /// Attribute used as Machina session username.
    #[serde(default = "default_ldap_username_attr")]
    pub username_attribute: String,
    #[serde(default)]
    pub use_tls: bool,
    #[serde(default)]
    pub insecure_tls: bool,
    /// LDAP attribute for group membership (e.g. `memberOf` on AD).
    #[serde(default = "default_ldap_member_attribute")]
    pub member_attribute: String,
    /// If any group DN/name contains one of these substrings → admin role.
    #[serde(default)]
    pub admin_group_substrings: Vec<String>,
    #[serde(default)]
    pub operator_group_substrings: Vec<String>,
    #[serde(default)]
    pub readonly_group_substrings: Vec<String>,
}

fn default_ldap_member_attribute() -> String {
    "memberOf".to_string()
}

fn default_ldap_user_filter() -> String {
    "(uid={username})".to_string()
}

fn default_ldap_username_attr() -> String {
    "uid".to_string()
}

impl Default for LdapConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            base_dn: String::new(),
            user_filter: default_ldap_user_filter(),
            bind_dn: String::new(),
            bind_password: String::new(),
            user_dn_template: String::new(),
            username_attribute: default_ldap_username_attr(),
            use_tls: false,
            insecure_tls: false,
            member_attribute: default_ldap_member_attribute(),
            admin_group_substrings: Vec::new(),
            operator_group_substrings: Vec::new(),
            readonly_group_substrings: Vec::new(),
        }
    }
}

impl LdapConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled && !self.url.trim().is_empty()
    }
}

/// Remote Machina daemon peers for fleet overview (this daemon remains the login/UI entry).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetPeer {
    pub name: String,
    /// Base URL, e.g. `https://hypervisor2:5092`
    pub url: String,
    /// Optional Bearer token for peer API (automation token on the remote host).
    #[serde(default)]
    pub api_token: String,
    #[serde(default)]
    pub insecure_tls: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FleetConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Name of peer treated as standby/secondary in UI (optional).
    #[serde(default)]
    pub standby_peer: String,
    /// Preferred peer for proxied lifecycle when set and reachable.
    #[serde(default)]
    pub primary_peer: String,
    #[serde(default)]
    pub peers: Vec<FleetPeer>,
}

impl FleetConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled && !self.peers.is_empty()
    }
}

/// PAM configuration for `machina-daemon` (web sign-in uses the same password as the selected stack).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// PAM service: which `/etc/pam.d/<name>` to use. `sshd` matches “remote” password rules; `login` is
    /// for local TTY and can block `root` or fail without a TTY.
    #[serde(default = "default_pam_service")]
    pub pam_service: String,
    /// Optional OpenID Connect login backend. When enabled, the web UI can redirect users to an external IdP.
    #[serde(default)]
    pub oidc: OidcConfig,
    /// Optional run-as-user impersonation (`docs/oidc-run-as-user.md`).
    #[serde(default)]
    pub run_as_user: RunAsUserConfig,
    /// Optional LDAP / Active Directory bind for password login (tried before PAM when enabled).
    #[serde(default)]
    pub ldap: LdapConfig,
    /// SAML SP metadata and IdP settings (config-only until SAML login is implemented).
    #[serde(default)]
    pub saml: SamlConfig,
    /// Max in-memory browser sessions cluster-wide (oldest evicted when exceeded).
    #[serde(default = "default_max_sessions_global")]
    pub max_sessions_global: usize,
    /// Max concurrent browser sessions per username. `0` = unlimited (default, no auto-logout).
    #[serde(default)]
    pub max_sessions_per_user: usize,
    /// Whether the daemon is actually serving over TLS (`TlsConfig::is_effectively_enabled`),
    /// not just `[tls] enabled = true` in config. Not read from the config file — set once
    /// at startup in `daemon/src/server.rs::create_app` from the live TLS decision, so the
    /// `machina_session` cookie's `Secure` attribute matches how the browser really reached us.
    #[serde(default)]
    pub tls_enabled: bool,
}

fn default_max_sessions_global() -> usize {
    1000
}

fn default_pam_service() -> String {
    "sshd".to_string()
}

fn default_oidc_scopes() -> Vec<String> {
    vec![
        "openid".to_string(),
        "profile".to_string(),
        "email".to_string(),
    ]
}

fn default_oidc_username_claim() -> String {
    "preferred_username".to_string()
}

fn default_oidc_groups_claim() -> String {
    "groups".to_string()
}

fn default_oidc_button_label() -> String {
    "Sign in with SSO".to_string()
}

fn default_oidc_linux_username_claim() -> String {
    "preferred_username".to_string()
}

/// Optional OpenID Connect / OAuth 2.0 Authorization Code flow for browser login.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcConfig {
    /// Enable OIDC login in addition to PAM.
    #[serde(default)]
    pub enabled: bool,
    /// Issuer URL, e.g. `https://sso.example.com/realms/machina`.
    #[serde(default)]
    pub issuer_url: String,
    /// OAuth client id registered with the IdP.
    #[serde(default)]
    pub client_id: String,
    /// OAuth client secret for confidential clients.
    #[serde(default)]
    pub client_secret: String,
    /// Absolute callback URL served by machina, e.g. `https://host:5092/api/v1/auth/oidc/callback`.
    #[serde(default)]
    pub redirect_url: String,
    /// Scopes requested during login. Must include `openid`.
    #[serde(default = "default_oidc_scopes")]
    pub scopes: Vec<String>,
    /// Claim used for the session username / RBAC lookup when present.
    #[serde(default = "default_oidc_username_claim")]
    pub username_claim: String,
    /// Claim containing group memberships for RBAC mapping.
    #[serde(default = "default_oidc_groups_claim")]
    pub groups_claim: String,
    /// Claim used to map an OIDC identity onto a local Linux user for sudo-gated host operations.
    #[serde(default = "default_oidc_linux_username_claim")]
    pub linux_username_claim: String,
    /// Groups that map to the admin role.
    #[serde(default)]
    pub admin_groups: Vec<String>,
    /// Groups that map to the operator role.
    #[serde(default)]
    pub operator_groups: Vec<String>,
    /// Role used when no configured group matches and no local role mapping exists.
    #[serde(default)]
    pub default_role: OidcDefaultRole,
    /// Login button text shown by the web UI.
    #[serde(default = "default_oidc_button_label")]
    pub button_label: String,
    /// When true, `qemu:///session` VM creation requires a mapped local Linux user to exist on the host.
    #[serde(default)]
    pub require_local_user_for_session_libvirt: bool,
}

impl OidcConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled
            && !self.issuer_url.trim().is_empty()
            && !self.client_id.trim().is_empty()
            && !self.redirect_url.trim().is_empty()
    }
}

impl Default for OidcConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            issuer_url: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
            redirect_url: String::new(),
            scopes: default_oidc_scopes(),
            username_claim: default_oidc_username_claim(),
            groups_claim: default_oidc_groups_claim(),
            linux_username_claim: default_oidc_linux_username_claim(),
            admin_groups: Vec::new(),
            operator_groups: Vec::new(),
            default_role: OidcDefaultRole::ReadOnly,
            button_label: default_oidc_button_label(),
            require_local_user_for_session_libvirt: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum OidcDefaultRole {
    Admin,
    Operator,
    #[default]
    ReadOnly,
}

fn default_saml_name_id_format() -> String {
    "urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress".to_string()
}

fn default_saml_button_label() -> String {
    "Sign in with SAML".to_string()
}

/// SAML 2.0 service-provider metadata (stored in config; login flow not yet wired).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamlConfig {
    #[serde(default)]
    pub enabled: bool,
    /// SP entity ID, e.g. `https://hypervisor.example.com/saml/metadata`.
    #[serde(default)]
    pub sp_entity_id: String,
    /// Assertion consumer URL, e.g. `https://host:5092/api/v1/auth/saml/acs`.
    #[serde(default)]
    pub sp_acs_url: String,
    /// IdP entity ID from federation metadata.
    #[serde(default)]
    pub idp_entity_id: String,
    /// Remote metadata URL (preferred over inline XML).
    #[serde(default)]
    pub idp_metadata_url: String,
    /// Inline IdP metadata XML when URL is unavailable.
    #[serde(default)]
    pub idp_metadata_xml: String,
    #[serde(default = "default_saml_name_id_format")]
    pub name_id_format: String,
    #[serde(default = "default_saml_button_label")]
    pub button_label: String,
    #[serde(default)]
    pub admin_groups: Vec<String>,
    #[serde(default)]
    pub operator_groups: Vec<String>,
    #[serde(default)]
    pub default_role: OidcDefaultRole,
    /// Operator notes (not used at runtime).
    #[serde(default)]
    pub notes: String,
}

impl SamlConfig {
    pub fn is_configured(&self) -> bool {
        self.enabled
            && !self.sp_entity_id.trim().is_empty()
            && (!self.idp_metadata_url.trim().is_empty()
                || !self.idp_metadata_xml.trim().is_empty())
    }
}

impl Default for SamlConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sp_entity_id: String::new(),
            sp_acs_url: String::new(),
            idp_entity_id: String::new(),
            idp_metadata_url: String::new(),
            idp_metadata_xml: String::new(),
            name_id_format: default_saml_name_id_format(),
            button_label: default_saml_button_label(),
            admin_groups: Vec::new(),
            operator_groups: Vec::new(),
            default_role: OidcDefaultRole::ReadOnly,
            notes: String::new(),
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            pam_service: default_pam_service(),
            oidc: OidcConfig::default(),
            run_as_user: RunAsUserConfig::default(),
            ldap: LdapConfig::default(),
            saml: SamlConfig::default(),
            max_sessions_global: default_max_sessions_global(),
            max_sessions_per_user: 0,
            tls_enabled: false,
        }
    }
}

/// Named SSH destinations for the browser terminal (`POST /api/v1/terminal/sessions` with `target_id`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshTerminalTarget {
    /// Opaque id (e.g. `lab-db`); never put raw IPs in the WebSocket URL — resolve via session API.
    pub id: String,
    /// Hostname or IP passed to `ssh user@host`.
    pub host: String,
    /// Default SSH login when the client omits `ssh_user` (empty = client must send `ssh_user`).
    #[serde(default)]
    pub ssh_user: String,
}

fn default_ssh_terminal_session_ttl() -> u64 {
    120
}

/// Controls `POST /api/v1/terminal/sessions` and `/ws/v1/terminal/{session_id}` (PTY + OpenSSH client).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshTerminalConfig {
    /// How long a created session id remains valid (seconds). Clamped to 30–3600 when used.
    #[serde(default = "default_ssh_terminal_session_ttl")]
    pub session_ttl_secs: u64,
    /// When no `target_id` is sent, allow `host` in the JSON body (still validated; browser never passes host in the WS path).
    #[serde(default = "default_true")]
    pub allow_adhoc_hosts: bool,
    /// Legacy `/ws/v1/ssh/{host}` WebSocket (raw keystrokes, no session). Prefer session flow; keep off in production.
    #[serde(default)]
    pub legacy_plain_host_websocket: bool,
    #[serde(default)]
    pub targets: Vec<SshTerminalTarget>,
}

impl Default for SshTerminalConfig {
    fn default() -> Self {
        Self {
            session_ttl_secs: default_ssh_terminal_session_ttl(),
            allow_adhoc_hosts: true,
            legacy_plain_host_websocket: false,
            targets: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval_secs: u64,
    /// Optional VM tag prefix for multi-project hosts (e.g. `project:` → tags like `project:team-a`).
    #[serde(default)]
    pub default_project_tag: String,
}

/// HTTP listen port when `[daemon]` has no `port = …` (matches install template & CLI overrides).
pub const DEFAULT_DAEMON_PORT: u16 = 5092;

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

/// How new VMs are created when the API client does not override `create_backend` on the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VmCreateBackend {
    /// Native Machina domain XML + `qemu-img`.
    LibvirtXml,
    /// Shell out to `virt-install`. Default when the client omits `create_backend`.
    #[default]
    VirtInstall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibvirtConfig {
    #[serde(default = "default_libvirt_uri")]
    pub uri: String,
    /// Default VM create engine when the client omits `create_backend` (normally [`VmCreateBackend::VirtInstall`]).
    #[serde(default)]
    pub create_backend: VmCreateBackend,
    /// Legacy libguestfs `virt-builder` integration (optional API fields). **Default: disabled** — prefer [`mkosi_allowed`](Self::mkosi_allowed) / `mkosi_workspace`. Set `virt_builder_allowed = true` only if you need virt-builder.
    #[serde(default = "default_false")]
    pub virt_builder_allowed: bool,
    /// If set and the client does not send `virt_builder_ssh_pubkey`, used for `--ssh-inject root:file:…`.
    #[serde(default)]
    pub virt_builder_default_ssh_pubkey_path: String,
    /// Pass `--update` to `virt-builder` (package updates inside the template).
    #[serde(default = "default_true")]
    pub virt_builder_update: bool,
    /// Packages always installed via `virt-builder --install` for every virt-builder VM.
    #[serde(default)]
    pub virt_builder_default_packages: Vec<String>,
    /// Install GuestKit agent (QGA protocol on `org.qemu.guest_agent.0`) on every new VM by default.
    #[serde(default = "default_true")]
    pub guest_agent_by_default: bool,
    /// Hypervisor path to `guestkit` binary used for cloud-init seed and offline inject.
    #[serde(default = "default_guestkit_agent_binary")]
    pub guestkit_agent_binary: String,
    /// Max concurrent async `virt-image-build` jobs (daemon). Default 2.
    #[serde(default = "default_virt_image_build_max_concurrent")]
    pub virt_image_build_max_concurrent: usize,
    /// Wall-clock limit for each `virt-image-build` / `virt-builder` child (seconds). `0` = unlimited.
    #[serde(default)]
    pub virt_image_build_timeout_secs: u64,
    /// Minimum free bytes on the filesystem that holds the output image directory (default 512 MiB).
    #[serde(default = "default_virt_image_build_min_free_parent_bytes")]
    pub virt_image_build_min_free_parent_bytes: u64,
    /// Minimum free bytes on `TMPDIR` (or `/tmp`) for libguestfs scratch (default 256 MiB).
    #[serde(default = "default_virt_image_build_min_free_tmp_bytes")]
    pub virt_image_build_min_free_tmp_bytes: u64,
    /// Allow `CreateVmRequest.mkosi_workspace` → `mkosi build` (optional image builds; requires mkosi on host; see install.sh).
    #[serde(default = "default_true")]
    pub mkosi_allowed: bool,
    /// Connect to **both** `qemu:///system` and `qemu:///session` (Cockpit-style); ignores `uri` when true.
    #[serde(default)]
    pub dual_connection: bool,
    /// Additional read-only libvirt URIs (e.g. `qemu+ssh://hypervisor2/system`) merged into VM lists.
    #[serde(default)]
    pub extra_uris: Vec<String>,
    /// Directory that browser ISO uploads are written to. Created on first upload if missing.
    /// Uploads are *only* ever written here — the client never chooses the directory.
    #[serde(default = "default_iso_upload_dir")]
    pub iso_upload_dir: String,
    /// Largest accepted ISO upload, in GiB. `0` disables uploads entirely.
    #[serde(default = "default_iso_upload_max_gib")]
    pub iso_upload_max_gib: u64,
    /// Allow Golden Forge Windows builds via dockur/windows (Podman + KVM). Default: disabled.
    #[serde(default = "default_false")]
    pub dockur_windows_allowed: bool,
    /// Override dockur `DISK_SIZE` (e.g. `64G`). Empty = script default.
    #[serde(default)]
    pub dockur_disk_size: String,
    /// Override dockur `RAM_SIZE` (e.g. `4G`). Empty = script default.
    #[serde(default)]
    pub dockur_ram_size: String,
    /// Override dockur `CPU_CORES`. Empty = script default.
    #[serde(default)]
    pub dockur_cpu_cores: String,
}

fn default_iso_upload_dir() -> String {
    "/var/lib/libvirt/images/isos".to_string()
}

fn default_iso_upload_max_gib() -> u64 {
    16
}

fn default_virt_image_build_max_concurrent() -> usize {
    2
}

fn default_virt_image_build_min_free_parent_bytes() -> u64 {
    512 * 1024 * 1024
}

fn default_virt_image_build_min_free_tmp_bytes() -> u64 {
    256 * 1024 * 1024
}

fn default_false() -> bool {
    false
}

fn default_true() -> bool {
    true
}

fn default_refresh_interval() -> u64 {
    5
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    DEFAULT_DAEMON_PORT
}

fn default_libvirt_uri() -> String {
    "qemu:///system".to_string()
}

fn default_guestkit_agent_binary() -> String {
    "/usr/local/bin/guestkit".to_string()
}

fn default_backup_dir() -> String {
    "/var/lib/machina/backups".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupConfig {
    #[serde(default = "default_backup_dir")]
    pub backup_dir: String,
    #[serde(default)]
    pub nfs_target: String,
    #[serde(default)]
    pub with_disks: bool,
    #[serde(default = "default_retain")]
    pub retain: u32,
}

fn default_retain() -> u32 {
    7
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            backup_dir: default_backup_dir(),
            nfs_target: String::new(),
            with_disks: false,
            retain: default_retain(),
        }
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            refresh_interval_secs: default_refresh_interval(),
            default_project_tag: String::new(),
        }
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

impl Default for LibvirtConfig {
    fn default() -> Self {
        Self {
            uri: default_libvirt_uri(),
            create_backend: VmCreateBackend::default(),
            virt_builder_allowed: false,
            virt_builder_default_ssh_pubkey_path: String::new(),
            virt_builder_update: true,
            virt_builder_default_packages: Vec::new(),
            guest_agent_by_default: true,
            guestkit_agent_binary: default_guestkit_agent_binary(),
            virt_image_build_max_concurrent: default_virt_image_build_max_concurrent(),
            virt_image_build_timeout_secs: 0,
            virt_image_build_min_free_parent_bytes: default_virt_image_build_min_free_parent_bytes(
            ),
            virt_image_build_min_free_tmp_bytes: default_virt_image_build_min_free_tmp_bytes(),
            mkosi_allowed: true,
            dual_connection: false,
            extra_uris: Vec::new(),
            iso_upload_dir: default_iso_upload_dir(),
            iso_upload_max_gib: default_iso_upload_max_gib(),
            dockur_windows_allowed: false,
            dockur_disk_size: String::new(),
            dockur_ram_size: String::new(),
            dockur_cpu_cores: String::new(),
        }
    }
}

impl MachinaConfig {
    /// Installed daemon config (`install.sh`, systemd unit).
    pub fn system_config_path() -> PathBuf {
        PathBuf::from("/etc/machina/config.toml")
    }

    /// Optional per-user overrides (development / non-root).
    pub fn user_config_dir() -> PathBuf {
        dirs_or_home().join(".machina")
    }

    pub fn user_config_path() -> PathBuf {
        Self::user_config_dir().join("config.toml")
    }

    /// Legacy alias for [`Self::user_config_dir`].
    pub fn config_dir() -> PathBuf {
        Self::user_config_dir()
    }

    /// Prefer [`Self::system_config_path`] as the canonical location.
    pub fn config_path() -> PathBuf {
        Self::system_config_path()
    }

    pub fn load() -> Self {
        let paths = [Self::system_config_path(), Self::user_config_path()];

        for config_path in &paths {
            if config_path.exists() {
                match fs::read_to_string(config_path) {
                    Ok(content) => match toml::from_str(&content) {
                        Ok(config) => {
                            tracing::info!("Loaded config from {}", config_path.display());
                            Self::warn_on_insecure_tls(&config);
                            return config;
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse {}: {e}", config_path.display());
                        }
                    },
                    Err(e) => {
                        tracing::warn!("Failed to read {}: {e}", config_path.display());
                    }
                }
            }
        }

        tracing::info!("No config file found, using defaults");
        Self::default()
    }

    /// Loud startup warning when the FluxVM link's TLS-verification bypass is
    /// enabled. It defaults to secure; an admin who explicitly flips it on
    /// should see it called out at boot.
    fn warn_on_insecure_tls(config: &Self) {
        if config.fluxvm.enabled && config.fluxvm.insecure_tls {
            tracing::warn!(
                "fluxvm.insecure_tls = true: TLS certificate verification is DISABLED for the \
                 fluxvm-api link ({}). Only use this for lab/self-signed setups — it \
                 accepts any certificate and is vulnerable to MITM.",
                config.fluxvm.base_url
            );
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        let sys = Self::system_config_path();
        if let Some(parent) = sys.parent() {
            let _ = fs::create_dir_all(parent);
            if fs::write(&sys, &content).is_ok() {
                return Ok(());
            }
        }
        let user = Self::user_config_path();
        if let Some(parent) = user.parent() {
            fs::create_dir_all(parent)?;
            fs::write(user, content)?;
            return Ok(());
        }
        anyhow::bail!("cannot save config (try sudo for /etc/machina)")
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.daemon.host, self.daemon.port)
    }

    /// Base URL for API clients (TUI, scripts). Uses `https` when TLS certs are configured.
    pub fn daemon_url(&self) -> String {
        let scheme = if self.tls.enabled
            && !self.tls.cert_path.is_empty()
            && !self.tls.key_path.is_empty()
        {
            "https"
        } else {
            "http"
        };
        format!("{}://{}:{}", scheme, self.daemon.host, self.daemon.port)
    }
}

fn dirs_or_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // Fallback: use /var/lib/machina instead of world-writable /tmp
            PathBuf::from("/var/lib/machina")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_url_uses_http_without_tls() {
        let c = MachinaConfig::default();
        assert!(c.daemon_url().starts_with("http://"), "{}", c.daemon_url());
    }

    #[test]
    fn daemon_url_uses_https_when_tls_configured() {
        let mut c = MachinaConfig::default();
        c.tls.enabled = true;
        c.tls.cert_path = "/etc/machina/ssl/cert.pem".into();
        c.tls.key_path = "/etc/machina/ssl/key.pem".into();
        assert!(c.daemon_url().starts_with("https://"));
    }

    #[test]
    fn oidc_default_claims_and_session_libvirt_gate() {
        let o = OidcConfig::default();
        assert_eq!(o.username_claim, "preferred_username");
        assert_eq!(o.linux_username_claim, "preferred_username");
        assert!(!o.require_local_user_for_session_libvirt);
    }

    #[test]
    fn inventory_history_defaults() {
        let i = InventoryHistoryConfig::default();
        assert!(i.enabled);
        assert_eq!(i.interval_secs, 3600);
        assert_eq!(i.max_file_mb, 64);
    }

    #[test]
    fn audit_log_defaults() {
        let a = AuditLogConfig::default();
        assert_eq!(a.max_file_mb, 64);
        assert_eq!(a.rotate_keep, 5);
    }

    #[test]
    fn observability_defaults() {
        let o = ObservabilityConfig::default();
        assert!(!o.otlp.enabled);
        assert!(o.linux_audit.enabled);
        assert_eq!(o.linux_audit.max_events, 200);
    }

    #[test]
    fn metrics_history_defaults() {
        let m = MetricsHistoryConfig::default();
        assert!(m.enabled);
        assert_eq!(m.interval_secs, 30);
        assert_eq!(m.max_points, 120);
        assert!(m.persist);
        assert_eq!(m.max_file_mb, 32);
    }

    #[test]
    fn k8s_inventory_history_defaults() {
        let k = K8sInventoryHistoryConfig::default();
        assert!(!k.enabled);
        assert_eq!(k.max_file_mb, 32);
    }

    #[test]
    fn fleet_and_ldap_defaults() {
        let c = MachinaConfig::default();
        assert!(!c.fleet.is_enabled());
        assert!(!c.auth.ldap.is_enabled());
    }

    #[test]
    fn run_as_user_polkit_active() {
        let r = RunAsUserConfig {
            enabled: true,
            mode: RunAsUserMode::Polkit,
            ..Default::default()
        };
        assert!(r.polkit_impersonation_active());
        assert!(r.impersonation_active());
    }

    #[test]
    fn run_as_user_setuid_active() {
        let r = RunAsUserConfig {
            enabled: true,
            mode: RunAsUserMode::SetuidHelper,
            ..Default::default()
        };
        assert!(r.setuid_helper_impersonation_active());
        assert!(r.impersonation_active());
        assert!(!r.setuid_helper_path.is_empty());
    }
}
