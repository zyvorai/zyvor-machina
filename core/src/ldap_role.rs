// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Map LDAP group membership strings to Machina RBAC roles.

use crate::config::LdapConfig;
use crate::libvirt::automation::Role;

pub fn role_from_ldap_groups(cfg: &LdapConfig, groups: &[String]) -> Role {
    let hay: Vec<String> = groups.iter().map(|g| g.to_ascii_lowercase()).collect();
    let matches = |subs: &[String]| {
        subs.iter().any(|sub| {
            let s = sub.trim().to_ascii_lowercase();
            !s.is_empty() && hay.iter().any(|g| g.contains(&s))
        })
    };
    if matches(&cfg.admin_group_substrings) {
        return Role::Admin;
    }
    if matches(&cfg.operator_group_substrings) {
        return Role::Operator;
    }
    if matches(&cfg.readonly_group_substrings) {
        return Role::ReadOnly;
    }
    Role::ReadOnly
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_group_wins_over_operator() {
        let cfg = LdapConfig {
            admin_group_substrings: vec!["machina-admins".into()],
            operator_group_substrings: vec!["machina-ops".into()],
            ..Default::default()
        };
        let role = role_from_ldap_groups(
            &cfg,
            &["CN=machina-admins,OU=Groups,DC=example,DC=com".into()],
        );
        assert!(matches!(role, Role::Admin));
    }
}
