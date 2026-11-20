// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Fleet Linux health rollup (Phase 36).

use futures_util::future::join_all;
use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use crate::config::ControllerConfig;
use crate::engine::host_os;

#[derive(Debug, Clone, Serialize)]
pub struct FleetLinuxHostItem {
    pub host_id: String,
    pub hostname: String,
    pub io_pressure_pct: f64,
    pub thermal_max_c: f64,
    pub smart_failures: usize,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetLinuxHealthOverview {
    pub hosts_scanned: usize,
    pub pressure_hosts: usize,
    pub thermal_alerts: usize,
    pub smart_alerts: usize,
    pub hosts: Vec<FleetLinuxHostItem>,
    pub summary: String,
}

pub async fn overview(
    pool: &DbPool,
    cfg: &ControllerConfig,
) -> anyhow::Result<FleetLinuxHealthOverview> {
    let rows: Vec<(Uuid, String)> = crate::db::query_as(
        "SELECT id, hostname FROM hosts WHERE state = 'online' ORDER BY hostname LIMIT 20",
    )
    .fetch_all(pool)
    .await?;

    // Each per-host RPC can take up to the agent's read timeout (see
    // agent_client::READ_RPC_TIMEOUT) if that host's agent is wedged. Fetching
    // sequentially meant one slow/wedged host stalled every other host behind
    // it in line; this endpoint is polled by several always-on shell widgets; a
    // single wedged agent turned into a 30-60s stall for the whole platform
    // shell. Fetching concurrently caps worst case at one RPC timeout instead
    // of `hosts.len()` of them.
    let observations = join_all(rows.into_iter().map(|(id, hostname)| async move {
        let obs = host_os::linux_observability(pool, cfg, id).await.ok();
        (id, hostname, obs)
    }))
    .await;

    let mut hosts = Vec::new();
    let mut pressure_hosts = 0usize;
    let mut thermal_alerts = 0usize;
    let mut smart_alerts = 0usize;

    for (id, hostname, obs) in observations {
        let Some(obs) = obs else {
            continue;
        };
        // `avg10` from `/proc/pressure/io` is already a 0-100 percentage —
        // multiplying by 100 here inflated it 100x (e.g. 17.67% read as
        // "1767%"), which falsely tripped the `io > 50.0` pressure threshold
        // below on almost any nonzero real pressure.
        let io = obs
            .get("pressure")
            .and_then(|p| p.get("io"))
            .and_then(|i| i.get("some"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let thermal_max = obs
            .get("thermal")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.get("temp_celsius").and_then(|v| v.as_f64()))
                    .fold(0.0_f64, f64::max)
            })
            .unwrap_or(0.0);
        let smart_failures = obs
            .get("smart")
            .and_then(|s| s.as_array())
            .map(|arr| {
                arr.iter()
                    .filter(|d| d.get("passed") == Some(&serde_json::json!(false)))
                    .count()
            })
            .unwrap_or(0);
        let status = if io > 50.0 || smart_failures > 0 {
            pressure_hosts += 1;
            "pressure"
        } else if thermal_max > 80.0 {
            thermal_alerts += 1;
            "thermal"
        } else {
            "ok"
        };
        if smart_failures > 0 {
            smart_alerts += 1;
        }
        hosts.push(FleetLinuxHostItem {
            host_id: id.to_string(),
            hostname,
            io_pressure_pct: io,
            thermal_max_c: thermal_max,
            smart_failures,
            status: status.into(),
        });
    }

    let hosts_scanned = hosts.len();
    Ok(FleetLinuxHealthOverview {
        hosts_scanned,
        pressure_hosts,
        thermal_alerts,
        smart_alerts,
        hosts,
        summary: format!(
            "{hosts_scanned} host(s) scanned · {pressure_hosts} under pressure · {thermal_alerts} thermal · {smart_alerts} SMART"
        ),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetDiagnoseReport {
    pub query: String,
    pub summary: String,
    pub zyra_status: String,
    pub linux_summary: String,
    pub hypotheses: Vec<crate::engine::ai::knowledge_diagnose::DiagnoseHypothesis>,
}

pub async fn diagnose(
    pool: &DbPool,
    cfg: &ControllerConfig,
    query: &str,
) -> anyhow::Result<FleetDiagnoseReport> {
    let zeus = crate::engine::ai::zyra_summary::summarize(pool).await?;
    let linux = overview(pool, cfg).await?;
    let mut diag = crate::engine::ai::knowledge_diagnose::diagnose(pool, query).await?;
    if linux.pressure_hosts > 0 {
        diag.hypotheses
            .push(crate::engine::ai::knowledge_diagnose::DiagnoseHypothesis {
                title: "Hypervisor IO/memory pressure".into(),
                confidence: 0.8,
                evidence: linux.summary.clone(),
                action: "Review Activity Monitor and storage tiers; consider rebalance.".into(),
            });
    }
    Ok(FleetDiagnoseReport {
        query: query.into(),
        summary: format!("Fleet {} · {}", zeus.status, linux.summary),
        zyra_status: zeus.status,
        linux_summary: linux.summary,
        hypotheses: diag.hypotheses,
    })
}
