// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Automation: RBAC, API tokens, alerts, webhooks, scheduled actions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::LibvirtError;

const DATA_DIR: &str = "/var/lib/machina";

/// Global mutex for JSON file read-modify-write operations.
/// Since all operations go through the same daemon process, a process-level
/// mutex is sufficient to prevent race conditions on concurrent requests.
static JSON_LOCK: Mutex<()> = Mutex::new(());

/// Execute a closure while holding the JSON file lock.
/// Prevents concurrent read-modify-write races on JSON state files.
pub fn with_json_lock<T, F: FnOnce() -> T>(f: F) -> T {
    let _guard = JSON_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    f()
}

// ── RBAC ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Role {
    #[serde(rename = "admin")]
    Admin,
    #[serde(rename = "operator")]
    Operator,
    #[serde(rename = "readonly")]
    ReadOnly,
}

impl Role {
    pub fn is_admin(&self) -> bool {
        matches!(self, Role::Admin)
    }
    pub fn can_write(&self) -> bool {
        matches!(self, Role::Admin | Role::Operator)
    }
    pub fn can_delete(&self) -> bool {
        matches!(self, Role::Admin)
    }
    pub fn can_manage_users(&self) -> bool {
        matches!(self, Role::Admin)
    }
    /// Undefine/destroy guests and remove host disk images from browse API.
    pub fn can_destroy_vm(&self) -> bool {
        self.can_delete()
    }
    /// USB and PCI passthrough onto a guest.
    pub fn can_usb_pci(&self) -> bool {
        matches!(self, Role::Admin | Role::Operator)
    }
    /// Browse arbitrary host directories from the UI/API (hypervisor filesystem).
    pub fn can_browse_host_paths(&self) -> bool {
        matches!(self, Role::Admin)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRole {
    pub username: String,
    pub role: Role,
}

type RoleMap = HashMap<String, Role>;

fn roles_path() -> String {
    format!("{DATA_DIR}/roles.json")
}

pub fn load_roles() -> RoleMap {
    match std::fs::read_to_string(roles_path()) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

pub fn save_roles(roles: &RoleMap) -> Result<(), LibvirtError> {
    let _ = std::fs::create_dir_all(DATA_DIR);
    let data = serde_json::to_string_pretty(roles)
        .map_err(|e| LibvirtError::Operation(format!("Serialize roles: {e}")))?;
    std::fs::write(roles_path(), data)
        .map_err(|e| LibvirtError::Operation(format!("Write roles: {e}")))?;
    Ok(())
}

pub fn get_user_role(username: &str) -> Role {
    let roles = load_roles();
    if roles.is_empty() {
        // No roles.json yet — treat browser/PAM users as admin until an admin creates the map.
        return Role::Admin;
    }
    roles.get(username).cloned().unwrap_or(Role::ReadOnly)
}

pub fn set_user_role(username: &str, role: Role) -> Result<(), LibvirtError> {
    with_json_lock(|| {
        let mut roles = load_roles();
        roles.insert(username.to_string(), role);
        save_roles(&roles)
    })
}

// ── API Tokens ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToken {
    pub name: String,
    /// Masked preview (`head...tail`) once persisted or listed — never the live
    /// secret. The raw value only ever exists in the `ApiToken` returned directly
    /// to the immediate caller at creation time (e.g. the `POST /tokens` HTTP
    /// response), so an admin can copy it once; it is never written to disk here.
    pub token: String,
    /// SHA-256 hex digest of the raw token, used to authenticate bearer tokens
    /// without ever persisting the secret itself. `#[serde(default)]` lets a
    /// pre-hashing `api-tokens.json` (raw token stored in `token`) deserialize;
    /// `load_tokens` migrates any such legacy entry to hashed form on first read.
    #[serde(default)]
    pub token_hash: String,
    pub username: String,
    pub role: Role,
    pub created: String,
    /// Fine-grained scopes; empty = defaults from role (see `default_scopes_for_role`).
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// Default API token scopes when `scopes` is empty on the token.
pub fn default_scopes_for_role(role: &Role) -> Vec<String> {
    match role {
        Role::Admin => vec!["*".to_string()],
        Role::Operator => vec![
            "vms:read".into(),
            "vms:write".into(),
            "networks:read".into(),
            "networks:write".into(),
            "storage:read".into(),
            "storage:write".into(),
            "fleet:proxy".into(),
        ],
        Role::ReadOnly => vec![
            "vms:read".into(),
            "networks:read".into(),
            "storage:read".into(),
            "audit:read".into(),
        ],
    }
}

pub fn effective_token_scopes(token: &ApiToken) -> Vec<String> {
    if token.scopes.is_empty() {
        default_scopes_for_role(&token.role)
    } else {
        token.scopes.clone()
    }
}

/// `required` may be `vms:write` or `*` (admin).
pub fn token_allows(scopes: &[String], required: &str) -> bool {
    if scopes.iter().any(|s| s == "*") {
        return true;
    }
    if scopes.iter().any(|s| s == required) {
        return true;
    }
    if let Some((prefix, _)) = required.split_once(':') {
        let wild = format!("{prefix}:*");
        if scopes.iter().any(|s| s == &wild) {
            return true;
        }
    }
    false
}

type TokenMap = HashMap<String, ApiToken>; // sha256(token) hex -> ApiToken

fn tokens_path() -> String {
    format!("{DATA_DIR}/api-tokens.json")
}

/// SHA-256 hex digest of a raw token. Matches the scheme
/// `controller/src/api/apikeys.rs::hash_token` already uses for its own API keys,
/// so both layers hash API bearer secrets the same way.
fn hash_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>()
}

/// `head...tail` preview safe to display or persist once the raw secret itself
/// has been discarded (previously computed on the fly in `list_api_tokens` from
/// the live plaintext; now computed once at creation/migration time instead).
fn mask_token(token: &str) -> String {
    let n = token.chars().count();
    if n <= 12 {
        return token.to_string();
    }
    let head: String = token.chars().take(8).collect();
    let tail: String = token.chars().skip(n.saturating_sub(4)).collect();
    format!("{head}...{tail}")
}

/// Directory holding raw plaintext secrets for named/service API tokens (e.g. the
/// `machina-backup` credential `backup.sh` reads to authenticate itself to the
/// daemon). Kept separate from `api-tokens.json`, which stores only a SHA-256
/// hash + masked preview of every token and never the usable secret, so a leak of
/// the general token store (a log bundle, an unencrypted off-box backup copy, an
/// over-permissive file mode) can't be replayed as a live bearer credential.
/// Files here are written mode 0600 (owner/root read-write only).
fn service_token_secret_dir() -> String {
    format!("{DATA_DIR}/service-tokens")
}

fn service_token_secret_path(name: &str) -> Option<std::path::PathBuf> {
    // Guard against path traversal / unexpected characters in a token name.
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(std::path::PathBuf::from(service_token_secret_dir()).join(format!("{name}.token")))
}

fn write_service_token_secret(name: &str, raw_token: &str) {
    let Some(path) = service_token_secret_path(name) else {
        tracing::warn!("skipping service-token secret file for invalid name '{name}'");
        return;
    };
    if std::fs::create_dir_all(service_token_secret_dir()).is_err() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            service_token_secret_dir(),
            std::fs::Permissions::from_mode(0o700),
        );
    }
    if let Err(e) = std::fs::write(&path, raw_token) {
        tracing::warn!("could not write service-token secret file for '{name}': {e}");
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
}

pub fn load_tokens() -> TokenMap {
    // Migration (below) performs a read-modify-write via `save_tokens`, which must
    // run under `JSON_LOCK` — the same lock `create_api_token_scoped`/
    // `delete_api_token` hold while they read-modify-write the same file — or a
    // concurrent create/delete can interleave with the migration write and lose
    // an update. Callers that already hold the lock must use `load_tokens_locked`
    // instead, to avoid re-entering this (non-reentrant) `std::sync::Mutex`.
    with_json_lock(load_tokens_locked)
}

/// Same as `load_tokens`, but assumes `JSON_LOCK` is already held by the caller
/// (e.g. `create_api_token_scoped`, `delete_api_token`). Never acquires the lock
/// itself — doing so would deadlock on the non-reentrant `JSON_LOCK` mutex.
fn load_tokens_locked() -> TokenMap {
    let raw: TokenMap = match std::fs::read_to_string(tokens_path()) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => return HashMap::new(),
    };
    migrate_legacy_plaintext_tokens(raw)
}

/// Installs from before token hashing was added persisted the raw token as both
/// the map key and the `token` field (`token_hash` absent → empty via
/// `#[serde(default)]`). Migrate any such entry on load: move its raw secret to
/// the root-only `service-tokens/<name>.token` sidecar file (so a named service
/// token like `machina-backup` keeps working for scripts that read it), replace
/// the persisted value with a hash + masked preview, and write the migrated map
/// back so this only runs once per token.
fn migrate_legacy_plaintext_tokens(map: TokenMap) -> TokenMap {
    let mut migrated = false;
    let mut out: TokenMap = HashMap::with_capacity(map.len());
    for (key, mut token) in map {
        if token.token_hash.is_empty() {
            let raw = token.token.clone();
            let hash = hash_token(&raw);
            write_service_token_secret(&token.name, &raw);
            token.token_hash = hash.clone();
            token.token = mask_token(&raw);
            out.insert(hash, token);
            migrated = true;
        } else {
            out.insert(key, token);
        }
    }
    if migrated {
        if let Err(e) = save_tokens(&out) {
            tracing::warn!("could not persist migrated api-tokens.json: {e}");
        }
    }
    out
}

fn save_tokens(tokens: &TokenMap) -> Result<(), LibvirtError> {
    let _ = std::fs::create_dir_all(DATA_DIR);
    let data = serde_json::to_string_pretty(tokens)
        .map_err(|e| LibvirtError::Operation(format!("Serialize tokens: {e}")))?;
    let path = tokens_path();
    std::fs::write(&path, data)
        .map_err(|e| LibvirtError::Operation(format!("Write tokens: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn create_api_token(name: &str, username: &str, role: Role) -> Result<ApiToken, LibvirtError> {
    create_api_token_scoped(name, username, role, Vec::new())
}

pub fn create_api_token_scoped(
    name: &str,
    username: &str,
    role: Role,
    scopes: Vec<String>,
) -> Result<ApiToken, LibvirtError> {
    with_json_lock(|| {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let bytes: [u8; 32] = rng.gen();
        let token = format!("mach_{}", hex::encode(bytes));
        let hash = hash_token(&token);

        // Never persist the raw secret — only its hash and a masked preview.
        let stored = ApiToken {
            name: name.to_string(),
            token: mask_token(&token),
            token_hash: hash.clone(),
            username: username.to_string(),
            role,
            created: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            scopes,
        };

        let mut tokens = load_tokens_locked();
        tokens.insert(hash, stored.clone());
        save_tokens(&tokens)?;

        // The raw token is returned to the immediate caller only (e.g. the
        // `POST /tokens` HTTP response) — this is the one and only time it is
        // ever available; it is never written to api-tokens.json.
        Ok(ApiToken { token, ..stored })
    })
}

/// Get-or-create a service token identified by `name`. Returns the existing token if
/// one already exists with this name, otherwise mints a new one. Used to provision a
/// stable machine credential (e.g. for the backup script, which must authenticate to
/// the daemon's read APIs) without minting a fresh token on every startup.
///
/// Service tokens are consumed by local root-owned scripts that need the raw
/// secret, not just its hash, so the newly minted raw value is additionally
/// persisted to the 0600 `service-tokens/<name>.token` sidecar file — never to
/// api-tokens.json, which only ever stores the hash (see `create_api_token_scoped`).
pub fn ensure_named_token(
    name: &str,
    username: &str,
    role: Role,
    scopes: Vec<String>,
) -> Result<ApiToken, LibvirtError> {
    if let Some(existing) = load_tokens().into_values().find(|t| t.name == name) {
        return Ok(existing);
    }
    let created = create_api_token_scoped(name, username, role, scopes)?;
    write_service_token_secret(name, &created.token);
    Ok(created)
}

pub fn validate_api_token(token: &str) -> Option<ApiToken> {
    let tokens = load_tokens();
    tokens
        .get(&hash_token(token))
        .cloned()
        .inspect(|_| crate::obs_counters::inc_api_token_ok())
}

pub fn delete_api_token(token: &str) -> Result<(), LibvirtError> {
    with_json_lock(|| {
        let mut tokens = load_tokens_locked();
        // HashMap::remove's return value was previously discarded, so deleting a
        // nonexistent/already-revoked token still returned Ok(()) — the daemon
        // route then reported {"status": "deleted"} even though nothing was
        // removed. Surface a NotFound instead of a false success.
        if tokens.remove(&hash_token(token)).is_none() {
            return Err(LibvirtError::NotFound("API token not found".into()));
        }
        save_tokens(&tokens)
    })
}

#[cfg(test)]
mod token_scope_tests {
    use super::*;

    #[test]
    fn wildcard_allows_write() {
        assert!(token_allows(&["*".into()], "vms:write"));
    }

    #[test]
    fn prefix_wildcard() {
        assert!(token_allows(&["vms:*".into()], "vms:write"));
        assert!(!token_allows(&["vms:read".into()], "vms:write"));
    }
}

pub fn list_api_tokens() -> Vec<ApiToken> {
    // `token` is already a masked `head...tail` preview as persisted by
    // `create_api_token_scoped` / migrated by `load_tokens` — the raw secret is
    // never stored, so there is nothing left to mask here.
    load_tokens().into_values().collect()
}

// ── Alerts ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: String,
    pub name: String,
    pub condition: String, // cpu_percent > 90, disk_percent > 85, vm_down
    pub threshold: f64,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: String,
    pub rule_name: String,
    pub message: String,
    pub severity: String, // warning, critical
    pub timestamp: String,
    pub acknowledged: bool,
}

fn alerts_path() -> String {
    format!("{DATA_DIR}/alerts.json")
}
fn alert_rules_path() -> String {
    format!("{DATA_DIR}/alert-rules.json")
}

pub fn load_alert_rules() -> Vec<AlertRule> {
    match std::fs::read_to_string(alert_rules_path()) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => default_alert_rules(),
    }
}

fn default_alert_rules() -> Vec<AlertRule> {
    vec![
        AlertRule {
            id: "cpu-high".into(),
            name: "High CPU".into(),
            condition: "cpu_percent".into(),
            threshold: 90.0,
            enabled: true,
        },
        AlertRule {
            id: "mem-high".into(),
            name: "High Memory".into(),
            condition: "memory_percent".into(),
            threshold: 90.0,
            enabled: true,
        },
        AlertRule {
            id: "disk-high".into(),
            name: "Disk Full".into(),
            condition: "disk_percent".into(),
            threshold: 85.0,
            enabled: true,
        },
        AlertRule {
            id: "vm-mem-high".into(),
            name: "VM High Memory".into(),
            condition: "vm_memory_percent".into(),
            threshold: 90.0,
            enabled: false,
        },
        AlertRule {
            id: "vm-cpu-high".into(),
            name: "VM High CPU".into(),
            condition: "vm_cpu_percent".into(),
            threshold: 90.0,
            enabled: false,
        },
        AlertRule {
            id: "vm-down".into(),
            name: "Autostart VM Down".into(),
            condition: "vm_down".into(),
            threshold: 0.0,
            enabled: false,
        },
    ]
}

pub fn save_alert_rules(rules: &[AlertRule]) -> Result<(), LibvirtError> {
    with_json_lock(|| {
        let _ = std::fs::create_dir_all(DATA_DIR);
        let data = serde_json::to_string_pretty(rules)
            .map_err(|e| LibvirtError::Operation(format!("Serialize alert rules: {e}")))?;
        std::fs::write(alert_rules_path(), data)
            .map_err(|e| LibvirtError::Operation(format!("Write alert rules: {e}")))?;
        Ok(())
    })
}

pub fn load_alerts() -> Vec<Alert> {
    match std::fs::read_to_string(alerts_path()) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn save_alert(alert: &Alert) -> Result<(), LibvirtError> {
    with_json_lock(|| {
        let mut alerts = load_alerts();
        // Keep only last 100 alerts
        if alerts.len() > 100 {
            alerts.drain(0..alerts.len() - 100);
        }
        alerts.push(alert.clone());
        let _ = std::fs::create_dir_all(DATA_DIR);
        let data = serde_json::to_string_pretty(&alerts)
            .map_err(|e| LibvirtError::Operation(format!("Serialize alerts: {e}")))?;
        std::fs::write(alerts_path(), data)
            .map_err(|e| LibvirtError::Operation(format!("Write alerts: {e}")))?;
        Ok(())
    })
}

pub fn acknowledge_alert(id: &str) -> Result<(), LibvirtError> {
    with_json_lock(|| {
        let mut alerts = load_alerts();
        for a in &mut alerts {
            if a.id == id {
                a.acknowledged = true;
            }
        }
        let data = serde_json::to_string_pretty(&alerts)
            .map_err(|e| LibvirtError::Operation(format!("{e}")))?;
        std::fs::write(alerts_path(), data).map_err(|e| LibvirtError::Operation(format!("{e}")))?;
        Ok(())
    })
}

// ── Webhooks ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub id: String,
    pub url: String,
    pub events: Vec<String>, // vm_started, vm_stopped, alert_fired, etc.
    pub enabled: bool,
}

fn webhooks_path() -> String {
    format!("{DATA_DIR}/webhooks.json")
}

pub fn load_webhooks() -> Vec<WebhookConfig> {
    match std::fs::read_to_string(webhooks_path()) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn save_webhooks(hooks: &[WebhookConfig]) -> Result<(), LibvirtError> {
    with_json_lock(|| {
        let _ = std::fs::create_dir_all(DATA_DIR);
        let data = serde_json::to_string_pretty(hooks)
            .map_err(|e| LibvirtError::Operation(format!("{e}")))?;
        std::fs::write(webhooks_path(), data)
            .map_err(|e| LibvirtError::Operation(format!("{e}")))?;
        Ok(())
    })
}

pub fn fire_webhook(event: &str, payload: &serde_json::Value) {
    let hooks = load_webhooks();
    for hook in hooks {
        if !hook.enabled {
            continue;
        }
        if !hook.events.contains(&event.to_string()) && !hook.events.contains(&"*".to_string()) {
            continue;
        }
        let url = hook.url.clone();
        // Validate URL scheme to prevent arbitrary command injection
        if !url.starts_with("http://") && !url.starts_with("https://") {
            continue;
        }
        let body = serde_json::json!({ "event": event, "data": payload }).to_string();
        // Fire and forget in background
        std::thread::spawn(move || {
            let _ = std::process::Command::new("curl")
                .args([
                    "-sf",
                    // Bound the request so a hung / slow-loris webhook endpoint can't pin
                    // this thread + curl child indefinitely; without these a burst of VM
                    // lifecycle events against a black-hole endpoint piles up threads.
                    "--connect-timeout",
                    "5",
                    "--max-time",
                    "15",
                    "-X",
                    "POST",
                    "-H",
                    "Content-Type: application/json",
                    "-d",
                    &body,
                    "--",
                    &url,
                ])
                .output();
        });
    }
}

/// Emit a VM lifecycle webhook (`vm_started`, `vm_stopped`, etc.).
pub fn fire_vm_event(event: &str, vm_name: &str, extra: &serde_json::Value) {
    let mut payload = serde_json::json!({
        "vm": vm_name,
        "timestamp": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    });
    if let Some(obj) = payload.as_object_mut() {
        if let Some(map) = extra.as_object() {
            for (k, v) in map {
                obj.insert(k.clone(), v.clone());
            }
        }
    }
    fire_webhook(event, &payload);
}

// ── Scheduled Actions ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledAction {
    pub id: String,
    pub vm_name: String,
    pub action: String,   // start, stop, shutdown, snapshot, reboot
    pub schedule: String, // cron-like: "0 22 * * *" or simple: "daily 22:00"
    pub enabled: bool,
    pub last_run: String,
}

fn schedules_path() -> String {
    format!("{DATA_DIR}/schedules.json")
}

pub fn load_schedules() -> Vec<ScheduledAction> {
    match std::fs::read_to_string(schedules_path()) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn save_schedules(schedules: &[ScheduledAction]) -> Result<(), LibvirtError> {
    with_json_lock(|| {
        let _ = std::fs::create_dir_all(DATA_DIR);
        let data = serde_json::to_string_pretty(schedules)
            .map_err(|e| LibvirtError::Operation(format!("{e}")))?;
        std::fs::write(schedules_path(), data)
            .map_err(|e| LibvirtError::Operation(format!("{e}")))?;
        Ok(())
    })
}

// ── Notification Channels ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationChannel {
    pub id: String,
    pub channel_type: String, // slack, email, telegram, webhook
    pub config: String,       // webhook URL, email address, or bot token
    pub enabled: bool,
}

fn notifications_path() -> String {
    format!("{DATA_DIR}/notifications.json")
}

pub fn load_notification_channels() -> Vec<NotificationChannel> {
    match std::fs::read_to_string(notifications_path()) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn save_notification_channels(channels: &[NotificationChannel]) -> Result<(), LibvirtError> {
    with_json_lock(|| {
        let _ = std::fs::create_dir_all(DATA_DIR);
        let data = serde_json::to_string_pretty(channels)
            .map_err(|e| LibvirtError::Operation(format!("Serialize notifications: {e}")))?;
        std::fs::write(notifications_path(), data)
            .map_err(|e| LibvirtError::Operation(format!("Write notifications: {e}")))?;
        Ok(())
    })
}

/// Send a notification through the given channel.
pub fn send_notification(
    channel: &NotificationChannel,
    subject: &str,
    message: &str,
) -> Result<(), LibvirtError> {
    if !channel.enabled {
        return Ok(());
    }
    match channel.channel_type.as_str() {
        "slack" => {
            let body = serde_json::json!({ "text": format!("{subject}: {message}") }).to_string();
            let url = channel.config.clone();
            // Validate scheme + pass "--" so a config value starting with '-' can't be
            // read by curl as an option (e.g. -o/path → arbitrary file write as the
            // daemon user). Mirrors fire_webhook.
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return Err(LibvirtError::Invalid(
                    "Slack webhook URL must start with http:// or https://".into(),
                ));
            }
            std::process::Command::new("curl")
                .args([
                    "-sf",
                    "-X",
                    "POST",
                    "-H",
                    "Content-Type: application/json",
                    "-d",
                    &body,
                    "--",
                    &url,
                ])
                .output()
                .map_err(|e| LibvirtError::Operation(format!("Slack notification failed: {e}")))?;
            Ok(())
        }
        "email" => {
            let addr = &channel.config;
            // Validate email address: must contain @, no spaces or newlines
            if !addr.contains('@')
                || addr.contains(' ')
                || addr.contains('\n')
                || addr.contains('\r')
            {
                return Err(LibvirtError::Invalid("Invalid email address".to_string()));
            }
            // `addr` is passed as a bare positional argv element to `sendmail` below
            // with no `--` separator ahead of it (sendmail's own `--` support is not
            // reliable across implementations, unlike curl above), so a value like
            // "-C/tmp/evil.cf@x" (which still contains '@' and no space/newline, so
            // it would otherwise pass the check above) would be parsed by sendmail
            // as a command-line option instead of a recipient address.
            if addr.starts_with('-') {
                return Err(LibvirtError::Invalid("Invalid email address".to_string()));
            }
            // Sanitize subject to prevent header injection
            let safe_subject: String = subject
                .chars()
                .filter(|c| *c != '\r' && *c != '\n')
                .collect();
            let full_message = format!("Subject: {safe_subject}\n\n{message}");
            let mut child = std::process::Command::new("sendmail")
                .arg(addr)
                .stdin(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| LibvirtError::Operation(format!("sendmail failed: {e}")))?;
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = stdin.write_all(full_message.as_bytes());
            }
            let _ = child.wait();
            Ok(())
        }
        "telegram" => {
            // config format: "bot_token|chat_id" (use | delimiter because bot tokens contain ':')
            let parts: Vec<&str> = channel.config.splitn(2, '|').collect();
            if parts.len() != 2 {
                return Err(LibvirtError::Invalid(
                    "Telegram config must be bot_token|chat_id".into(),
                ));
            }
            let url = format!("https://api.telegram.org/bot{}/sendMessage", parts[0]);
            let body =
                serde_json::json!({ "chat_id": parts[1], "text": format!("{subject}: {message}") })
                    .to_string();
            std::process::Command::new("curl")
                .args([
                    "-sf",
                    "-X",
                    "POST",
                    "-H",
                    "Content-Type: application/json",
                    "-d",
                    &body,
                    &url,
                ])
                .output()
                .map_err(|e| {
                    LibvirtError::Operation(format!("Telegram notification failed: {e}"))
                })?;
            Ok(())
        }
        "webhook" => {
            let body = serde_json::json!({ "subject": subject, "message": message }).to_string();
            let url = channel.config.clone();
            // Validate scheme + "--" so a config starting with '-' can't be parsed by
            // curl as an option (arbitrary file write/read as the daemon user).
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return Err(LibvirtError::Invalid(
                    "Webhook URL must start with http:// or https://".into(),
                ));
            }
            std::process::Command::new("curl")
                .args([
                    "-sf",
                    "-X",
                    "POST",
                    "-H",
                    "Content-Type: application/json",
                    "-d",
                    &body,
                    "--",
                    &url,
                ])
                .output()
                .map_err(|e| {
                    LibvirtError::Operation(format!("Webhook notification failed: {e}"))
                })?;
            Ok(())
        }
        other => Err(LibvirtError::Invalid(format!(
            "Unknown channel type: {other}"
        ))),
    }
}

// ── Snapshot Schedules ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotSchedule {
    pub id: String,
    pub vm_name: String,
    pub interval_hours: u32,
    pub retain_count: u32,
    pub enabled: bool,
    pub last_run: String,
}

fn snapshot_schedules_path() -> String {
    format!("{DATA_DIR}/snapshot-schedules.json")
}

pub fn load_snapshot_schedules() -> Vec<SnapshotSchedule> {
    match std::fs::read_to_string(snapshot_schedules_path()) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn save_snapshot_schedules(schedules: &[SnapshotSchedule]) -> Result<(), LibvirtError> {
    with_json_lock(|| {
        let _ = std::fs::create_dir_all(DATA_DIR);
        let data = serde_json::to_string_pretty(schedules)
            .map_err(|e| LibvirtError::Operation(format!("Serialize snapshot schedules: {e}")))?;
        std::fs::write(snapshot_schedules_path(), data)
            .map_err(|e| LibvirtError::Operation(format!("Write snapshot schedules: {e}")))?;
        Ok(())
    })
}

/// Check if a schedule should run now (simple daily HH:MM matching).
pub fn should_run_now(schedule: &str) -> bool {
    let now = chrono::Local::now();
    // Parse "daily HH:MM" format
    if let Some(time) = schedule.strip_prefix("daily ") {
        let parts: Vec<&str> = time.split(':').collect();
        if parts.len() == 2 {
            let hour: u32 = parts[0].parse().unwrap_or(99);
            let min: u32 = parts[1].parse().unwrap_or(99);
            return now.format("%H").to_string().parse::<u32>().unwrap_or(0) == hour
                && now.format("%M").to_string().parse::<u32>().unwrap_or(0) == min;
        }
    }
    false
}
