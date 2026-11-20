// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Extension, Path, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use machina_core::system_accounts;
use machina_core::{
    apply_ldap_patch, apply_observability_patch, apply_oidc_patch, apply_saml_patch, audit,
    audit_ship, ldap_settings_view_from_config, oidc_settings_view_from_config,
    saml_settings_view_from_config, settings_view_from_config, LdapSettingsPatch, LdapTestRequest,
    LdapTestResponse, LibvirtError, LibvirtManager, MachinaConfig, ObservabilitySettingsPatch,
    OidcSettingsPatch, SamlSettingsPatch,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tracing::info;

use crate::auth::{effective_linux_user, RequestActor};
use crate::daemon_stats::DaemonStats;
use crate::error::AppError;
use crate::http_metrics::HttpMetrics;
use crate::metrics_history::MetricsHistoryStore;
use crate::obs_workers::ObservabilityWorkers;

fn os_user_capability_json(actor: &RequestActor) -> serde_json::Value {
    let libvirt_ok = system_accounts::libvirt_unix_group_exists();
    let backend = system_accounts::os_user_account_backend();
    let homed_active = system_accounts::systemd_homed_is_active();
    let homectl_ok = system_accounts::homectl_available();
    let sudo_g = system_accounts::sudo_supplementary_group();

    if actor.from_api_token {
        return serde_json::json!({
            "canCreateOsUsers": false,
            "canDeleteOsUsers": false,
            "reason": "API tokens cannot create or delete system users",
            "libvirtGroupAvailable": libvirt_ok,
            "libvirtGroupName": system_accounts::LIBVIRT_UNIX_GROUP,
            "userAccountBackend": backend,
            "systemdHomedActive": homed_active,
            "homectlAvailable": homectl_ok,
            "sudoSupplementaryGroup": sudo_g,
        });
    }
    let effective = effective_linux_user(actor);
    let ok = effective
        .map(system_accounts::unix_user_may_use_sudo)
        .unwrap_or(false);
    serde_json::json!({
        "canCreateOsUsers": ok,
        "canDeleteOsUsers": ok,
        "effectiveLinuxUser": effective,
        "reason": if ok {
            serde_json::Value::Null
        } else if effective.is_none() {
            serde_json::json!("Signed-in identity is not mapped to a local Linux user on this host")
        } else {
            serde_json::json!("Signed-in user is not in wheel, sudo, or admin (required to create or delete accounts)")
        },
        "libvirtGroupAvailable": libvirt_ok,
        "libvirtGroupName": system_accounts::LIBVIRT_UNIX_GROUP,
        "userAccountBackend": backend,
        "systemdHomedActive": homed_active,
        "homectlAvailable": homectl_ok,
        "sudoSupplementaryGroup": sudo_g,
    })
}

async fn os_users_capability(Extension(actor): Extension<RequestActor>) -> Json<serde_json::Value> {
    Json(os_user_capability_json(&actor))
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
struct CreateOsUserRequest {
    username: String,
    password: String,
    /// Append new user to host `libvirt` group (`qemu:///system`). Default: true.
    #[serde(default = "default_true")]
    add_to_libvirt_group: bool,
}

async fn create_os_user(
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<CreateOsUserRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if actor.from_api_token {
        return Err(LibvirtError::Forbidden("API tokens cannot create system users".into()).into());
    }
    let Some(effective_user) = effective_linux_user(&actor) else {
        return Err(LibvirtError::Forbidden(
            "This identity is not mapped to a local Linux user on this host".into(),
        )
        .into());
    };
    if !system_accounts::unix_user_may_use_sudo(effective_user) {
        return Err(LibvirtError::Forbidden(
            "Only users in wheel, sudo, or admin may create system accounts".into(),
        )
        .into());
    }
    let cfg = MachinaConfig::load();
    let exec_as = cfg
        .auth
        .run_as_user
        .impersonation_active()
        .then_some((&cfg.auth.run_as_user, effective_user));
    let outcome = system_accounts::create_local_user(
        &req.username,
        &req.password,
        req.add_to_libvirt_group,
        exec_as,
    )?;
    info!(
        "OS user '{}' created via machina by session user '{}' (effective linux user '{}', libvirt group: {})",
        req.username, actor.username, effective_user, outcome.libvirt_group_attached
    );
    Ok(Json(serde_json::json!({
        "status": "ok",
        "username": req.username,
        "libvirt_group_attached": outcome.libvirt_group_attached,
        "account_backend": system_accounts::os_user_account_backend(),
    })))
}

async fn delete_os_user(
    Extension(actor): Extension<RequestActor>,
    Path(username): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if actor.from_api_token {
        return Err(LibvirtError::Forbidden("API tokens cannot delete system users".into()).into());
    }
    let Some(effective_user) = effective_linux_user(&actor) else {
        return Err(LibvirtError::Forbidden(
            "This identity is not mapped to a local Linux user on this host".into(),
        )
        .into());
    };
    if !system_accounts::unix_user_may_use_sudo(effective_user) {
        return Err(LibvirtError::Forbidden(
            "Only users in wheel, sudo, or admin may delete system accounts".into(),
        )
        .into());
    }
    if username == effective_user {
        return Err(
            LibvirtError::Forbidden("Cannot delete the signed-in UNIX account".into()).into(),
        );
    }
    let cfg = MachinaConfig::load();
    let exec_as = cfg
        .auth
        .run_as_user
        .impersonation_active()
        .then_some((&cfg.auth.run_as_user, effective_user));
    system_accounts::delete_local_user(&username, exec_as)?;
    info!(
        "OS user '{}' removed via machina by session user '{}' (effective linux user '{}')",
        username, actor.username, effective_user,
    );
    Ok(Json(serde_json::json!({
        "status": "ok",
        "username": username,
    })))
}

const CREATE_VM_DEFAULTS_PATH: &str = "/var/lib/machina/create-vm-defaults.json";

fn load_create_vm_defaults_json() -> serde_json::Value {
    std::fs::read_to_string(CREATE_VM_DEFAULTS_PATH)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(json!({}))
}

async fn get_create_vm_defaults(
    Extension(_actor): Extension<RequestActor>,
) -> Json<serde_json::Value> {
    Json(load_create_vm_defaults_json())
}

async fn put_create_vm_defaults(
    Extension(actor): Extension<RequestActor>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_write() {
        return Err(LibvirtError::Forbidden(
            "Saving hypervisor defaults requires operator or admin.".into(),
        )
        .into());
    }
    std::fs::create_dir_all("/var/lib/machina").map_err(|e| {
        AppError::from(LibvirtError::Operation(format!(
            "create /var/lib/machina: {e}"
        )))
    })?;
    let data = serde_json::to_string_pretty(&body)
        .map_err(|e| AppError::from(LibvirtError::Operation(format!("serialize defaults: {e}"))))?;
    std::fs::write(CREATE_VM_DEFAULTS_PATH, data)
        .map_err(|e| AppError::from(LibvirtError::Operation(format!("write defaults: {e}"))))?;
    Ok(Json(json!({ "status": "saved" })))
}

fn read_host_os_pretty() -> String {
    let Ok(content) = std::fs::read_to_string("/etc/os-release") else {
        return String::new();
    };
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("PRETTY_NAME=") {
            return rest.trim_matches('"').to_string();
        }
    }
    String::new()
}

/// Surfaces the runtime "what is enabled?" view used by the web shell to render
/// capability badges (TLS, OIDC, KubeVirt cluster exec, virtio-win image, etc.).
async fn platform_info() -> Json<serde_json::Value> {
    let cfg = MachinaConfig::load();
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "host": {
            "os_pretty_name": read_host_os_pretty(),
        },
        "tls": {
            "enabled": cfg.tls.enabled
                && !cfg.tls.cert_path.is_empty()
                && !cfg.tls.key_path.is_empty(),
        },
        "auth": {
            "pam_service": cfg.auth.pam_service,
            "ldap_enabled": cfg.auth.ldap.is_enabled(),
            "oidc_enabled": cfg.auth.oidc.is_enabled(),
            "run_as_user_enabled": cfg.auth.run_as_user.enabled,
            "run_as_user_mode": format!("{:?}", cfg.auth.run_as_user.mode).to_lowercase(),
        },
        "fleet": {
            "enabled": cfg.fleet.is_enabled(),
            "peer_count": cfg.fleet.peers.len(),
        },
        "rdp": {
            "builtin_ws_proxy": true,
        },
        "libvirt": {
            "dual_connection": cfg.libvirt.dual_connection,
            "extra_uris": cfg.libvirt.extra_uris,
        },
        "kubevirt": {
            "exec_enabled": cfg.kubevirt.exec_enabled,
            "default_namespace": cfg.kubevirt.default_namespace,
            "default_storage_class": cfg.kubevirt.default_storage_class,
            "virtio_container_disk_image": cfg.kubevirt.virtio_container_disk_image,
            "machine_type": cfg.kubevirt.machine_type,
        },
        "fluxvm": {
            "enabled": cfg.fluxvm.enabled,
            "base_url": cfg.fluxvm.base_url,
            "default_backend": cfg.fluxvm.default_backend,
        },
        "guestkit": {
            "enabled": cfg.guestkit.enabled,
            "base_url": cfg.guestkit.base_url,
            "insecure_tls": cfg.guestkit.insecure_tls,
        },
        "native_bpf": {
            "socket": std::env::var("MACHINA_BPFD_SOCK")
                .unwrap_or_else(|_| machina_core::bpf_probe::BPFD_SOCKET.to_string()),
        },
        "zeus_firewall": {
            "enabled": true,
            "phase": 1,
        },
        "control_plane": {
            "proxy_url": "/api/v1/platform/controller",
            "direct_url": std::env::var("MACHINA_PLATFORM_CONTROLLER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:5093".into()),
        },
    }))
}

async fn get_observability_settings(
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_manage_users() {
        return Err(LibvirtError::Forbidden(
            "Observability settings require the admin role.".into(),
        )
        .into());
    }
    let cfg = MachinaConfig::load();
    Ok(Json(serde_json::json!(settings_view_from_config(&cfg))))
}

async fn put_observability_settings(
    Extension(actor): Extension<RequestActor>,
    Extension(workers): Extension<Arc<ObservabilityWorkers>>,
    State(manager): State<LibvirtManager>,
    Extension(store): Extension<MetricsHistoryStore>,
    Extension(stats): Extension<Arc<DaemonStats>>,
    Extension(http_metrics): Extension<Arc<HttpMetrics>>,
    Json(patch): Json<ObservabilitySettingsPatch>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_manage_users() {
        return Err(LibvirtError::Forbidden(
            "Observability settings require the admin role.".into(),
        )
        .into());
    }
    let mut cfg = MachinaConfig::load();
    apply_observability_patch(&mut cfg, &patch);
    cfg.save()
        .map_err(|e| AppError::from(LibvirtError::Operation(format!("save config: {e}"))))?;
    audit::configure_rotation(cfg.audit.clone());
    audit_ship::configure_ship(cfg.audit.clone());
    machina_core::linux_audit::configure_linux_audit(cfg.observability.linux_audit.clone());
    let view = settings_view_from_config(&cfg);
    info!("observability settings saved to {}", view.config_path);
    workers.reload(&manager, &cfg, &store, &stats, &http_metrics);
    Ok(Json(serde_json::json!({
        "status": "saved",
        "restart_recommended": false,
        "workers_reloaded": true,
        "note": "OTLP and metrics-history workers reloaded from config.",
        "settings": view,
    })))
}

async fn get_ldap_settings(
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_manage_users() {
        return Err(LibvirtError::Forbidden("LDAP settings require the admin role.".into()).into());
    }
    let cfg = MachinaConfig::load();
    Ok(Json(serde_json::json!(ldap_settings_view_from_config(
        &cfg
    ))))
}

async fn put_ldap_settings(
    Extension(actor): Extension<RequestActor>,
    Json(patch): Json<LdapSettingsPatch>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_manage_users() {
        return Err(LibvirtError::Forbidden("LDAP settings require the admin role.".into()).into());
    }
    let mut cfg = MachinaConfig::load();
    apply_ldap_patch(&mut cfg, &patch);
    cfg.save()
        .map_err(|e| AppError::from(LibvirtError::Operation(format!("save config: {e}"))))?;
    let view = ldap_settings_view_from_config(&cfg);
    info!("LDAP settings saved to {}", view.config_path);
    Ok(Json(serde_json::json!({
        "status": "saved",
        "restart_recommended": true,
        "restart_message": "Saved to the config file, but the running daemon keeps its settings from startup. Restart machina-daemon to apply: sudo systemctl restart machina-daemon",
        "settings": view,
    })))
}

async fn post_ldap_test(
    Extension(actor): Extension<RequestActor>,
    Json(body): Json<LdapTestRequest>,
) -> Result<Json<LdapTestResponse>, AppError> {
    if !actor.role.can_manage_users() {
        return Err(LibvirtError::Forbidden("LDAP test requires the admin role.".into()).into());
    }
    let cfg = MachinaConfig::load();
    if !cfg.auth.ldap.is_enabled() {
        return Ok(Json(LdapTestResponse {
            ok: false,
            message: "LDAP is not enabled in config".into(),
            username: None,
            role: None,
        }));
    }
    let username = body.username.unwrap_or_default();
    let password = body.password.unwrap_or_default();
    if username.is_empty() || password.is_empty() {
        return Ok(Json(LdapTestResponse {
            ok: false,
            message: "username and password required for bind test".into(),
            username: None,
            role: None,
        }));
    }
    match crate::ldap_auth::ldap_authenticate_async(&cfg.auth.ldap, &username, &password).await {
        Ok(r) => Ok(Json(LdapTestResponse {
            ok: true,
            message: "LDAP bind succeeded".into(),
            username: Some(r.username),
            role: Some(format!("{:?}", r.role).to_lowercase()),
        })),
        Err(e) => Ok(Json(LdapTestResponse {
            ok: false,
            message: e,
            username: None,
            role: None,
        })),
    }
}

async fn get_oidc_settings(
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_manage_users() {
        return Err(LibvirtError::Forbidden("OIDC settings require the admin role.".into()).into());
    }
    let cfg = MachinaConfig::load();
    Ok(Json(serde_json::json!(oidc_settings_view_from_config(
        &cfg
    ))))
}

async fn put_oidc_settings(
    Extension(actor): Extension<RequestActor>,
    Json(patch): Json<OidcSettingsPatch>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_manage_users() {
        return Err(LibvirtError::Forbidden("OIDC settings require the admin role.".into()).into());
    }
    let mut cfg = MachinaConfig::load();
    apply_oidc_patch(&mut cfg, &patch);
    cfg.save()
        .map_err(|e| AppError::from(LibvirtError::Operation(format!("save config: {e}"))))?;
    let view = oidc_settings_view_from_config(&cfg);
    info!("OIDC settings saved to {}", view.config_path);
    Ok(Json(serde_json::json!({
        "status": "saved",
        "restart_recommended": true,
        "restart_message": "Saved to the config file, but the running daemon keeps its settings from startup — the SSO button won't appear on the login page until you restart it: sudo systemctl restart machina-daemon",
        "settings": view,
    })))
}

async fn get_saml_settings(
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_manage_users() {
        return Err(LibvirtError::Forbidden("SAML settings require the admin role.".into()).into());
    }
    let cfg = MachinaConfig::load();
    Ok(Json(serde_json::json!(saml_settings_view_from_config(
        &cfg
    ))))
}

async fn put_saml_settings(
    Extension(actor): Extension<RequestActor>,
    Json(patch): Json<SamlSettingsPatch>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_manage_users() {
        return Err(LibvirtError::Forbidden("SAML settings require the admin role.".into()).into());
    }
    let mut cfg = MachinaConfig::load();
    apply_saml_patch(&mut cfg, &patch);
    cfg.save()
        .map_err(|e| AppError::from(LibvirtError::Operation(format!("save config: {e}"))))?;
    let view = saml_settings_view_from_config(&cfg);
    info!("SAML settings saved to {}", view.config_path);
    Ok(Json(serde_json::json!({
        "status": "saved",
        "restart_recommended": true,
        "restart_message": "Saved to the config file, but the running daemon keeps its settings from startup. Restart machina-daemon to apply: sudo systemctl restart machina-daemon",
        "note": "SAML login flow is config-only — metadata is stored for IdP federation setup.",
        "settings": view,
    })))
}

pub fn system_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/system/platform-info", get(platform_info))
        .route("/system/os-users/capability", get(os_users_capability))
        .route("/system/os-users", post(create_os_user))
        .route("/system/os-users/{username}", delete(delete_os_user))
        .route(
            "/system/create-vm-defaults",
            get(get_create_vm_defaults).put(put_create_vm_defaults),
        )
        .route(
            "/system/observability-settings",
            get(get_observability_settings).put(put_observability_settings),
        )
        .route(
            "/system/auth/ldap-settings",
            get(get_ldap_settings).put(put_ldap_settings),
        )
        .route("/system/auth/ldap-test", post(post_ldap_test))
        .route(
            "/system/auth/oidc-settings",
            get(get_oidc_settings).put(put_oidc_settings),
        )
        .route(
            "/system/auth/saml-settings",
            get(get_saml_settings).put(put_saml_settings),
        )
}
