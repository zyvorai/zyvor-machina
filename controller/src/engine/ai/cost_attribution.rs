// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct TeamCostRow {
    pub team: String,
    pub vm_count: i64,
    pub vcpus: i64,
    pub memory_gib: f64,
    pub estimated_monthly_usd: f64,
    pub exposure_monthly_usd: f64,
    pub share_pct: f32,
}

#[derive(Debug, Serialize)]
pub struct CostAttributionReport {
    pub total_monthly_usd: f64,
    pub teams: Vec<TeamCostRow>,
    pub unattributed_monthly_usd: f64,
    pub summary: String,
}

pub async fn attribute(pool: &DbPool) -> anyhow::Result<CostAttributionReport> {
    let rates: (f64, f64) = crate::db::query_as(
        "SELECT finops_vcpu_hour_usd, finops_gib_hour_usd FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_one(pool)
    .await?;

    let rows: Vec<(Option<String>, i64, i64, i64)> = crate::db::query_as(
        "SELECT COALESCE(NULLIF(TRIM(project), ''), NULL) AS project,
                COUNT(*),
                COALESCE(SUM(vcpus), 0),
                COALESCE(SUM(memory_mib), 0)
         FROM vms
         GROUP BY COALESCE(NULLIF(TRIM(project), ''), NULL)",
    )
    .fetch_all(pool)
    .await?;

    let tag_rows: Vec<(String, i64, i64, i64)> = crate::db::query_as(
        "SELECT COALESCE(
            (SELECT value FROM json_each(COALESCE(tags,'[]')) WHERE value LIKE 'team:%' LIMIT 1),
            'team:unassigned'
         ) AS team,
         COUNT(*),
         COALESCE(SUM(vcpus), 0),
         COALESCE(SUM(memory_mib), 0)
         FROM vms
         GROUP BY 1",
    )
    .fetch_all(pool)
    .await?;

    let mut teams = Vec::new();
    let mut total = 0.0_f64;

    for (project, count, vcpus, mem_mib) in rows {
        let team = project
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "default".into());
        let memory_gib = mem_mib as f64 / 1024.0;
        let monthly = (vcpus as f64 * rates.0 + memory_gib * rates.1) * 730.0;
        total += monthly;
        teams.push(TeamCostRow {
            team: format!("project:{team}"),
            vm_count: count,
            vcpus,
            memory_gib,
            estimated_monthly_usd: monthly,
            exposure_monthly_usd: 0.0,
            share_pct: 0.0,
        });
    }

    for (tag, count, vcpus, mem_mib) in tag_rows {
        let team = tag.strip_prefix("team:").unwrap_or(&tag).to_string();
        if teams.iter().any(|t| t.team == format!("tag:{team}")) {
            continue;
        }
        let memory_gib = mem_mib as f64 / 1024.0;
        let monthly = (vcpus as f64 * rates.0 + memory_gib * rates.1) * 730.0;
        teams.push(TeamCostRow {
            team: format!("tag:{team}"),
            vm_count: count,
            vcpus,
            memory_gib,
            estimated_monthly_usd: monthly,
            exposure_monthly_usd: 0.0,
            share_pct: 0.0,
        });
    }

    teams.sort_by(|a, b| {
        b.estimated_monthly_usd
            .partial_cmp(&a.estimated_monthly_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if total <= 0.0 {
        total = teams.iter().map(|t| t.estimated_monthly_usd).sum();
    }

    for row in &mut teams {
        row.share_pct = if total > 0.0 {
            (row.estimated_monthly_usd / total * 100.0) as f32
        } else {
            0.0
        };
        row.exposure_monthly_usd = 0.0;
    }

    let cfg = crate::config::ControllerConfig::default();
    if let Ok(exp_teams) =
        crate::engine::zeus_firewall::finops::team_exposure_attribution(pool, &cfg).await
    {
        for (team, exposure, _) in exp_teams {
            let key = format!("tag:{team}");
            if let Some(row) = teams
                .iter_mut()
                .find(|t| t.team.contains(&team) || t.team == key)
            {
                row.exposure_monthly_usd = exposure;
            } else {
                teams.push(TeamCostRow {
                    team: key,
                    vm_count: 0,
                    vcpus: 0,
                    memory_gib: 0.0,
                    estimated_monthly_usd: 0.0,
                    exposure_monthly_usd: exposure,
                    share_pct: 0.0,
                });
            }
        }
    }

    let unattributed = teams
        .iter()
        .filter(|t| t.team.contains("unassigned") || t.team.contains("default"))
        .map(|t| t.estimated_monthly_usd)
        .sum();

    let summary = if teams.is_empty() {
        "No VMs — add project or team: tags for FinOps attribution.".into()
    } else {
        format!(
            "{} team bucket(s) · ${:.0}/mo total · {:.0}% unattributed/default",
            teams.len(),
            total,
            if total > 0.0 {
                unattributed / total * 100.0
            } else {
                0.0
            }
        )
    };

    Ok(CostAttributionReport {
        total_monthly_usd: total,
        teams,
        unattributed_monthly_usd: unattributed,
        summary,
    })
}

pub async fn export_csv(pool: &DbPool) -> anyhow::Result<String> {
    let report = attribute(pool).await?;
    let mut csv = String::from(
        "Machina FinOps Team Attribution\nTeam,VM Count,vCPUs,Memory GiB,Est Monthly USD,Share %\n",
    );
    for row in &report.teams {
        csv.push_str(&format!(
            "{},{},{},{:.1},{:.2},{:.1}\n",
            csv_escape(&row.team),
            row.vm_count,
            row.vcpus,
            row.memory_gib,
            row.estimated_monthly_usd,
            row.share_pct
        ));
    }
    csv.push_str(&format!(
        "\nTotal USD,{:.2}\nUnattributed USD,{:.2}\n",
        report.total_monthly_usd, report.unattributed_monthly_usd
    ));
    Ok(csv)
}

fn csv_escape(s: &str) -> String {
    // Neutralize CSV/Excel formula injection: values here can originate from
    // user-controlled data (team tags) and this file is served as an
    // attachment for humans to open in Excel/Sheets. A cell starting with
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
