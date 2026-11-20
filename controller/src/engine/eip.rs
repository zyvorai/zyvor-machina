// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Pushes the elastic IP associations of each host to its agent (`eip.sync`), on every change and every 30 s.

use std::net::Ipv4Addr;
use std::time::Duration;

use serde_json::{json, Value};
use crate::db::DbPool;
use uuid::Uuid;

use crate::state::AppState;

/// The first usable address of `cidr` (`a.b.c.d/n`) that is not in `used`. For prefixes up to /30 the network and broadcast
/// addresses are skipped. At most 65 536 candidates are scanned.
pub fn next_free(cidr: &str, used: &std::collections::HashSet<String>) -> Option<String> {
    let (addr, bits) = cidr.split_once('/')?;
    let base: u32 = u32::from(addr.parse::<Ipv4Addr>().ok()?);
    let bits: u32 = bits.parse().ok()?;
    if bits > 32 {
        return None;
    }
    let size: u64 = 1u64 << (32 - bits);
    let mask: u32 = if bits == 0 { 0 } else { u32::MAX << (32 - bits) };
    let net = base & mask;
    let (lo, hi) = if bits >= 31 { (0u64, size) } else { (1u64, size - 1) };
    (lo..hi.min(lo + 65_536))
        .map(|i| Ipv4Addr::from(net.wrapping_add(i as u32)).to_string())
        .find(|a| !used.contains(a))
}

/// `Ok(())` when `address` belongs to `cidr`.
pub fn in_cidr(cidr: &str, address: &str) -> bool {
    let Some((a, bits)) = cidr.split_once('/') else { return false };
    let (Ok(net), Ok(ip), Ok(bits)) = (a.parse::<Ipv4Addr>(), address.parse::<Ipv4Addr>(), bits.parse::<u32>()) else { return false };
    if bits > 32 {
        return false;
    }
    let mask: u32 = if bits == 0 { 0 } else { u32::MAX << (32 - bits) };
    u32::from(net) & mask == u32::from(ip) & mask
}

pub async fn entries_for_host(pool: &DbPool, host: Uuid) -> anyhow::Result<Vec<Value>> {
    let rows: Vec<(String, Option<String>, Option<String>, String)> = crate::db::query_as(
        "SELECT e.address, v.guest_ip, v.guest_ips, p.interface FROM elastic_ips e \
         JOIN eip_pools p ON p.id = e.pool_id JOIN vms v ON v.id = e.vm_id WHERE p.host_id = ? AND e.vm_id IS NOT NULL",
    )
    .bind(host)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(address, ip, ips, interface)| {
            let private = ip
                .filter(|a| !a.is_empty())
                .or_else(|| ips.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok()).and_then(|v| v.into_iter().next()))?;
            Some(json!({ "address": address, "private_ip": private.split('/').next().unwrap_or(&private), "interface": interface }))
        })
        .collect())
}

/// Push the current table to one host's agent.
pub async fn push_host(pool: &DbPool, host: Uuid) -> anyhow::Result<()> {
    let addr: Option<String> = crate::db::query_scalar("SELECT agent_grpc_addr FROM hosts WHERE id = ? AND state = 'online'").bind(host).fetch_optional(pool).await?;
    let addr = addr.ok_or_else(|| anyhow::anyhow!("the host is not online"))?;
    let entries = entries_for_host(pool, host).await?;
    let mut client = crate::agent_client::connect(&addr).await?;
    crate::agent_client::host_libvirt_invoke(&mut client, "eip.sync", &json!({ "entries": entries })).await?;
    Ok(())
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            let hosts: Vec<Uuid> = crate::db::query_scalar("SELECT DISTINCT host_id FROM eip_pools").fetch_all(&state.pool).await.unwrap_or_default();
            for h in hosts {
                if let Err(e) = push_host(&state.pool, h).await {
                    tracing::debug!(host = %h, "elastic ip sync: {e:#}");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn picks_the_first_free_usable_address() {
        let mut used = HashSet::new();
        assert_eq!(next_free("203.0.113.0/29", &used).as_deref(), Some("203.0.113.1"));
        used.insert("203.0.113.1".to_string());
        assert_eq!(next_free("203.0.113.0/29", &used).as_deref(), Some("203.0.113.2"));
    }

    #[test]
    fn network_and_broadcast_are_skipped_and_a_full_pool_is_none() {
        let all: HashSet<String> = (1..=6).map(|i| format!("203.0.113.{i}")).collect();
        assert_eq!(next_free("203.0.113.0/29", &all), None, ".0 and .7 are not usable");
        assert_eq!(next_free("203.0.113.9/32", &HashSet::new()).as_deref(), Some("203.0.113.9"));
        assert_eq!(next_free("203.0.113.0/31", &HashSet::new()).as_deref(), Some("203.0.113.0"));
        assert_eq!(next_free("nonsense", &HashSet::new()), None);
        assert_eq!(next_free("1.2.3.4/33", &HashSet::new()), None);
    }

    #[test]
    fn membership() {
        assert!(in_cidr("203.0.113.0/24", "203.0.113.77"));
        assert!(!in_cidr("203.0.113.0/24", "203.0.114.1"));
        assert!(in_cidr("0.0.0.0/0", "8.8.8.8"));
        assert!(!in_cidr("bad", "8.8.8.8"));
    }
}
