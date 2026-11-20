// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! DNS threat feeds. While any feed exists, every running VM's taps are
//! programmed with DNS capture, and each reply naming a listed domain (or a
//! subdomain) raises a `threat_domain` alert. For a blocking feed, the
//! answer addresses get IDENTITY_THREAT in VM_IPS and every VM an egress
//! deny entry towards it, so connections show as AUDIT in observe mode and
//! drop under the enforcement lease. Bindings live as long as `toFQDNs` ones.

use chrono::Utc;

use crate::api::{VmThreatBlock, VmThreatFeed, VmThreatStatus};
use crate::netpol::{threat, IDENTITY_THREAT};

use super::*;

/// A blocking feed's answer address seen in a VM DNS reply.
pub(in crate::server) struct ThreatHit {
    pub addr: [u8; ADDR_LEN],
    pub domain: String,
    pub feed: String,
    pub vm: String,
    pub ttl: u32,
}

pub(in crate::server) struct Feed {
    pub info: VmThreatFeed,
    pub domains: Vec<String>,
}

pub(in crate::server) struct Bound {
    domain: String,
    feed: String,
    vm: String,
    expires: Instant,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Saved {
    feeds: Vec<(VmThreatFeed, Vec<String>)>,
}

/// Check one VM DNS reply against the feeds: alerts for listed names, and
/// blocking answers queued for the engine (true when queued).
pub(in crate::server) fn threat_observe(
    s: &mut Shared,
    msg: &crate::dns::DnsMessage,
    vm: &str,
    client: &str,
) -> (bool, Vec<VmFlowAlert>) {
    let mut alerts = Vec::new();
    if !msg.is_response {
        return (false, alerts);
    }
    if msg.rcode == 0 {
        alerts.extend(s.flow_hist.observe_domain(vm, client, &msg.qname));
    }
    if s.vm_threat.is_empty() {
        return (false, alerts);
    }
    let mut names = vec![msg.qname.clone()];
    names.extend(msg.answers.iter().map(|a| a.name.clone()));
    let addrs: Vec<IpAddr> = msg
        .answers
        .iter()
        .filter(|a| a.rtype == "A" || a.rtype == "AAAA")
        .filter_map(|a| a.data.parse().ok())
        .collect();
    let hit = names.iter().find_map(|n| {
        threat::lookup(&s.vm_threat, n).map(|(d, (f, b))| (d.to_string(), f.clone(), *b))
    });
    let Some((domain, feed, block)) = hit else {
        return (false, alerts);
    };
    let shown: Vec<String> = addrs.iter().map(|a| a.to_string()).collect();
    let outcome = match (block, addrs.is_empty()) {
        (_, true) => "no address".to_string(),
        (true, false) => format!("{}; egress denied", shown.join(", ")),
        (false, false) => format!("{}; alert only", shown.join(", ")),
    };
    alerts.extend(s.flow_hist.threat_alert(
        vm,
        client,
        &msg.qname,
        format!(
            "{vm} resolved {} (listed as {domain} in feed {feed}) → {outcome}",
            msg.qname.trim_end_matches('.')
        ),
    ));
    if !block {
        return (false, alerts);
    }
    let ttl = msg
        .answers
        .iter()
        .filter(|a| a.rtype == "A" || a.rtype == "AAAA")
        .map(|a| a.ttl)
        .max()
        .unwrap_or(0);
    for a in &addrs {
        s.vm_threat_queue.push(ThreatHit {
            addr: policy::ip_to_addr(*a),
            domain: domain.clone(),
            feed: feed.clone(),
            vm: vm.to_string(),
            ttl,
        });
    }
    (!addrs.is_empty(), alerts)
}

impl VmEdgeRuntime {
    pub(super) fn threat_watch(&self) -> bool {
        !self.threat_feeds.is_empty()
    }

    pub(super) fn threat_blocking(&self) -> bool {
        !self.threat_bound.is_empty()
    }

    /// Every VM identity with edge state or a programmed tap.
    fn threat_subjects(&self) -> BTreeSet<u32> {
        self.state
            .vms
            .iter()
            .map(vm_ident)
            .chain(
                self.threat_vms
                    .iter()
                    .filter(|n| !self.state.vms.iter().any(|v| &v.name == *n))
                    .map(|n| crate::netpol::vm_identity(n)),
            )
            .collect()
    }

    /// Bound addresses and deny entries for `vm_edge_apply`. Known VM and
    /// peer addresses are never rebound, so a hostile answer cannot cut off
    /// a VM.
    pub(super) fn threat_entries(
        &self,
        ips: &mut HashMap<[u8; ADDR_LEN], u32>,
        policy: &mut HashMap<PolicyKey, u32>,
        index: &mut FlowIndex,
    ) {
        let mut any = false;
        for addr in self.threat_bound.keys() {
            if self.base_ips.contains_key(addr) {
                continue;
            }
            ips.insert(*addr, IDENTITY_THREAT);
            any = true;
        }
        if !any {
            return;
        }
        index
            .names
            .insert(IDENTITY_THREAT, ("threat-domain".into(), BTreeMap::new()));
        for subject in self.threat_subjects() {
            let k = PolicyKey {
                subject_identity: subject,
                peer_identity: IDENTITY_THREAT,
                direction: POLICY_EGRESS,
                proto: 0,
                port: 0u16.to_be_bytes(),
            };
            policy.insert(k, VM_POLICY_DENY);
            index.rules.insert(
                (subject, IDENTITY_THREAT, true, 0, 0),
                (true, "threat feed".into()),
            );
        }
    }

    fn threat_lists(&self) -> HashMap<String, (String, bool)> {
        let mut m: HashMap<String, (String, bool)> = HashMap::new();
        for f in self.threat_feeds.values() {
            for d in &f.domains {
                let e = m
                    .entry(d.clone())
                    .or_insert_with(|| (f.info.name.clone(), f.info.block));
                if f.info.block && !e.1 {
                    *e = (f.info.name.clone(), true);
                }
            }
        }
        m
    }
}

impl Engine {
    pub(in crate::server) fn threat_set_path(&mut self, path: std::path::PathBuf) {
        self.vm_edge.threat_path = Some(path);
    }

    pub(in crate::server) fn vm_threat_feed_set(
        &mut self,
        name: &str,
        source: String,
        block: bool,
        domains: Vec<String>,
    ) -> Result<VmThreatFeed> {
        threat::check_name(name).map_err(|e| anyhow!(e))?;
        let mut list: Vec<String> = domains
            .iter()
            .filter_map(|d| threat::normalize(d))
            .collect();
        list.sort();
        list.dedup();
        if list.len() > threat::MAX_DOMAINS {
            return Err(anyhow!(
                "threat feed {name}: {} domains, at most {}",
                list.len(),
                threat::MAX_DOMAINS
            ));
        }
        let info = VmThreatFeed {
            name: name.to_string(),
            source,
            block,
            domains: list.len(),
            updated: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            hostname: None,
        };
        self.vm_edge.threat_feeds.insert(
            name.to_string(),
            Feed {
                info: info.clone(),
                domains: list,
            },
        );
        self.threat_changed()?;
        Ok(info)
    }

    pub(in crate::server) fn vm_threat_feed_remove(&mut self, name: &str) -> Result<bool> {
        if self.vm_edge.threat_feeds.remove(name).is_none() {
            return Ok(false);
        }
        self.threat_changed()?;
        Ok(true)
    }

    /// Publish the lists to the DNS reader, drop bindings no blocking feed
    /// lists any more, reapply and save.
    fn threat_changed(&mut self) -> Result<()> {
        let lists = self.vm_edge.threat_lists();
        self.vm_edge
            .threat_bound
            .retain(|_, b| lists.get(&b.domain).is_some_and(|(_, block)| *block));
        lock(&self.shared).vm_threat = lists;
        if !self.vm_edge.threat_watch() {
            self.vm_edge.threat_vms.clear();
        }
        self.vm_edge_apply()?;
        self.vm_edge_refresh();
        self.threat_save();
        Ok(())
    }

    fn threat_save(&self) {
        let Some(path) = &self.vm_edge.threat_path else {
            return;
        };
        let saved = Saved {
            feeds: self
                .vm_edge
                .threat_feeds
                .values()
                .map(|f| (f.info.clone(), f.domains.clone()))
                .collect(),
        };
        let tmp = path.with_extension("json.tmp");
        let res = serde_json::to_vec(&saved)
            .map_err(anyhow::Error::from)
            .and_then(|b| std::fs::write(&tmp, b).map_err(Into::into))
            .and_then(|_| std::fs::rename(&tmp, path).map_err(Into::into));
        if let Err(e) = res {
            tracing::warn!("threat feeds save: {e:#}");
        }
    }

    pub(in crate::server) fn threat_restore(&mut self) {
        let Some(path) = self.vm_edge.threat_path.clone() else {
            return;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        match serde_json::from_slice::<Saved>(&bytes) {
            Ok(s) => {
                for (info, domains) in s.feeds {
                    self.vm_edge
                        .threat_feeds
                        .insert(info.name.clone(), Feed { info, domains });
                }
                if let Err(e) = self.threat_changed() {
                    tracing::warn!("restore threat feeds: {e:#}");
                }
            }
            Err(e) => tracing::warn!("threat feeds {}: {e}", path.display()),
        }
    }

    /// Take queued blocking answers, expire old bindings, reapply on change.
    pub(in crate::server) fn threat_tick(&mut self) -> Result<()> {
        let queue = std::mem::take(&mut lock(&self.shared).vm_threat_queue);
        if queue.is_empty() && self.vm_edge.threat_bound.is_empty() {
            return Ok(());
        }
        let now = Instant::now();
        let was_blocking = self.vm_edge.threat_blocking();
        let mut changed = false;
        for h in queue {
            let ttl = Duration::from_secs(h.ttl as u64).clamp(FQDN_MIN_TTL, FQDN_MAX_TTL);
            let b = self.vm_edge.threat_bound.entry(h.addr).or_insert_with(|| {
                changed = true;
                Bound {
                    domain: String::new(),
                    feed: String::new(),
                    vm: String::new(),
                    expires: now,
                }
            });
            b.domain = h.domain;
            b.feed = h.feed;
            b.vm = h.vm;
            b.expires = b.expires.max(now + ttl);
        }
        let before = self.vm_edge.threat_bound.len();
        self.vm_edge.threat_bound.retain(|_, b| b.expires > now);
        changed |= self.vm_edge.threat_bound.len() != before;
        while self.vm_edge.threat_bound.len() > FQDN_CACHE_CAP {
            let oldest = self
                .vm_edge
                .threat_bound
                .iter()
                .min_by_key(|(_, b)| b.expires)
                .map(|(a, _)| *a);
            match oldest {
                Some(a) => {
                    self.vm_edge.threat_bound.remove(&a);
                }
                None => break,
            }
        }
        if changed {
            self.vm_edge_apply()?;
            if was_blocking != self.vm_edge.threat_blocking() {
                self.vm_edge_refresh();
            }
        }
        Ok(())
    }

    pub(in crate::server) fn vm_threat_status(&self) -> VmThreatStatus {
        let now = Instant::now();
        let mut blocked: Vec<VmThreatBlock> = self
            .vm_edge
            .threat_bound
            .iter()
            .map(|(a, b)| VmThreatBlock {
                address: fmt_addr(a),
                domain: b.domain.clone(),
                feed: b.feed.clone(),
                vm: b.vm.clone(),
                expires_in_secs: b.expires.saturating_duration_since(now).as_secs(),
            })
            .collect();
        blocked.sort_by(|a, b| a.domain.cmp(&b.domain).then(a.address.cmp(&b.address)));
        let watched: BTreeSet<&str> = self
            .vm_edge
            .taps
            .values()
            .map(|(_, v)| v.as_str())
            .collect();
        VmThreatStatus {
            feeds: self
                .vm_edge
                .threat_feeds
                .values()
                .map(|f| f.info.clone())
                .collect(),
            blocked,
            watched_vms: if self.vm_edge.threat_watch() {
                watched.len()
            } else {
                0
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_addresses_get_deny_entries_for_every_vm() {
        let mut rt = VmEdgeRuntime::default();
        rt.state.vms.push(VmEdgeVm {
            name: "web".into(),
            identity: Some(77),
            ..Default::default()
        });
        rt.threat_vms.insert("other".into());
        let known = addr16("10.0.0.5").unwrap();
        rt.base_ips.insert(known, 77);
        let bad = addr16("203.0.113.9").unwrap();
        for a in [bad, known] {
            rt.threat_bound.insert(
                a,
                Bound {
                    domain: "evil.example".into(),
                    feed: "f".into(),
                    vm: "web".into(),
                    expires: Instant::now() + Duration::from_secs(60),
                },
            );
        }
        let (mut ips, mut policy, mut index) =
            (HashMap::new(), HashMap::new(), FlowIndex::default());
        rt.threat_entries(&mut ips, &mut policy, &mut index);
        assert_eq!(ips.get(&bad), Some(&IDENTITY_THREAT));
        assert!(!ips.contains_key(&known), "VM addresses are never rebound");
        let subjects: BTreeSet<u32> = policy.keys().map(|k| k.subject_identity).collect();
        assert_eq!(
            subjects,
            [77, crate::netpol::vm_identity("other")]
                .into_iter()
                .collect()
        );
        assert!(policy
            .iter()
            .all(|(k, v)| k.peer_identity == IDENTITY_THREAT
                && k.direction == POLICY_EGRESS
                && *v == VM_POLICY_DENY));
    }

    #[test]
    fn a_blocking_feed_wins_for_a_shared_domain() {
        let mut rt = VmEdgeRuntime::default();
        for (name, block) in [("a", false), ("b", true)] {
            rt.threat_feeds.insert(
                name.into(),
                Feed {
                    info: VmThreatFeed {
                        name: name.into(),
                        block,
                        ..Default::default()
                    },
                    domains: vec!["evil.example".into()],
                },
            );
        }
        assert_eq!(
            rt.threat_lists().get("evil.example"),
            Some(&("b".to_string(), true))
        );
    }
}
