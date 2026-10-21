// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Failure domains: a host's `site` + `rack` labels name the unit that is lost together
//! (a rack's PDU or top-of-rack switch, a room). Two things use them:
//!
//! * placement spreads anti-affinity groups across domains, not just hosts
//!   ([`widen_to_domains`]), as a preference with the same fallback as host anti-affinity;
//! * [`analyze`] answers "if this domain went dark, could HA restart what ran in it on the
//!   hosts that remain?" (the rack-loss readiness report behind `GET /api/v1/failure-domains`).
//!
//! Hosts with no `rack` label belong to no domain: nothing is spread or counted for them.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;
use uuid::Uuid;

use crate::db::DbPool;

/// Stable key for a domain: `site/rack`, or just `rack` when the site is empty.
pub fn domain_key(site: &str, rack: &str) -> Option<String> {
    let rack = rack.trim();
    if rack.is_empty() {
        return None;
    }
    let site = site.trim();
    Some(if site.is_empty() {
        rack.to_string()
    } else {
        format!("{site}/{rack}")
    })
}

/// One host as the analysis sees it.
#[derive(Debug, Clone)]
pub struct DomainHost {
    pub id: Uuid,
    pub domain: Option<String>,
    pub online: bool,
    /// Online, not in maintenance and schedulable: can receive a recovered VM.
    pub can_take_vms: bool,
    pub memory_total_mib: i64,
    pub memory_used_mib: i64,
    pub vm_count: i64,
    /// Memory of running VMs with an enabled HA policy on this host.
    pub ha_memory_mib: i64,
    pub ha_vm_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DomainReport {
    pub domain: String,
    pub hosts: usize,
    pub hosts_online: usize,
    pub vms: i64,
    pub ha_vms: i64,
    pub memory_total_mib: i64,
    pub memory_used_mib: i64,
    /// Memory HA would have to find elsewhere if the whole domain were lost.
    pub ha_memory_to_recover_mib: i64,
    /// Free memory on schedulable hosts outside this domain.
    pub headroom_outside_mib: i64,
    /// Hosts outside this domain that could take recovered VMs.
    pub hosts_outside: usize,
    /// `headroom_outside_mib - ha_memory_to_recover_mib` (negative = shortfall).
    pub spare_after_loss_mib: i64,
    /// True when the HA VMs fit in the memory left outside the domain.
    pub survivable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FailureDomainsReport {
    pub domains: Vec<DomainReport>,
    /// Hosts with no rack label (not part of any domain).
    pub unassigned_hosts: usize,
    /// True when there are at least two domains and every one is survivable.
    pub rack_loss_tolerant: bool,
    /// Plain-language problems to fix, empty when tolerant.
    pub findings: Vec<String>,
}

/// Rack-loss readiness. Capacity is a memory check only (the hard OOM constraint HA also
/// uses); it does not prove network, storage or quorum survive the loss.
pub fn analyze(hosts: &[DomainHost]) -> FailureDomainsReport {
    let mut by_domain: BTreeMap<String, Vec<&DomainHost>> = BTreeMap::new();
    let mut unassigned = 0usize;
    for h in hosts {
        match &h.domain {
            Some(d) => by_domain.entry(d.clone()).or_default().push(h),
            None => unassigned += 1,
        }
    }

    let mut domains = Vec::with_capacity(by_domain.len());
    for (name, members) in &by_domain {
        let outside: Vec<&DomainHost> = hosts
            .iter()
            .filter(|h| h.can_take_vms && h.domain.as_ref() != Some(name))
            .collect();
        let headroom_outside: i64 = outside
            .iter()
            .map(|h| (h.memory_total_mib - h.memory_used_mib).max(0))
            .sum();
        let ha_memory: i64 = members.iter().map(|h| h.ha_memory_mib).sum();
        let spare = headroom_outside - ha_memory;
        domains.push(DomainReport {
            domain: name.clone(),
            hosts: members.len(),
            hosts_online: members.iter().filter(|h| h.online).count(),
            vms: members.iter().map(|h| h.vm_count).sum(),
            ha_vms: members.iter().map(|h| h.ha_vm_count).sum(),
            memory_total_mib: members.iter().map(|h| h.memory_total_mib).sum(),
            memory_used_mib: members.iter().map(|h| h.memory_used_mib).sum(),
            ha_memory_to_recover_mib: ha_memory,
            headroom_outside_mib: headroom_outside,
            hosts_outside: outside.len(),
            spare_after_loss_mib: spare,
            survivable: spare >= 0,
        });
    }

    let mut findings = Vec::new();
    if domains.len() < 2 {
        findings.push(
            "Fewer than two failure domains: label hosts with a rack (and site) so a rack loss can be reasoned about."
                .to_string(),
        );
    }
    for d in &domains {
        if !d.survivable {
            findings.push(format!(
                "Losing {} leaves {} MiB of HA memory without room (short by {} MiB).",
                d.domain,
                d.ha_memory_to_recover_mib,
                -d.spare_after_loss_mib
            ));
        }
    }
    if unassigned > 0 && !domains.is_empty() {
        findings.push(format!("{unassigned} host(s) have no rack label and are not counted in any domain."));
    }
    let rack_loss_tolerant = domains.len() >= 2 && domains.iter().all(|d| d.survivable);
    FailureDomainsReport {
        domains,
        unassigned_hosts: unassigned,
        rack_loss_tolerant,
        findings,
    }
}

/// host -> anti-affinity tags of the VMs running on it, widened so each host also carries
/// the tags of every other host in its domain. Placement then treats "a peer in my rack"
/// like "a peer on my host", still as a preference (callers fall back when every host would
/// violate). Hosts outside any domain are left as they were.
pub fn widen_to_domains(
    anti_map: &HashMap<Uuid, HashSet<String>>,
    host_domain: &HashMap<Uuid, String>,
) -> HashMap<Uuid, HashSet<String>> {
    let mut per_domain: HashMap<&str, HashSet<String>> = HashMap::new();
    for (host, tags) in anti_map {
        if let Some(d) = host_domain.get(host) {
            per_domain.entry(d.as_str()).or_default().extend(tags.iter().cloned());
        }
    }
    let mut out = anti_map.clone();
    for (host, d) in host_domain {
        if let Some(tags) = per_domain.get(d.as_str()) {
            out.entry(*host).or_default().extend(tags.iter().cloned());
        }
    }
    out
}

/// host id -> domain key for every host that has a rack label.
pub async fn host_domains(pool: &DbPool) -> anyhow::Result<HashMap<Uuid, String>> {
    let rows: Vec<(Uuid, String, String)> = crate::db::query_as(
        "SELECT id, COALESCE(site, '') AS site, COALESCE(rack, '') AS rack FROM hosts",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, site, rack)| domain_key(&site, &rack).map(|d| (id, d)))
        .collect())
}

type HostRow = (Uuid, String, String, String, bool, bool, i64, i64, i64);

/// Load every host with its HA footprint and run [`analyze`].
pub async fn load_report(pool: &DbPool) -> anyhow::Result<FailureDomainsReport> {
    let rows: Vec<HostRow> = crate::db::query_as(
        "SELECT h.id, COALESCE(h.site, '') AS site, COALESCE(h.rack, '') AS rack, h.state,
                h.maintenance_mode, h.schedulable, h.memory_total_mib, h.memory_used_mib,
                CAST(h.vm_count AS BIGINT) AS vm_count
         FROM hosts h",
    )
    .fetch_all(pool)
    .await?;

    let ha: Vec<(Uuid, i64, i64)> = crate::db::query_as(
        "SELECT v.host_id, CAST(COALESCE(SUM(v.memory_mib), 0) AS BIGINT) AS mem, COUNT(*) AS n
         FROM vms v
         JOIN ha_policies hp ON hp.vm_id = v.id AND hp.enabled = TRUE
         WHERE v.desired_state = 'running' AND v.host_id IS NOT NULL
         GROUP BY v.host_id",
    )
    .fetch_all(pool)
    .await?;
    let ha: HashMap<Uuid, (i64, i64)> = ha.into_iter().map(|(h, m, n)| (h, (m, n))).collect();

    let hosts: Vec<DomainHost> = rows
        .into_iter()
        .map(|(id, site, rack, state, maintenance, schedulable, total, used, vm_count)| {
            let online = state == "online";
            let (ha_mem, ha_n) = ha.get(&id).copied().unwrap_or((0, 0));
            DomainHost {
                id,
                domain: domain_key(&site, &rack),
                online,
                can_take_vms: online && !maintenance && schedulable,
                memory_total_mib: total,
                memory_used_mib: used,
                vm_count,
                ha_memory_mib: ha_mem,
                ha_vm_count: ha_n,
            }
        })
        .collect();
    Ok(analyze(&hosts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(domain: Option<&str>, total: i64, used: i64, ha_mem: i64) -> DomainHost {
        DomainHost {
            id: Uuid::new_v4(),
            domain: domain.map(str::to_string),
            online: true,
            can_take_vms: true,
            memory_total_mib: total,
            memory_used_mib: used,
            vm_count: if ha_mem > 0 { 1 } else { 0 },
            ha_memory_mib: ha_mem,
            ha_vm_count: if ha_mem > 0 { 1 } else { 0 },
        }
    }

    #[test]
    fn domain_key_needs_a_rack() {
        assert_eq!(domain_key("dc1", "r1").as_deref(), Some("dc1/r1"));
        assert_eq!(domain_key("", " r1 ").as_deref(), Some("r1"));
        assert_eq!(domain_key("dc1", "  "), None);
    }

    #[test]
    fn two_racks_with_room_survive() {
        let hosts = vec![
            host(Some("dc1/a"), 1000, 500, 400),
            host(Some("dc1/b"), 1000, 100, 0),
        ];
        let r = analyze(&hosts);
        assert!(r.rack_loss_tolerant);
        let a = r.domains.iter().find(|d| d.domain == "dc1/a").unwrap();
        assert_eq!(a.headroom_outside_mib, 900);
        assert_eq!(a.spare_after_loss_mib, 500);
        assert!(a.survivable);
        assert!(r.findings.is_empty());
    }

    #[test]
    fn shortfall_names_the_rack() {
        let hosts = vec![
            host(Some("a"), 1000, 900, 800),
            host(Some("b"), 1000, 700, 0),
        ];
        let r = analyze(&hosts);
        assert!(!r.rack_loss_tolerant);
        let a = r.domains.iter().find(|d| d.domain == "a").unwrap();
        assert_eq!(a.spare_after_loss_mib, 300 - 800);
        assert!(r.findings.iter().any(|f| f.contains("Losing a") && f.contains("500")));
    }

    #[test]
    fn one_domain_is_not_tolerant_and_cordoned_hosts_do_not_count() {
        let r = analyze(&[host(Some("a"), 1000, 0, 0), host(Some("a"), 1000, 0, 0)]);
        assert!(!r.rack_loss_tolerant);
        assert!(r.findings[0].contains("Fewer than two"));

        let mut cordoned = host(Some("b"), 1000, 0, 0);
        cordoned.can_take_vms = false;
        let r = analyze(&[host(Some("a"), 1000, 500, 100), cordoned]);
        let a = r.domains.iter().find(|d| d.domain == "a").unwrap();
        assert_eq!(a.hosts_outside, 0);
        assert!(!a.survivable);
    }

    #[test]
    fn unlabelled_hosts_are_reported_not_counted() {
        let r = analyze(&[
            host(Some("a"), 1000, 0, 0),
            host(Some("b"), 1000, 0, 0),
            host(None, 1000, 0, 0),
        ]);
        assert_eq!(r.unassigned_hosts, 1);
        assert!(r.findings.iter().any(|f| f.contains("no rack label")));
        // The unlabelled host is outside both racks and offers headroom.
        let a = r.domains.iter().find(|d| d.domain == "a").unwrap();
        assert_eq!(a.hosts_outside, 2);
    }

    #[test]
    fn widen_spreads_anti_affinity_across_a_rack() {
        let (h1, h2, h3, h4) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let mut anti: HashMap<Uuid, HashSet<String>> = HashMap::new();
        anti.entry(h1).or_default().insert("anti-affinity:db".into());
        let domains: HashMap<Uuid, String> = [
            (h1, "a".to_string()),
            (h2, "a".to_string()),
            (h3, "b".to_string()),
        ]
        .into_iter()
        .collect();
        let wide = widen_to_domains(&anti, &domains);
        assert!(wide[&h2].contains("anti-affinity:db")); // same rack as h1
        assert!(!wide.contains_key(&h3)); // other rack stays clean
        assert!(!wide.contains_key(&h4)); // no domain: untouched
    }
}
