// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// FinOps × Security — fleet exposure cost rollup (Phase 22).

use machina_core::{
    cloud_sg_monthly_cost, exposure_chargeback_tag, fleet_exposure_monthly, gather_cloud_inventory,
    gather_firewall_inventory, gather_metal_inventory, gpu_profile_exposure_cost,
    idle_open_port_cost, port_monthly_cost, profile_exposure_multiplier, public_port_finops_alert,
    storage_profile_exposure_cost, ExposureRisk, OpenPort,
};
use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use crate::config::ControllerConfig;

use super::inventory::{overview, target_detail};
use super::metal;

#[derive(Debug, Clone, Serialize)]
pub struct ExposureTargetCost {
    pub target_id: String,
    pub kind: String,
    pub name: String,
    pub team: String,
    pub chargeback_tag: String,
    pub open_ports: usize,
    pub critical_ports: usize,
    pub idle_port_waste_usd: f64,
    pub exposure_monthly_usd: f64,
    pub profile_multiplier: f64,
    pub alert: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VmIdlePortRow {
    pub vm_id: String,
    pub vm_name: String,
    pub team: String,
    pub idle_ports: usize,
    pub waste_usd: f64,
    pub rank: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CloudSgAttribution {
    pub provider: String,
    pub rule_count: usize,
    pub public_rules: usize,
    pub estimated_monthly_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChargebackLineItem {
    pub line_id: String,
    pub team: String,
    pub category: String,
    pub label: String,
    pub monthly_usd: f64,
    pub chargeback_tag: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExposureTrendPoint {
    pub month: String,
    pub exposure_usd: f64,
    pub idle_waste_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExposureFinOpsReport {
    pub fleet_exposure_monthly_usd: f64,
    pub idle_port_waste_usd: f64,
    pub cloud_sg_monthly_usd: f64,
    pub gpu_exposure_usd: f64,
    pub storage_exposure_usd: f64,
    pub mission_stack_network_usd: f64,
    pub public_port_alerts: Vec<String>,
    pub targets: Vec<ExposureTargetCost>,
    pub vm_idle_ranking: Vec<VmIdlePortRow>,
    pub cloud_attribution: CloudSgAttribution,
    pub chargeback_lines: Vec<ChargebackLineItem>,
    pub monthly_trend: Vec<ExposureTrendPoint>,
    pub summary: String,
}

pub async fn exposure_rollup(
    pool: &DbPool,
    cfg: &ControllerConfig,
) -> anyhow::Result<ExposureFinOpsReport> {
    let ov = overview(pool, cfg).await?;
    let mut targets = Vec::new();
    let mut fleet_exposure = 0.0_f64;
    let mut idle_waste = 0.0_f64;
    let mut gpu_exposure = 0.0_f64;
    let mut storage_exposure = 0.0_f64;
    let mut public_port_alerts = Vec::new();

    for t in &ov.targets {
        let detail = target_detail(pool, cfg, &t.id).await.ok();
        let ports = detail
            .as_ref()
            .map(|d| d.inventory.open_ports.clone())
            .unwrap_or_default();
        let team = resolve_team(pool, &t.id, &t.kind).await;
        let mult = profile_exposure_multiplier(t.profile.as_deref(), &t.hostname);
        let idle = idle_open_port_cost(&ports);
        let exposure = fleet_exposure_monthly(&ports, mult);
        let critical_ports = ports
            .iter()
            .filter(|p| p.risk == ExposureRisk::Critical)
            .count();
        let chargeback = exposure_chargeback_tag(&team, exposure);
        if let Some(alert) = public_port_finops_alert(&ports) {
            public_port_alerts.push(format!("{}: {alert}", t.name));
        }
        if mult >= machina_core::GPU_EXPOSURE_MULTIPLIER - 0.01 {
            gpu_exposure += gpu_profile_exposure_cost(exposure / mult);
        } else if mult >= machina_core::STORAGE_EXPOSURE_MULTIPLIER - 0.01 {
            storage_exposure += storage_profile_exposure_cost(exposure / mult);
        }
        fleet_exposure += exposure;
        idle_waste += idle;
        targets.push(ExposureTargetCost {
            target_id: t.id.clone(),
            kind: t.kind.clone(),
            name: t.name.clone(),
            team,
            chargeback_tag: chargeback,
            open_ports: ports.len(),
            critical_ports,
            idle_port_waste_usd: idle,
            exposure_monthly_usd: exposure,
            profile_multiplier: mult,
            alert: public_port_finops_alert(&ports),
        });
    }

    targets.sort_by(|a, b| {
        b.exposure_monthly_usd
            .partial_cmp(&a.exposure_monthly_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let cloud_inv = gather_cloud_inventory();
    let public_rules = cloud_inv
        .security_groups
        .iter()
        .filter(|r| r.source == "0.0.0.0/0" || r.source == "::/0")
        .count();
    let cloud_sg_monthly = cloud_sg_monthly_cost(cloud_inv.security_groups.len(), public_rules);
    let cloud_attribution = CloudSgAttribution {
        provider: cloud_inv.provider.as_str().into(),
        rule_count: cloud_inv.security_groups.len(),
        public_rules,
        estimated_monthly_usd: cloud_sg_monthly,
    };

    let vm_idle_ranking = vm_idle_port_ranking(pool).await?;
    let chargeback_lines = build_chargeback_lines(&targets, cloud_sg_monthly);
    let monthly_trend = synthetic_monthly_trend(fleet_exposure, idle_waste);
    let mission_stack_network_usd = machina_core::mission_stack_network_cost(2);

    let summary = format!(
        "Fleet exposure est ${:.0}/mo · ${:.0} idle port waste · ${:.0} cloud SG · {} alert(s)",
        fleet_exposure,
        idle_waste,
        cloud_sg_monthly,
        public_port_alerts.len()
    );

    Ok(ExposureFinOpsReport {
        fleet_exposure_monthly_usd: fleet_exposure,
        idle_port_waste_usd: idle_waste,
        cloud_sg_monthly_usd: cloud_sg_monthly,
        gpu_exposure_usd: gpu_exposure,
        storage_exposure_usd: storage_exposure,
        mission_stack_network_usd,
        public_port_alerts,
        targets,
        vm_idle_ranking,
        cloud_attribution,
        chargeback_lines,
        monthly_trend,
        summary,
    })
}

async fn resolve_team(pool: &DbPool, target_id: &str, kind: &str) -> String {
    if kind == "bare_metal" {
        if let Ok(uid) = Uuid::parse_str(target_id) {
            if let Ok(Some(h)) = crate::db::query_scalar::<_, String>(
                "SELECT hostname FROM baremetal_servers WHERE id = ?",
            )
            .bind(uid)
            .fetch_optional(pool)
            .await
            {
                return format!("metal:{h}");
            }
        }
        return "metal:unassigned".into();
    }
    if let Ok(uid) = Uuid::parse_str(target_id) {
        if let Ok(Some(Some(t))) = crate::db::query_scalar::<_, Option<String>>(
            "SELECT (SELECT value FROM json_each(COALESCE(tags,'[]')) WHERE value LIKE 'team:%' LIMIT 1)
             FROM vms WHERE host_id = ? LIMIT 1",
        )
        .bind(uid)
        .fetch_optional(pool)
        .await
        {
            return t.strip_prefix("team:").unwrap_or(&t).to_string();
        }
    }
    "platform".into()
}

pub async fn vm_idle_port_ranking(pool: &DbPool) -> anyhow::Result<Vec<VmIdlePortRow>> {
    let rows: Vec<(Uuid, String, Option<String>)> = crate::db::query_as(
        "SELECT id, name,
                (SELECT value FROM json_each(COALESCE(tags,'[]')) WHERE value LIKE 'team:%' LIMIT 1) AS team_tag
         FROM vms ORDER BY name",
    )
    .fetch_all(pool)
    .await?;

    let mut ranked = Vec::new();
    for (vm_id, vm_name, team_tag) in rows {
        let team = team_tag
            .as_deref()
            .and_then(|t| t.strip_prefix("team:"))
            .unwrap_or("unassigned")
            .to_string();
        let host: Option<(String,)> = crate::db::query_as(
            "SELECT h.hostname FROM hosts h JOIN vms v ON v.host_id = h.id WHERE v.id = ?",
        )
        .bind(vm_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
        let ports = host
            .and_then(|(h,)| gather_firewall_inventory(&h).ok())
            .map(|inv| inv.open_ports)
            .unwrap_or_default();
        let idle_ports: Vec<&OpenPort> = ports
            .iter()
            .filter(|p| {
                p.risk == ExposureRisk::Safe && machina_core::is_public_bind(&p.bind_address)
            })
            .collect();
        let waste: f64 = idle_ports.iter().map(|p| port_monthly_cost(p, true)).sum();
        if waste > 0.0 || !idle_ports.is_empty() {
            ranked.push(VmIdlePortRow {
                vm_id: vm_id.to_string(),
                vm_name,
                team,
                idle_ports: idle_ports.len(),
                waste_usd: waste,
                rank: 0,
            });
        }
    }
    ranked.sort_by(|a, b| {
        b.waste_usd
            .partial_cmp(&a.waste_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (i, row) in ranked.iter_mut().enumerate() {
        row.rank = (i + 1) as u32;
    }
    ranked.truncate(10);
    Ok(ranked)
}

fn build_chargeback_lines(
    targets: &[ExposureTargetCost],
    cloud_sg: f64,
) -> Vec<ChargebackLineItem> {
    let mut lines = Vec::new();
    for t in targets.iter().take(8) {
        lines.push(ChargebackLineItem {
            line_id: format!("fw-{}", t.target_id),
            team: t.team.clone(),
            category: "firewall_exposure".into(),
            label: format!("{} ({})", t.name, t.kind),
            monthly_usd: t.exposure_monthly_usd,
            chargeback_tag: t.chargeback_tag.clone(),
        });
    }
    if cloud_sg > 0.0 {
        lines.push(ChargebackLineItem {
            line_id: "cloud-sg".into(),
            team: "cloud".into(),
            category: "cloud_sg".into(),
            label: "Cloud security group rules".into(),
            monthly_usd: cloud_sg,
            chargeback_tag: exposure_chargeback_tag("cloud", cloud_sg),
        });
    }
    lines
}

fn synthetic_monthly_trend(current_exposure: f64, current_idle: f64) -> Vec<ExposureTrendPoint> {
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun"];
    months
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let factor = 0.82 + (i as f64 * 0.036);
            ExposureTrendPoint {
                month: (*m).into(),
                exposure_usd: current_exposure * factor,
                idle_waste_usd: current_idle * factor,
            }
        })
        .collect()
}

pub async fn export_csv(pool: &DbPool, cfg: &ControllerConfig) -> anyhow::Result<String> {
    let report = exposure_rollup(pool, cfg).await?;
    let mut csv = String::from(
        "Machina Zeus Firewall Exposure Cost Export\n\
Metric,USD\n\
Fleet exposure monthly,",
    );
    csv.push_str(&format!("{:.2}\n", report.fleet_exposure_monthly_usd));
    csv.push_str(&format!(
        "Idle port waste,{:.2}\n",
        report.idle_port_waste_usd
    ));
    csv.push_str(&format!("Cloud SG,{:.2}\n", report.cloud_sg_monthly_usd));
    csv.push_str(&format!("GPU exposure,{:.2}\n", report.gpu_exposure_usd));
    csv.push_str(&format!(
        "Storage exposure,{:.2}\n",
        report.storage_exposure_usd
    ));
    csv.push_str(
        "\nTarget,Kind,Team,Open Ports,Critical,Exposure USD,Idle Waste USD,Chargeback Tag\n",
    );
    for t in &report.targets {
        csv.push_str(&format!(
            "{},{},{},{},{},{:.2},{:.2},{}\n",
            csv_escape(&t.name),
            csv_escape(&t.kind),
            csv_escape(&t.team),
            t.open_ports,
            t.critical_ports,
            t.exposure_monthly_usd,
            t.idle_port_waste_usd,
            csv_escape(&t.chargeback_tag),
        ));
    }
    csv.push_str("\nVM Rank,VM,Team,Idle Ports,Waste USD\n");
    for v in &report.vm_idle_ranking {
        csv.push_str(&format!(
            "{},{},{},{},{:.2}\n",
            v.rank,
            csv_escape(&v.vm_name),
            csv_escape(&v.team),
            v.idle_ports,
            v.waste_usd,
        ));
    }
    Ok(csv)
}

fn csv_escape(s: &str) -> String {
    // Neutralize CSV/Excel formula injection: values here can originate from
    // user-controlled data (VM names, team tags) and this file is served as
    // an attachment for humans to open in Excel/Sheets. A cell starting with
    // =, +, -, or @ is interpreted as a formula by those tools (e.g.
    // `=cmd|'/c calc'!A1`), so prefix a neutralizing apostrophe before
    // applying the existing quote/comma/newline escaping.
    let needs_formula_guard = s
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '=' | '+' | '-' | '@' | '\t' | '\r'));
    let s = if needs_formula_guard {
        std::borrow::Cow::Owned(format!("'{s}"))
    } else {
        std::borrow::Cow::Borrowed(s)
    };
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.into_owned()
    }
}

/// Per-team exposure attribution rollup (AI-302).
pub async fn team_exposure_attribution(
    pool: &DbPool,
    cfg: &ControllerConfig,
) -> anyhow::Result<Vec<(String, f64, f64)>> {
    let report = exposure_rollup(pool, cfg).await?;
    let mut teams: std::collections::HashMap<String, (f64, f64)> = std::collections::HashMap::new();
    for t in &report.targets {
        let entry = teams.entry(t.team.clone()).or_insert((0.0, 0.0));
        entry.0 += t.exposure_monthly_usd;
        entry.1 += t.idle_port_waste_usd;
    }
    let mut rows: Vec<(String, f64, f64)> = teams
        .into_iter()
        .map(|(team, (exp, idle))| (team, exp, idle))
        .collect();
    rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    Ok(rows)
}

#[allow(dead_code)]
pub fn estimate_metal_exposure(row: &metal::BaremetalFirewallRow) -> f64 {
    let inv = gather_metal_inventory(&metal::row_to_input(row));
    let mult = profile_exposure_multiplier(Some(&row.firewall_profile), &row.hostname);
    fleet_exposure_monthly(&inv.open_ports, mult)
}
