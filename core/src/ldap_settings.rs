// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! API-facing LDAP / Active Directory settings (subset of [`MachinaConfig::auth.ldap`]).

use serde::{Deserialize, Serialize};

use crate::config::{LdapConfig, MachinaConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LdapSettingsView {
    pub enabled: bool,
    pub url: String,
    pub base_dn: String,
    pub user_filter: String,
    pub bind_dn: String,
    pub bind_password: String,
    pub bind_password_set: bool,
    pub user_dn_template: String,
    pub username_attribute: String,
    pub use_tls: bool,
    pub insecure_tls: bool,
    pub member_attribute: String,
    pub admin_group_substrings: Vec<String>,
    pub operator_group_substrings: Vec<String>,
    pub readonly_group_substrings: Vec<String>,
    pub config_path: String,
    /// Preset id when settings match a known template (e.g. `zyvorai-local`).
    pub preset: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LdapSettingsPatch {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub base_dn: Option<String>,
    #[serde(default)]
    pub user_filter: Option<String>,
    #[serde(default)]
    pub bind_dn: Option<String>,
    /// Set to empty string to clear; `***` leaves unchanged.
    #[serde(default)]
    pub bind_password: Option<String>,
    #[serde(default)]
    pub user_dn_template: Option<String>,
    #[serde(default)]
    pub username_attribute: Option<String>,
    #[serde(default)]
    pub use_tls: Option<bool>,
    #[serde(default)]
    pub insecure_tls: Option<bool>,
    #[serde(default)]
    pub member_attribute: Option<String>,
    #[serde(default)]
    pub admin_group_substrings: Option<Vec<String>>,
    #[serde(default)]
    pub operator_group_substrings: Option<Vec<String>>,
    #[serde(default)]
    pub readonly_group_substrings: Option<Vec<String>>,
    /// Apply a known preset (`zyvorai-local`) — overwrites ldap fields except bind_password when omitted.
    #[serde(default)]
    pub preset: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LdapTestRequest {
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LdapTestResponse {
    pub ok: bool,
    pub message: String,
    pub username: Option<String>,
    pub role: Option<String>,
}

fn mask_secret(value: &str) -> String {
    let v = value.trim();
    if v.is_empty() {
        return String::new();
    }
    "***".into()
}

fn config_path_display() -> String {
    if MachinaConfig::system_config_path().exists() {
        MachinaConfig::system_config_path().display().to_string()
    } else {
        MachinaConfig::user_config_path().display().to_string()
    }
}

/// Active Directory preset for `zyvorai.local` (45.82.66.172).
pub fn zyvorai_local_preset() -> LdapConfig {
    LdapConfig {
        enabled: true,
        url: "ldap://45.82.66.172:389".into(),
        base_dn: "DC=zyvorai,DC=local".into(),
        user_filter: "(|(sAMAccountName={username})(userPrincipalName={username}))".into(),
        bind_dn: String::new(),
        bind_password: String::new(),
        user_dn_template: String::new(),
        username_attribute: "sAMAccountName".into(),
        use_tls: false,
        insecure_tls: false,
        member_attribute: "memberOf".into(),
        admin_group_substrings: vec!["Domain Admins".into(), "Machina-Admins".into()],
        operator_group_substrings: vec!["Machina-Operators".into()],
        readonly_group_substrings: vec!["Domain Users".into()],
    }
}

fn detect_preset(ldap: &LdapConfig) -> Option<String> {
    let preset = zyvorai_local_preset();
    if ldap.url.trim() == preset.url
        && ldap.base_dn.eq_ignore_ascii_case(&preset.base_dn)
        && ldap.user_filter == preset.user_filter
    {
        Some("zyvorai-local".into())
    } else {
        None
    }
}

pub fn ldap_settings_view_from_config(cfg: &MachinaConfig) -> LdapSettingsView {
    let ldap = &cfg.auth.ldap;
    LdapSettingsView {
        enabled: ldap.enabled,
        url: ldap.url.clone(),
        base_dn: ldap.base_dn.clone(),
        user_filter: ldap.user_filter.clone(),
        bind_dn: ldap.bind_dn.clone(),
        bind_password: mask_secret(&ldap.bind_password),
        bind_password_set: !ldap.bind_password.trim().is_empty(),
        user_dn_template: ldap.user_dn_template.clone(),
        username_attribute: ldap.username_attribute.clone(),
        use_tls: ldap.use_tls,
        insecure_tls: ldap.insecure_tls,
        member_attribute: ldap.member_attribute.clone(),
        admin_group_substrings: ldap.admin_group_substrings.clone(),
        operator_group_substrings: ldap.operator_group_substrings.clone(),
        readonly_group_substrings: ldap.readonly_group_substrings.clone(),
        config_path: config_path_display(),
        preset: detect_preset(ldap),
    }
}

pub fn apply_ldap_patch(cfg: &mut MachinaConfig, patch: &LdapSettingsPatch) {
    if let Some(preset) = patch.preset.as_deref() {
        if preset == "zyvorai-local" {
            cfg.auth.ldap = zyvorai_local_preset();
        }
    }
    let ldap = &mut cfg.auth.ldap;
    if let Some(v) = patch.enabled {
        ldap.enabled = v;
    }
    if let Some(v) = &patch.url {
        ldap.url = v.clone();
    }
    if let Some(v) = &patch.base_dn {
        ldap.base_dn = v.clone();
    }
    if let Some(v) = &patch.user_filter {
        ldap.user_filter = v.clone();
    }
    if let Some(v) = &patch.bind_dn {
        ldap.bind_dn = v.clone();
    }
    if let Some(v) = &patch.bind_password {
        if v != "***" {
            ldap.bind_password = v.clone();
        }
    }
    if let Some(v) = &patch.user_dn_template {
        ldap.user_dn_template = v.clone();
    }
    if let Some(v) = &patch.username_attribute {
        ldap.username_attribute = v.clone();
    }
    if let Some(v) = patch.use_tls {
        ldap.use_tls = v;
    }
    if let Some(v) = patch.insecure_tls {
        ldap.insecure_tls = v;
    }
    if let Some(v) = &patch.member_attribute {
        ldap.member_attribute = v.clone();
    }
    if let Some(v) = &patch.admin_group_substrings {
        ldap.admin_group_substrings = v.clone();
    }
    if let Some(v) = &patch.operator_group_substrings {
        ldap.operator_group_substrings = v.clone();
    }
    if let Some(v) = &patch.readonly_group_substrings {
        ldap.readonly_group_substrings = v.clone();
    }
}
