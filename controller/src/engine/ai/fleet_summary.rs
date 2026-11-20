// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::time::Duration;

use serde::{Deserialize, Serialize};
use crate::db::DbPool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetClusterSlice {
    pub label: String,
    pub reachable: bool,
    pub vm_count: i64,
    pub estimated_monthly_usd: f64,
    pub memory_headroom_mib: i64,
    pub security_risk_level: String,
}

#[derive(Debug, Serialize)]
pub struct FleetZyraSummary {
    pub clusters: Vec<FleetClusterSlice>,
    pub aggregate_monthly_usd: f64,
    pub aggregate_vm_count: i64,
    pub peer_count: usize,
    pub reachable_peers: usize,
}

pub async fn summarize(pool: &DbPool) -> anyhow::Result<FleetZyraSummary> {
    let mut clusters = vec![local_slice(pool).await?];
    let peer_urls = super::settings::get_fleet_peer_urls(pool).await?;
    let mut reachable_peers = 0usize;

    for url in &peer_urls {
        let slice = fetch_peer_slice(url).await;
        if slice.reachable {
            reachable_peers += 1;
        }
        clusters.push(slice);
    }

    let aggregate_monthly_usd = clusters.iter().map(|c| c.estimated_monthly_usd).sum();
    let aggregate_vm_count = clusters.iter().map(|c| c.vm_count).sum();

    Ok(FleetZyraSummary {
        peer_count: peer_urls.len(),
        reachable_peers,
        aggregate_monthly_usd,
        aggregate_vm_count,
        clusters,
    })
}

async fn local_slice(pool: &DbPool) -> anyhow::Result<FleetClusterSlice> {
    let name: String = crate::db::query_scalar("SELECT name FROM clusters ORDER BY created_at LIMIT 1")
        .fetch_one(pool)
        .await?;
    let cost = super::cost::analyze(pool).await?;
    let cap = super::capacity::plan(pool).await?;
    let sec = super::security::scan(pool).await?;
    Ok(FleetClusterSlice {
        label: name,
        reachable: true,
        vm_count: cost.vm_count,
        estimated_monthly_usd: cost.estimated_monthly_usd,
        memory_headroom_mib: cap.memory_headroom_mib,
        security_risk_level: sec.risk_level,
    })
}

/// Reject cloud-metadata endpoints for an admin-configured fleet peer URL.
///
/// Fleet peers are other machina clusters, which legitimately live on
/// RFC1918/loopback addresses (same-LAN or same-host multi-cluster setups),
/// so this deliberately does not block private IP ranges — only the
/// well-known cloud instance-metadata addresses, which have zero legitimate
/// use as a fleet peer and are a classic SSRF target for credential theft.
fn is_blocked_metadata_host(base: &str) -> bool {
    let host = base
        .parse::<reqwest::Url>()
        .map(|u| u.host_str().unwrap_or("").to_string())
        .unwrap_or_default();
    let host = normalize_host_for_metadata_check(&host);
    const BLOCKED_METADATA_HOSTS: &[&str] = &[
        "169.254.169.254",
        "metadata.google.internal",
        "metadata.google",
        "metadata",
        "fd00:ec2::254",
        "100.100.100.200",
    ];
    BLOCKED_METADATA_HOSTS.contains(&host.as_str())
}

/// Canonicalize a `Url::host_str()` value before comparing it against the
/// blocklist above.
///
/// `host_str()` returns IPv6 hosts bracketed (e.g. `"[fd00:ec2::254]"`) and
/// leaves an IPv4-mapped IPv6 literal (e.g. `"::ffff:169.254.169.254"`) in its
/// compressed hextet form (`"::ffff:a9fe:a9fe"`) rather than the dotted-quad
/// the blocklist is written in — a peer URL written either way previously
/// sailed straight past the literal string comparison and reached the
/// metadata IP anyway, defeating the SSRF guard entirely.
fn normalize_host_for_metadata_check(host: &str) -> String {
    let mut host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if let Some(stripped) = host.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        host = stripped.to_string();
    }
    if let Ok(std::net::IpAddr::V6(v6)) = host.parse::<std::net::IpAddr>() {
        if let Some(v4) = v6.to_ipv4_mapped() {
            host = v4.to_string();
        }
    }
    host
}

async fn fetch_peer_slice(base: &str) -> FleetClusterSlice {
    let base = base.trim_end_matches('/');
    let label = base.to_string();
    if is_blocked_metadata_host(base) {
        tracing::warn!(peer = %label, "refusing to contact fleet peer: cloud metadata endpoint");
        return unreachable_peer(&label);
    }
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            return FleetClusterSlice {
                label,
                reachable: false,
                vm_count: 0,
                estimated_monthly_usd: 0.0,
                memory_headroom_mib: 0,
                security_risk_level: "unknown".into(),
            };
        }
    };

    let health = client.get(format!("{base}/api/v1/health")).send().await;
    if !health
        .as_ref()
        .map(|r| r.status().is_success())
        .unwrap_or(false)
    {
        return FleetClusterSlice {
            label,
            reachable: false,
            vm_count: 0,
            estimated_monthly_usd: 0.0,
            memory_headroom_mib: 0,
            security_risk_level: "unknown".into(),
        };
    }

    let mut req = client.get(format!("{base}/api/v1/ai/fleet/local"));
    if let Ok(key) = std::env::var("ZEUS_FLEET_API_KEY") {
        if !key.is_empty() {
            req = req.bearer_auth(key);
        }
    }

    match req.send().await {
        Ok(resp) if resp.status().is_success() => resp
            .json::<FleetClusterSlice>()
            .await
            .unwrap_or_else(|_| unreachable_peer(&label)),
        _ => unreachable_peer(&label),
    }
}

fn unreachable_peer(label: &str) -> FleetClusterSlice {
    FleetClusterSlice {
        label: label.to_string(),
        reachable: false,
        vm_count: 0,
        estimated_monthly_usd: 0.0,
        memory_headroom_mib: 0,
        security_risk_level: "unknown".into(),
    }
}

pub async fn local_export(pool: &DbPool) -> anyhow::Result<FleetClusterSlice> {
    local_slice(pool).await
}
