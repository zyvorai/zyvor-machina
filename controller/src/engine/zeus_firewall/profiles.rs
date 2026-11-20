// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use machina_core::{profile_by_name, FirewallPlanRequest, FirewallProfile};
use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Clone, Serialize)]
pub struct ProfileListItem {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub default_inbound: String,
    pub stealth_level: String,
}

pub async fn list_profiles(pool: &DbPool) -> anyhow::Result<Vec<ProfileListItem>> {
    let rows: Vec<(String, String, serde_json::Value)> =
        crate::db::query_as("SELECT name, display_name, spec_json FROM firewall_profiles ORDER BY name")
            .fetch_all(pool)
            .await
            .unwrap_or_default();

    if rows.is_empty() {
        return Ok(machina_core::builtin_profiles()
            .into_iter()
            .map(|p| profile_item(&p))
            .collect());
    }

    Ok(rows
        .into_iter()
        .map(|(name, display_name, spec)| ProfileListItem {
            name,
            display_name,
            description: spec
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .into(),
            default_inbound: spec
                .get("default_inbound")
                .and_then(|v| v.as_str())
                .unwrap_or("deny")
                .into(),
            stealth_level: spec
                .get("stealth")
                .and_then(|v| v.as_str())
                .unwrap_or("off")
                .into(),
        })
        .collect())
}

pub fn profile_item(p: &FirewallProfile) -> ProfileListItem {
    ProfileListItem {
        name: p.name.clone(),
        display_name: p.display_name.clone(),
        description: p.description.clone(),
        default_inbound: p.default_inbound.clone(),
        stealth_level: format!("{:?}", p.stealth_level).to_ascii_lowercase(),
    }
}

pub fn plan_for_profile(name: &str, dry_run: bool) -> anyhow::Result<FirewallPlanRequest> {
    profile_by_name(name).ok_or_else(|| anyhow::anyhow!("unknown profile {name}"))?;
    Ok(FirewallPlanRequest {
        profile: Some(name.into()),
        enable: Some(true),
        stealth_level: None,
        preset: None,
        dry_run,
        zone_cidrs: Default::default(),
    })
}
