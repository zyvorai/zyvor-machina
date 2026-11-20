// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! LDAP / Active Directory simple-bind authentication for web login.

use ldap3::{LdapConn, LdapConnSettings, Scope, SearchEntry};
use machina_core::config::LdapConfig;
use machina_core::ldap_role::role_from_ldap_groups;
use machina_core::libvirt::automation::Role;
use tracing::warn;

pub struct LdapAuthResult {
    pub username: String,
    pub role: Role,
}

/// Run blocking LDAP I/O off the async runtime (ldap3 `sync` cannot block inside Tokio workers).
pub async fn ldap_authenticate_async(
    cfg: &LdapConfig,
    username: &str,
    password: &str,
) -> Result<LdapAuthResult, String> {
    let cfg = cfg.clone();
    let username = username.to_string();
    let password = password.to_string();
    tokio::task::spawn_blocking(move || ldap_authenticate(&cfg, &username, &password))
        .await
        .map_err(|e| format!("LDAP worker failed: {e}"))?
}

fn open_ldap(url: &str, cfg: &LdapConfig) -> Result<LdapConn, String> {
    let mut settings = LdapConnSettings::new();
    if cfg.insecure_tls {
        settings = settings.set_no_tls_verify(true);
    }
    if cfg.use_tls && url.starts_with("ldap://") {
        settings = settings.set_starttls(true);
    }
    LdapConn::with_settings(settings, url).map_err(|e| format!("LDAP connect failed: {e}"))
}

pub fn ldap_authenticate(
    cfg: &LdapConfig,
    username: &str,
    password: &str,
) -> Result<LdapAuthResult, String> {
    if !cfg.is_enabled() {
        return Err("LDAP is not enabled".into());
    }
    let url = cfg.url.trim();
    if url.is_empty() {
        return Err("LDAP url is not configured".into());
    }
    // RFC 4513 §5.1.2: a simple bind with a non-empty DN but an *empty* password is an
    // "unauthenticated bind" that most LDAP servers accept as success without checking
    // any credential. Every current caller (web login, admin LDAP-test) already rejects
    // empty passwords before reaching here, but this module escapes filter values
    // defensively for the same reason (don't trust callers) — mirror that here too, or a
    // future caller that forgets the check turns "wrong password" into "any password".
    if password.is_empty() {
        return Err("LDAP password must not be empty".into());
    }

    let mut ldap = open_ldap(url, cfg)?;

    if username.contains('@')
        && cfg.bind_dn.trim().is_empty()
        && cfg.user_dn_template.trim().is_empty()
    {
        ldap.simple_bind(username, password)
            .map_err(|e| format!("LDAP bind failed: {e}"))?
            .success()
            .map_err(|e| format!("LDAP authentication failed: {e}"))?;
        let groups = lookup_groups_for_upn(cfg, &mut ldap, username).unwrap_or_default();
        let role = role_from_ldap_groups(cfg, &groups);
        return Ok(LdapAuthResult {
            username: session_username_from_input(username),
            role,
        });
    }

    let (user_dn, groups, session_username) = resolve_user_dn_and_groups(cfg, &mut ldap, username)?;
    ldap.simple_bind(&user_dn, password)
        .map_err(|e| format!("LDAP bind failed: {e}"))?
        .success()
        .map_err(|e| format!("LDAP authentication failed: {e}"))?;

    let role = role_from_ldap_groups(cfg, &groups);
    Ok(LdapAuthResult {
        username: session_username,
        role,
    })
}

fn session_username_from_input(raw: &str) -> String {
    if let Some((left, _)) = raw.split_once('@') {
        return left.to_string();
    }
    raw.to_string()
}

/// Escape a value per RFC 4515 before interpolating it into an LDAP search filter.
/// The web login path already restricts usernames to a safe character set (see
/// `daemon/src/auth.rs::login_handler`), but this module is also reachable from the
/// admin "LDAP test" endpoint, which does not apply that whitelist — escaping here
/// closes LDAP filter injection (e.g. `*`, `(`, `)`, `\`) regardless of caller.
fn escape_ldap_filter_value(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '\\' => out.push_str("\\5c"),
            '*' => out.push_str("\\2a"),
            '(' => out.push_str("\\28"),
            ')' => out.push_str("\\29"),
            '\0' => out.push_str("\\00"),
            _ => out.push(c),
        }
    }
    out
}

fn build_user_filter(cfg: &LdapConfig, username: &str) -> String {
    let escaped = escape_ldap_filter_value(username);
    if username.contains('@') {
        return format!("(userPrincipalName={escaped})");
    }
    cfg.user_filter.replace("{username}", &escaped)
}

fn resolve_user_dn_and_groups(
    cfg: &LdapConfig,
    ldap: &mut LdapConn,
    username: &str,
) -> Result<(String, Vec<String>, String), String> {
    let session_username = session_username_from_input(username);

    let template = cfg.user_dn_template.trim();
    if !template.is_empty() {
        let dn = template.replace("{username}", username);
        let groups = fetch_groups_for_dn(cfg, ldap, &dn)?;
        return Ok((dn, groups, session_username));
    }

    let bind_dn = cfg.bind_dn.trim();
    if !bind_dn.is_empty() {
        ldap.simple_bind(bind_dn, cfg.bind_password.trim())
            .map_err(|e| format!("LDAP service bind failed: {e}"))?
            .success()
            .map_err(|e| format!("LDAP service bind failed: {e}"))?;

        let filter = build_user_filter(cfg, username);
        let base = cfg.base_dn.trim();
        if base.is_empty() {
            return Err("LDAP base_dn required when using bind_dn + user_filter".into());
        }
        let attrs = vec!["dn", cfg.member_attribute.as_str()];
        let (rs, _) = ldap
            .search(base, Scope::Subtree, &filter, attrs)
            .map_err(|e| format!("LDAP search failed: {e}"))?
            .success()
            .map_err(|e| format!("LDAP search failed: {e}"))?;
        let entry = rs
            .into_iter()
            .map(SearchEntry::construct)
            .find(|e| !e.dn.is_empty())
            .ok_or_else(|| "LDAP user not found".to_string())?;
        let groups = entry
            .attrs
            .get(&cfg.member_attribute)
            .cloned()
            .unwrap_or_default();
        return Ok((entry.dn, groups, session_username));
    }

    warn!(
        "LDAP: configure user_dn_template, bind_dn+base_dn+user_filter, or sign in with user@domain UPN for user '{}'",
        username
    );
    Err("LDAP user DN resolution is not configured".into())
}

fn lookup_groups_for_upn(
    cfg: &LdapConfig,
    ldap: &mut LdapConn,
    upn: &str,
) -> Result<Vec<String>, String> {
    let base = cfg.base_dn.trim();
    if base.is_empty() {
        return Ok(Vec::new());
    }
    let filter = format!("(userPrincipalName={})", escape_ldap_filter_value(upn));
    let attrs = vec![cfg.member_attribute.as_str()];
    let (rs, _) = ldap
        .search(base, Scope::Subtree, &filter, attrs)
        .map_err(|e| format!("LDAP UPN lookup failed: {e}"))?
        .success()
        .map_err(|e| format!("LDAP UPN lookup failed: {e}"))?;
    Ok(rs
        .into_iter()
        .map(SearchEntry::construct)
        .flat_map(|e| {
            e.attrs
                .get(&cfg.member_attribute)
                .cloned()
                .unwrap_or_default()
        })
        .collect())
}

fn fetch_groups_for_dn(
    cfg: &LdapConfig,
    ldap: &mut LdapConn,
    dn: &str,
) -> Result<Vec<String>, String> {
    let (rs, _) = ldap
        .search(
            dn,
            Scope::Base,
            "(objectClass=*)",
            vec![cfg.member_attribute.as_str()],
        )
        .map_err(|e| format!("LDAP group lookup failed: {e}"))?
        .success()
        .map_err(|e| format!("LDAP group lookup failed: {e}"))?;
    Ok(rs
        .into_iter()
        .map(SearchEntry::construct)
        .next()
        .and_then(|e| e.attrs.get(&cfg.member_attribute).cloned())
        .unwrap_or_default())
}
