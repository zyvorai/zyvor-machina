// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use machina_core::{
    compile_metal_plan, gather_metal_inventory, scan_ipmi_exposure, FirewallPlanRequest,
    FirewallPlanResult, MetalServerInput,
};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct BaremetalFirewallRow {
    pub id: Uuid,
    pub hostname: String,
    pub bmc_address: String,
    pub bmc_type: String,
    pub state: String,
    pub firewall_profile: String,
    pub firewall_enabled: bool,
    pub bmc_vlan: String,
    pub pxe_vlan: String,
    pub posture_json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BaremetalFirewallOverview {
    pub servers: Vec<BaremetalTargetSummary>,
    pub critical_count: usize,
    pub warning_count: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BaremetalTargetSummary {
    pub id: String,
    pub hostname: String,
    pub bmc_address: String,
    pub firewall_profile: String,
    pub risk: String,
    pub score: u32,
    pub open_ports: usize,
}

pub fn row_to_input(row: &BaremetalFirewallRow) -> MetalServerInput {
    MetalServerInput {
        hostname: row.hostname.clone(),
        bmc_address: row.bmc_address.clone(),
        bmc_type: row.bmc_type.clone(),
        firewall_profile: row.firewall_profile.clone(),
        firewall_enabled: row.firewall_enabled,
        bmc_vlan: row.bmc_vlan.clone(),
        pxe_vlan: row.pxe_vlan.clone(),
    }
}

pub async fn load_row(pool: &DbPool, id: Uuid) -> anyhow::Result<BaremetalFirewallRow> {
    crate::db::query_as(
        "SELECT id, hostname, bmc_address, bmc_type, state, firewall_profile, firewall_enabled,
                bmc_vlan, pxe_vlan, posture_json
         FROM baremetal_servers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("bare metal server not found"))
}

pub async fn load_all(pool: &DbPool) -> anyhow::Result<Vec<BaremetalFirewallRow>> {
    Ok(crate::db::query_as(
        "SELECT id, hostname, bmc_address, bmc_type, state, firewall_profile, firewall_enabled,
                bmc_vlan, pxe_vlan, posture_json
         FROM baremetal_servers ORDER BY hostname",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn metal_overview(pool: &DbPool) -> anyhow::Result<BaremetalFirewallOverview> {
    let rows = load_all(pool).await?;
    let mut critical = 0usize;
    let mut warning = 0usize;
    let servers: Vec<BaremetalTargetSummary> = rows
        .iter()
        .map(|row| {
            let inv = gather_metal_inventory(&row_to_input(row));
            let risk = super::inventory::risk_label(&inv).to_string();
            if risk == "critical" {
                critical += 1;
            } else if risk == "warning" {
                warning += 1;
            }
            BaremetalTargetSummary {
                id: row.id.to_string(),
                hostname: row.hostname.clone(),
                bmc_address: row.bmc_address.clone(),
                firewall_profile: row.firewall_profile.clone(),
                risk,
                score: inv.score.score,
                open_ports: inv.open_ports.len(),
            }
        })
        .collect();
    Ok(BaremetalFirewallOverview {
        summary: format!(
            "{} bare-metal servers · {} critical · {} warnings",
            servers.len(),
            critical,
            warning
        ),
        servers,
        critical_count: critical,
        warning_count: warning,
    })
}

pub async fn scan_exposure(
    pool: &DbPool,
    id: Uuid,
    actor: &str,
) -> anyhow::Result<serde_json::Value> {
    let row = load_row(pool, id).await?;
    let scan = scan_ipmi_exposure(&row.bmc_address, &row.bmc_type);
    let posture = serde_json::to_value(&scan)?;
    crate::db::query(
        "UPDATE baremetal_servers SET posture_json = ?, last_exposure_scan_at = datetime('now') WHERE id = ?",
    )
    .bind(&posture)
    .bind(id)
    .execute(pool)
    .await?;

    let inv = gather_metal_inventory(&row_to_input(&row));
    let _ = super::drift::save_snapshot(pool, "bare_metal", id, &inv).await;

    let _ = crate::db::query(
        "INSERT INTO firewall_timeline (id, target_kind, target_id, kind, summary, detail_json, actor) VALUES (?, 'bare_metal', ?, 'metal_scan', ?, ?, ?)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(id)
    .bind(format!("Exposure scan — risk {}", scan.risk))
    .bind(&posture)
    .bind(actor)
    .execute(pool)
    .await;

    Ok(posture)
}

pub async fn plan_metal(
    pool: &DbPool,
    id: Uuid,
    req: FirewallPlanRequest,
) -> anyhow::Result<FirewallPlanResult> {
    let row = load_row(pool, id).await?;
    compile_metal_plan(&row_to_input(&row), &req).map_err(|e| anyhow::anyhow!(e.to_string()))
}

pub async fn apply_metal(
    pool: &DbPool,
    id: Uuid,
    req: FirewallPlanRequest,
    actor: &str,
) -> anyhow::Result<FirewallPlanResult> {
    let row = load_row(pool, id).await?;
    let mut apply_req = req;
    apply_req.dry_run = false;

    let inv = gather_metal_inventory(&row_to_input(&row));
    let _ =
        super::checkpoint::save_checkpoint(pool, "bare_metal", id, "pre-apply", &inv, Some(actor))
            .await;

    let result = compile_metal_plan(&row_to_input(&row), &apply_req)?;

    let profile = apply_req
        .profile
        .clone()
        .unwrap_or_else(|| row.firewall_profile.clone());
    let enabled = apply_req.enable.unwrap_or(true);

    crate::db::query(
        "UPDATE baremetal_servers SET firewall_profile = ?, firewall_enabled = ? WHERE id = ?",
    )
    .bind(&profile)
    .bind(enabled)
    .bind(id)
    .execute(pool)
    .await?;

    let after = gather_metal_inventory(&MetalServerInput {
        firewall_profile: profile.clone(),
        firewall_enabled: enabled,
        ..row_to_input(&row)
    });
    let _ = super::drift::save_snapshot(pool, "bare_metal", id, &after).await;

    let kind = if profile == "MetalLockdown" {
        "metal_lockdown"
    } else {
        "metal_profile_applied"
    };
    let _ = crate::db::query(
        "INSERT INTO firewall_timeline (id, target_kind, target_id, kind, summary, detail_json, actor) VALUES (?, 'bare_metal', ?, ?, ?, ?, ?)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(id)
    .bind(kind)
    .bind(format!("Applied metal profile {profile} (policy-only)"))
    .bind(serde_json::json!({ "operations": result.operations, "tag": "metal" }))
    .bind(actor)
    .execute(pool)
    .await;

    Ok(result)
}

pub async fn upsert_gitops_policy(
    pool: &DbPool,
    hostname: &str,
    profile: &str,
) -> anyhow::Result<()> {
    let name = format!("metal-{hostname}");
    let spec_yaml = format!(
        "apiVersion: zeus.machina/v1\nkind: MachineFirewallPolicy\nmetadata:\n  name: {name}\nspec:\n  targetKind: bare_metal\n  profile: {profile}\n  scope: bmc+pxe\n"
    );
    crate::db::query(
        "INSERT INTO firewall_policies (id, name, spec_yaml) VALUES (?, ?, ?)
         ON CONFLICT (name) DO UPDATE SET spec_yaml = EXCLUDED.spec_yaml, updated_at = datetime('now')",
    )
    .bind(Uuid::new_v4())
    .bind(&name)
    .bind(&spec_yaml)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct MetalTemporaryPreset {
    pub preset: String,
    pub reason: String,
    #[serde(default)]
    pub owner: Option<String>,
}

pub async fn create_metal_temporary_preset(
    pool: &DbPool,
    id: Uuid,
    preset: &str,
    reason: &str,
    owner: Option<&str>,
) -> anyhow::Result<super::TemporaryRule> {
    // Match explicitly and reject unknown presets. The previous `"bmc" | _ =>`
    // catch-all silently treated *any* unrecognized preset string (a typo, a
    // future preset name not yet handled, ...) as "bmc", which opens the IPMI
    // (623) and Redfish/HTTPS (443) management ports for 4h — granting BMC
    // access nobody asked for instead of surfacing the bad input.
    let (port, port2, proto, hours) = match preset {
        "pxe" => machina_core::metal_preset_temporary_pxe(),
        "bmc" => machina_core::metal_preset_temporary_bmc(),
        other => anyhow::bail!("unknown temporary preset '{other}' (expected 'pxe' or 'bmc')"),
    };

    let rule = super::temporary::create_temporary_rule(
        pool,
        super::TemporaryRuleRequest {
            target_kind: "bare_metal".into(),
            target_id: id,
            source_cidr: "10.0.0.0/8".into(),
            dest_port: port,
            protocol: proto.clone(),
            reason: reason.into(),
            duration_hours: hours,
            owner: owner.map(String::from),
        },
    )
    .await?;

    // Both presets are two-port pairs (bmc: IPMI 623 + Redfish/HTTPS 443; pxe:
    // DHCP 67 + TFTP 69) but only `port` was ever opened — `port2` was bound
    // to `_port2` and discarded. That silently granted half of what the
    // preset promised, which for "bmc" means Redfish/HTTPS access never
    // actually opens even though the preset claims to allow it.
    if port2 != port {
        let _ = super::temporary::create_temporary_rule(
            pool,
            super::TemporaryRuleRequest {
                target_kind: "bare_metal".into(),
                target_id: id,
                source_cidr: "10.0.0.0/8".into(),
                dest_port: port2,
                protocol: proto,
                reason: reason.into(),
                duration_hours: hours,
                owner: owner.map(String::from),
            },
        )
        .await;
    }

    Ok(rule)
}
