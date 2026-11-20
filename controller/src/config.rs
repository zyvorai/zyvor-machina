// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::path::PathBuf;
use std::sync::OnceLock;

/// Reads `MACHINA_JWT_SECRET`, or — when it is unset — generates a random secret
/// for this process's lifetime and logs a loud warning. This deliberately never
/// falls back to the historical `machina-dev-jwt-secret-change-me` literal: that
/// value is public (it ships in this repo), so a process actually signing tokens
/// with it would let anyone forge an admin JWT. Cached in a `OnceLock` so every
/// caller within this process sees the same secret (tokens signed early in
/// startup must still verify later).
///
/// If an operator explicitly sets `MACHINA_JWT_SECRET` to that literal dev
/// string, this function passes it through unchanged — `main.rs`'s startup
/// check still recognizes it as the known-bad default and refuses to boot
/// unless `MACHINA_ALLOW_DEV_SECRETS=1`. This function only covers the "the
/// operator never set the var at all" case: generate-random-and-warn instead
/// of silently using a public value, without hard-failing startup (a bigger,
/// separately-decided behavior change reserved for the explicit-bad-value case).
fn platform_jwt_secret() -> String {
    static SECRET: OnceLock<String> = OnceLock::new();
    SECRET
        .get_or_init(|| match std::env::var("MACHINA_JWT_SECRET") {
            Ok(s) if !s.is_empty() => s,
            _ => {
                tracing::warn!(
                    "MACHINA_JWT_SECRET is not set — generating a random controller JWT \
                     signing secret for this process only. Platform login sessions will \
                     NOT survive a controller restart until you set MACHINA_JWT_SECRET to \
                     a stable, private value (e.g. `openssl rand -hex 32`)."
                );
                use rand::Rng;
                let mut rng = rand::thread_rng();
                let bytes: [u8; 32] = rng.gen();
                hex::encode(bytes)
            }
        })
        .clone()
}

#[derive(Clone)]
pub struct ControllerConfig {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub nats_url: Option<String>,
    pub default_agent_addr: String,
    pub default_libvirt_uri: String,
    pub disk_image_dir: PathBuf,
    pub backup_dir: PathBuf,
    pub admin_user: String,
    pub admin_password: String,
    pub jwt_secret: String,
    pub controller_id: String,
    pub public_base_url: String,
    pub web_base_url: String,
    pub guestkit_enabled: bool,
    pub guestkit_worker_url: String,
    pub guestkit_insecure_tls: bool,
    /// Enforce lease (seconds) requested from each host's machina-bpfd when
    /// a runtime policy is applied or enforcement attached. bpfd clamps to
    /// [60, 86400] and fails open (observe) when the lease lapses.
    pub bpf_enforce_lease_secs: u64,
    /// Atlas — Zyvor storage control plane (Ceph/NFS/ZFS) used to provision
    /// VM disks as backend volumes and route snapshot/backup/restore.
    pub atlas_enabled: bool,
    pub atlas_base_url: String,
    /// Service-account JWT minted by Atlas (`POST /auth/tokens`); sent as a
    /// bearer token when Atlas runs with `ATLAS_AUTH_REQUIRED=1`.
    pub atlas_token: Option<String>,
    pub atlas_insecure_tls: bool,
    /// Default tenant recorded on Atlas volumes created for machina VMs.
    pub atlas_tenant_id: String,
    /// Default intent → placement policy for VM root disks (e.g. `database`,
    /// `general`); resolved to a StorageClass by atlas-policy.
    pub atlas_default_policy: String,
    /// Bound Atlas RGW bucket id used as the default target for VM backups.
    pub atlas_backup_bucket_id: Option<String>,
    /// Cluster-wide Ceph connection params used to attach Atlas RBD volumes as
    /// libvirt network disks. Atlas supplies the per-volume pool/image; these
    /// supply the monitor hosts and cephx credentials (a libvirt `ceph` secret).
    /// Comma-separated `host:port` list; empty = rely on the host's ceph.conf.
    pub atlas_rbd_mon_hosts: String,
    pub atlas_rbd_auth_user: Option<String>,
    pub atlas_rbd_secret_uuid: Option<String>,
    /// Co-located machina-daemon base URL for KubeVirt inventory sync.
    pub daemon_base_url: String,
    pub consolehub_session_ttl_secs: u64,
    /// Require approval workflow for production VM console (Phase 2/5).
    pub consolehub_require_approval: bool,
    /// Enable session recording metadata (Phase 2).
    pub consolehub_recording_enabled: bool,
    /// Directory for ConsoleHub session replay files (`.webm` per session id).
    pub consolehub_recording_dir: PathBuf,
    /// Require OIDC/SAML federation before opening production consoles (Phase 5).
    pub consolehub_require_oidc: bool,
    /// Zone name → CIDR map for firewall profile rule sources (e.g.
    /// `admin-network` → `10.0.0.0/8`), from `MACHINA_FIREWALL_ZONES`
    /// (`name=cidr,name=cidr`, malformed entries skipped). Injected into
    /// outgoing `FirewallPlanRequest`s so host/K8s firewall backends can
    /// resolve symbolic rule sources into real source restrictions instead of
    /// silently applying them unrestricted.
    pub firewall_zones: std::collections::HashMap<String, String>,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 5093,
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| crate::db::DEFAULT_URL.into()),
            nats_url: std::env::var("NATS_URL").ok(),
            default_agent_addr: std::env::var("MACHINA_AGENT_ADDR")
                .unwrap_or_else(|_| "http://127.0.0.1:50051".into()),
            default_libvirt_uri: std::env::var("MACHINA_LIBVIRT_URI")
                .unwrap_or_else(|_| "qemu:///system".into()),
            disk_image_dir: PathBuf::from("/var/lib/libvirt/images"),
            backup_dir: PathBuf::from("/var/lib/machina/backups"),
            // Bootstrap admin credentials — overridable so a deploy isn't stuck with
            // admin/admin. Only used to seed the first user when the table is empty.
            admin_user: std::env::var("MACHINA_ADMIN_USER").unwrap_or_else(|_| "admin".into()),
            admin_password: std::env::var("MACHINA_ADMIN_PASSWORD")
                .unwrap_or_else(|_| "admin".into()),
            jwt_secret: platform_jwt_secret(),
            controller_id: std::env::var("MACHINA_CONTROLLER_ID")
                .unwrap_or_else(|_| format!("ctrl-{}", &uuid::Uuid::new_v4().to_string()[..8])),
            public_base_url: std::env::var("MACHINA_PUBLIC_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:5093".into()),
            web_base_url: std::env::var("MACHINA_WEB_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:5173".into()),
            guestkit_enabled: std::env::var("GUESTKIT_ENABLED")
                .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(true),
            guestkit_worker_url: std::env::var("GUESTKIT_WORKER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080".into()),
            guestkit_insecure_tls: std::env::var("GUESTKIT_INSECURE_TLS")
                .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
                // Secure by default — set GUESTKIT_INSECURE_TLS=1 for a self-signed
                // HTTPS worker. Default worker URL is HTTP, where TLS doesn't apply.
                .unwrap_or(false),
            bpf_enforce_lease_secs: std::env::var("MACHINA_BPF_ENFORCE_LEASE_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(900),
            atlas_enabled: std::env::var("ATLAS_ENABLED")
                .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(false),
            atlas_base_url: std::env::var("ATLAS_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:5110".into()),
            atlas_token: std::env::var("ATLAS_TOKEN").ok().filter(|s| !s.is_empty()),
            atlas_insecure_tls: std::env::var("ATLAS_INSECURE_TLS")
                .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
                // Secure by default — set ATLAS_INSECURE_TLS=1 for a self-signed gateway.
                .unwrap_or(false),
            atlas_tenant_id: std::env::var("ATLAS_TENANT_ID").unwrap_or_else(|_| "machina".into()),
            atlas_default_policy: std::env::var("ATLAS_DEFAULT_POLICY")
                .unwrap_or_else(|_| "general".into()),
            atlas_backup_bucket_id: std::env::var("ATLAS_BACKUP_BUCKET_ID")
                .ok()
                .filter(|s| !s.is_empty()),
            atlas_rbd_mon_hosts: std::env::var("ATLAS_RBD_MON_HOSTS").unwrap_or_default(),
            atlas_rbd_auth_user: std::env::var("ATLAS_RBD_AUTH_USER")
                .ok()
                .filter(|s| !s.is_empty()),
            atlas_rbd_secret_uuid: std::env::var("ATLAS_RBD_SECRET_UUID")
                .ok()
                .filter(|s| !s.is_empty()),
            daemon_base_url: std::env::var("MACHINA_DAEMON_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:5092".into()),
            consolehub_session_ttl_secs: std::env::var("CONSOLEHUB_SESSION_TTL_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(600),
            consolehub_require_approval: std::env::var("CONSOLEHUB_REQUIRE_APPROVAL")
                .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(false),
            consolehub_recording_enabled: std::env::var("CONSOLEHUB_RECORDING_ENABLED")
                .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(false),
            consolehub_recording_dir: std::env::var("CONSOLEHUB_RECORDING_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/var/lib/machina/console-recordings")),
            consolehub_require_oidc: std::env::var("CONSOLEHUB_REQUIRE_OIDC")
                .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(false),
            firewall_zones: std::env::var("MACHINA_FIREWALL_ZONES")
                .map(|raw| machina_core::parse_zone_env(&raw))
                .unwrap_or_default(),
        }
    }
}

/// Hand-written so a stray `tracing::debug!("{config:?}")` (or similar) can
/// never leak the JWT signing secret, admin password, or other credentials
/// this struct carries — a derived `Debug` would print every field verbatim.
impl std::fmt::Debug for ControllerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const REDACTED: &str = "<redacted>";
        f.debug_struct("ControllerConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("database_url", &crate::db::redact_url(&self.database_url))
            .field("nats_url", &self.nats_url)
            .field("default_agent_addr", &self.default_agent_addr)
            .field("default_libvirt_uri", &self.default_libvirt_uri)
            .field("disk_image_dir", &self.disk_image_dir)
            .field("backup_dir", &self.backup_dir)
            .field("admin_user", &self.admin_user)
            .field("admin_password", &REDACTED)
            .field("jwt_secret", &REDACTED)
            .field("controller_id", &self.controller_id)
            .field("public_base_url", &self.public_base_url)
            .field("web_base_url", &self.web_base_url)
            .field("guestkit_enabled", &self.guestkit_enabled)
            .field("guestkit_worker_url", &self.guestkit_worker_url)
            .field("guestkit_insecure_tls", &self.guestkit_insecure_tls)
            .field("bpf_enforce_lease_secs", &self.bpf_enforce_lease_secs)
            .field("atlas_enabled", &self.atlas_enabled)
            .field("atlas_base_url", &self.atlas_base_url)
            .field("atlas_token", &self.atlas_token.as_ref().map(|_| REDACTED))
            .field("atlas_insecure_tls", &self.atlas_insecure_tls)
            .field("atlas_tenant_id", &self.atlas_tenant_id)
            .field("atlas_default_policy", &self.atlas_default_policy)
            .field("atlas_backup_bucket_id", &self.atlas_backup_bucket_id)
            .field("atlas_rbd_mon_hosts", &self.atlas_rbd_mon_hosts)
            .field("atlas_rbd_auth_user", &self.atlas_rbd_auth_user)
            .field(
                "atlas_rbd_secret_uuid",
                &self.atlas_rbd_secret_uuid.as_ref().map(|_| REDACTED),
            )
            .field("daemon_base_url", &self.daemon_base_url)
            .field(
                "consolehub_session_ttl_secs",
                &self.consolehub_session_ttl_secs,
            )
            .field(
                "consolehub_require_approval",
                &self.consolehub_require_approval,
            )
            .field(
                "consolehub_recording_enabled",
                &self.consolehub_recording_enabled,
            )
            .field("consolehub_recording_dir", &self.consolehub_recording_dir)
            .field("consolehub_require_oidc", &self.consolehub_require_oidc)
            .field(
                "firewall_zones",
                &self.firewall_zones.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}
