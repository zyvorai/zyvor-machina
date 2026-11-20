// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Admin API views for OIDC and SAML identity settings.

use serde::{Deserialize, Serialize};

use crate::config::{MachinaConfig, OidcDefaultRole};

fn config_path_display() -> String {
    if MachinaConfig::system_config_path().exists() {
        MachinaConfig::system_config_path().display().to_string()
    } else {
        MachinaConfig::user_config_path().display().to_string()
    }
}

fn mask_secret(value: &str) -> String {
    let v = value.trim();
    if v.is_empty() {
        return String::new();
    }
    "***".into()
}

/// Sentinel returned in place of stored SAML IdP metadata XML (never echoed back in
/// full — it can be large and isn't rendered as a plain "***" like other secrets).
/// `apply_saml_patch` must compare against this same literal so that a client which
/// round-trips the view unchanged doesn't clobber the stored metadata with the
/// placeholder text.
const SAML_METADATA_XML_PLACEHOLDER: &str = "[stored — paste to replace]";

fn role_label(role: OidcDefaultRole) -> &'static str {
    match role {
        OidcDefaultRole::Admin => "admin",
        OidcDefaultRole::Operator => "operator",
        OidcDefaultRole::ReadOnly => "readonly",
    }
}

fn parse_default_role(raw: &str) -> Option<OidcDefaultRole> {
    match raw.trim().to_lowercase().as_str() {
        "admin" => Some(OidcDefaultRole::Admin),
        "operator" => Some(OidcDefaultRole::Operator),
        "readonly" | "read_only" | "read-only" => Some(OidcDefaultRole::ReadOnly),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcSettingsView {
    pub enabled: bool,
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub client_secret_set: bool,
    pub redirect_url: String,
    pub scopes: Vec<String>,
    pub username_claim: String,
    pub groups_claim: String,
    pub linux_username_claim: String,
    pub admin_groups: Vec<String>,
    pub operator_groups: Vec<String>,
    pub default_role: String,
    pub button_label: String,
    pub require_local_user_for_session_libvirt: bool,
    pub config_path: String,
    pub login_path: String,
    pub callback_path: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct OidcSettingsPatch {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub issuer_url: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    /// Set to empty string to clear; `***` leaves unchanged.
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub redirect_url: Option<String>,
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
    #[serde(default)]
    pub username_claim: Option<String>,
    #[serde(default)]
    pub groups_claim: Option<String>,
    #[serde(default)]
    pub linux_username_claim: Option<String>,
    #[serde(default)]
    pub admin_groups: Option<Vec<String>>,
    #[serde(default)]
    pub operator_groups: Option<Vec<String>>,
    #[serde(default)]
    pub default_role: Option<String>,
    #[serde(default)]
    pub button_label: Option<String>,
    #[serde(default)]
    pub require_local_user_for_session_libvirt: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamlSettingsView {
    pub enabled: bool,
    pub configured: bool,
    pub sp_entity_id: String,
    pub sp_acs_url: String,
    pub idp_entity_id: String,
    pub idp_metadata_url: String,
    pub idp_metadata_xml: String,
    pub idp_metadata_xml_set: bool,
    pub name_id_format: String,
    pub button_label: String,
    pub admin_groups: Vec<String>,
    pub operator_groups: Vec<String>,
    pub default_role: String,
    pub notes: String,
    pub config_path: String,
    pub metadata_path: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SamlSettingsPatch {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub sp_entity_id: Option<String>,
    #[serde(default)]
    pub sp_acs_url: Option<String>,
    #[serde(default)]
    pub idp_entity_id: Option<String>,
    #[serde(default)]
    pub idp_metadata_url: Option<String>,
    #[serde(default)]
    pub idp_metadata_xml: Option<String>,
    #[serde(default)]
    pub name_id_format: Option<String>,
    #[serde(default)]
    pub button_label: Option<String>,
    #[serde(default)]
    pub admin_groups: Option<Vec<String>>,
    #[serde(default)]
    pub operator_groups: Option<Vec<String>>,
    #[serde(default)]
    pub default_role: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

pub fn oidc_settings_view_from_config(cfg: &MachinaConfig) -> OidcSettingsView {
    let oidc = &cfg.auth.oidc;
    OidcSettingsView {
        enabled: oidc.enabled,
        issuer_url: oidc.issuer_url.clone(),
        client_id: oidc.client_id.clone(),
        client_secret: mask_secret(&oidc.client_secret),
        client_secret_set: !oidc.client_secret.trim().is_empty(),
        redirect_url: oidc.redirect_url.clone(),
        scopes: oidc.scopes.clone(),
        username_claim: oidc.username_claim.clone(),
        groups_claim: oidc.groups_claim.clone(),
        linux_username_claim: oidc.linux_username_claim.clone(),
        admin_groups: oidc.admin_groups.clone(),
        operator_groups: oidc.operator_groups.clone(),
        default_role: role_label(oidc.default_role).into(),
        button_label: oidc.button_label.clone(),
        require_local_user_for_session_libvirt: oidc.require_local_user_for_session_libvirt,
        config_path: config_path_display(),
        login_path: "/api/v1/auth/oidc/login".into(),
        callback_path: "/api/v1/auth/oidc/callback".into(),
    }
}

pub fn apply_oidc_patch(cfg: &mut MachinaConfig, patch: &OidcSettingsPatch) {
    let oidc = &mut cfg.auth.oidc;
    if let Some(v) = patch.enabled {
        oidc.enabled = v;
    }
    if let Some(v) = &patch.issuer_url {
        oidc.issuer_url = v.clone();
    }
    if let Some(v) = &patch.client_id {
        oidc.client_id = v.clone();
    }
    if let Some(v) = &patch.client_secret {
        if v != "***" {
            oidc.client_secret = v.clone();
        }
    }
    if let Some(v) = &patch.redirect_url {
        oidc.redirect_url = v.clone();
    }
    if let Some(v) = &patch.scopes {
        oidc.scopes = v.clone();
    }
    if let Some(v) = &patch.username_claim {
        oidc.username_claim = v.clone();
    }
    if let Some(v) = &patch.groups_claim {
        oidc.groups_claim = v.clone();
    }
    if let Some(v) = &patch.linux_username_claim {
        oidc.linux_username_claim = v.clone();
    }
    if let Some(v) = &patch.admin_groups {
        oidc.admin_groups = v.clone();
    }
    if let Some(v) = &patch.operator_groups {
        oidc.operator_groups = v.clone();
    }
    if let Some(v) = &patch.default_role {
        if let Some(role) = parse_default_role(v) {
            oidc.default_role = role;
        }
    }
    if let Some(v) = &patch.button_label {
        oidc.button_label = v.clone();
    }
    if let Some(v) = patch.require_local_user_for_session_libvirt {
        oidc.require_local_user_for_session_libvirt = v;
    }
}

pub fn saml_settings_view_from_config(cfg: &MachinaConfig) -> SamlSettingsView {
    let saml = &cfg.auth.saml;
    SamlSettingsView {
        enabled: saml.enabled,
        configured: saml.is_configured(),
        sp_entity_id: saml.sp_entity_id.clone(),
        sp_acs_url: saml.sp_acs_url.clone(),
        idp_entity_id: saml.idp_entity_id.clone(),
        idp_metadata_url: saml.idp_metadata_url.clone(),
        idp_metadata_xml: if saml.idp_metadata_xml.trim().is_empty() {
            String::new()
        } else {
            SAML_METADATA_XML_PLACEHOLDER.into()
        },
        idp_metadata_xml_set: !saml.idp_metadata_xml.trim().is_empty(),
        name_id_format: saml.name_id_format.clone(),
        button_label: saml.button_label.clone(),
        admin_groups: saml.admin_groups.clone(),
        operator_groups: saml.operator_groups.clone(),
        default_role: role_label(saml.default_role).into(),
        notes: saml.notes.clone(),
        config_path: config_path_display(),
        metadata_path: "/api/v1/auth/saml/metadata".into(),
    }
}

pub fn apply_saml_patch(cfg: &mut MachinaConfig, patch: &SamlSettingsPatch) {
    let saml = &mut cfg.auth.saml;
    if let Some(v) = patch.enabled {
        saml.enabled = v;
    }
    if let Some(v) = &patch.sp_entity_id {
        saml.sp_entity_id = v.clone();
    }
    if let Some(v) = &patch.sp_acs_url {
        saml.sp_acs_url = v.clone();
    }
    if let Some(v) = &patch.idp_entity_id {
        saml.idp_entity_id = v.clone();
    }
    if let Some(v) = &patch.idp_metadata_url {
        saml.idp_metadata_url = v.clone();
    }
    if let Some(v) = &patch.idp_metadata_xml {
        if v != SAML_METADATA_XML_PLACEHOLDER {
            saml.idp_metadata_xml = v.clone();
        }
    }
    if let Some(v) = &patch.name_id_format {
        saml.name_id_format = v.clone();
    }
    if let Some(v) = &patch.button_label {
        saml.button_label = v.clone();
    }
    if let Some(v) = &patch.admin_groups {
        saml.admin_groups = v.clone();
    }
    if let Some(v) = &patch.operator_groups {
        saml.operator_groups = v.clone();
    }
    if let Some(v) = &patch.default_role {
        if let Some(role) = parse_default_role(v) {
            saml.default_role = role;
        }
    }
    if let Some(v) = &patch.notes {
        saml.notes = v.clone();
    }
}
