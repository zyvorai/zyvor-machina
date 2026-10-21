// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Reconciles Fleet Cloud / EC2 VPC forwarding into each host agent and
//! decorates the existing WireGuard overlay with VPC source ownership and
//! only the remote routes operators actually configured.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use machina_bpf::api::{VmOverlay, VmOverlayRoute};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbPool;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerRoute {
    pub source: String,
    pub destination: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InternetRoute {
    pub source: String,
    pub destination: String,
    #[serde(default)]
    pub uplink: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlackholeRoute {
    pub source: String,
    pub destination: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AclRule {
    pub subnet: String,
    pub egress: bool,
    pub rule_number: u32,
    pub protocol: String,
    pub action: String,
    pub cidr: String,
    pub port_from: Option<u16>,
    pub port_to: Option<u16>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct HostConfig {
    pub peer_routes: Vec<PeerRoute>,
    pub peer_ingress: Vec<PeerRoute>,
    /// Peered VPCs that live on this same host: forwarded bridge to bridge, no
    /// WireGuard hop. One-sided like the others: each direction needs its route.
    #[serde(default)]
    pub local_routes: Vec<PeerRoute>,
    pub internet_routes: Vec<InternetRoute>,
    pub blackholes: Vec<BlackholeRoute>,
    pub acls: Vec<AclRule>,
}

/// Extra WireGuard policy. `allowed` controls cryptographic source ownership;
/// `routes` controls which remote VPC CIDRs are installed in the host routing
/// table. Accepted peering makes a CIDR an allowed source, but no traffic is
/// routed until a route-table entry exists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OverlayPolicy {
    pub allowed: BTreeMap<(String, String), Vec<String>>,
    pub routes: BTreeMap<(String, String), Vec<String>>,
    /// Per-host routed source/destination pairs admitted by the overlay guard.
    pub direct: BTreeMap<String, Vec<VmOverlayRoute>>,
}

async fn vpc_host(pool: &DbPool, vpc: Uuid) -> anyhow::Result<Uuid> {
    Ok(
        crate::db::query_scalar("SELECT host_id FROM cloud_vpcs WHERE id = ?")
            .bind(vpc)
            .fetch_one(pool)
            .await?,
    )
}

/// True when any accepted peering has an installed route. The VM network-policy
/// overlay can remain disabled while Fleet Cloud still brings up the shared
/// authenticated WireGuard transport for routed VPC traffic.
pub async fn needs_overlay(pool: &DbPool) -> anyhow::Result<bool> {
    let native: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM cloud_routes r JOIN cloud_peerings p ON p.id = r.target_id \
         WHERE r.target = 'peering' AND p.status = 'planned'",
    )
    .fetch_one(pool)
    .await?;
    if native > 0 {
        return Ok(true);
    }
    let ec2: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM ec2_routes r JOIN cloud_peerings p ON p.id = r.target_id \
         WHERE r.target_kind = 'peering' AND p.status = 'planned'",
    )
    .fetch_one(pool)
    .await?;
    Ok(ec2 > 0)
}

/// Build the host forwarding policy from both the native cloud API tables and
/// the EC2 compatibility route-table/NACL tables.
pub async fn desired_for_host(pool: &DbPool, host: Uuid) -> anyhow::Result<HostConfig> {
    let mut out = HostConfig::default();

    // Native and EC2 peering routes. Build both the source host's outbound
    // policy and the remote host's inbound policy from the same route object;
    // this preserves one-sided route-table semantics.
    let native_peer: Vec<(Uuid, Uuid, String, String, Uuid, Uuid)> = crate::db::query_as(
        "SELECT v.id, v.host_id, v.cidr, r.destination, p.requester_id, p.accepter_id \
         FROM cloud_routes r \
         JOIN cloud_vpcs v ON v.id = r.vpc_id \
         JOIN cloud_peerings p ON p.id = r.target_id AND p.status = 'planned' \
         WHERE r.target = 'peering'",
    )
    .fetch_all(pool)
    .await?;
    for (local_vpc, local_host, source, destination, requester, accepter) in native_peer {
        let remote_vpc = if local_vpc == requester {
            accepter
        } else {
            requester
        };
        let remote_host = vpc_host(pool, remote_vpc).await?;
        if local_host == remote_host {
            // Both VPCs live on one host: nothing crosses machina-wg.
            if local_host == host {
                out.local_routes.push(PeerRoute {
                    source,
                    destination,
                });
            }
            continue;
        }
        if local_host == host {
            out.peer_routes.push(PeerRoute {
                source: source.clone(),
                destination: destination.clone(),
            });
        }
        if remote_host == host {
            out.peer_ingress.push(PeerRoute {
                source,
                destination,
            });
        }
    }

    let legacy_blackholes: Vec<(String, String)> = crate::db::query_as(
        "SELECT v.cidr, r.destination FROM cloud_routes r JOIN cloud_vpcs v ON v.id = r.vpc_id \
         WHERE v.host_id = ? AND r.target = 'blackhole'",
    )
    .bind(host)
    .fetch_all(pool)
    .await?;
    out.blackholes
        .extend(
            legacy_blackholes
                .into_iter()
                .map(|(source, destination)| BlackholeRoute {
                    source,
                    destination,
                }),
        );

    // EC2 route tables: explicit subnet association wins; main applies when
    // a subnet has no explicit association. Keep the source subnet in the
    // dataplane policy even though the kernel route itself is host-wide.
    let ec2_peer: Vec<(Uuid, Uuid, String, String, Uuid, Uuid)> = crate::db::query_as(
        "SELECT v.id, v.host_id, s.cidr, r.destination, p.requester_id, p.accepter_id \
         FROM ec2_routes r \
         JOIN ec2_route_tables t ON t.id = r.route_table_id \
         JOIN cloud_vpcs v ON v.id = t.vpc_id \
         JOIN cloud_subnets s ON s.vpc_id = v.id \
         LEFT JOIN ec2_route_table_assocs a ON a.subnet_id = s.id \
         JOIN cloud_peerings p ON p.id = r.target_id AND p.status = 'planned' \
         WHERE r.target_kind = 'peering' \
           AND (a.route_table_id = t.id OR (a.id IS NULL AND t.is_main = TRUE))",
    )
    .fetch_all(pool)
    .await?;
    for (local_vpc, local_host, source, destination, requester, accepter) in ec2_peer {
        let remote_vpc = if local_vpc == requester {
            accepter
        } else {
            requester
        };
        let remote_host = vpc_host(pool, remote_vpc).await?;
        if local_host == remote_host {
            // Both VPCs live on one host: nothing crosses machina-wg.
            if local_host == host {
                out.local_routes.push(PeerRoute {
                    source,
                    destination,
                });
            }
            continue;
        }
        if local_host == host {
            out.peer_routes.push(PeerRoute {
                source: source.clone(),
                destination: destination.clone(),
            });
        }
        if remote_host == host {
            out.peer_ingress.push(PeerRoute {
                source,
                destination,
            });
        }
    }

    let ec2_igw: Vec<(String, String)> = crate::db::query_as(
        "SELECT s.cidr, r.destination \
         FROM ec2_routes r \
         JOIN ec2_route_tables t ON t.id = r.route_table_id \
         JOIN cloud_vpcs v ON v.id = t.vpc_id \
         JOIN cloud_subnets s ON s.vpc_id = v.id \
         LEFT JOIN ec2_route_table_assocs a ON a.subnet_id = s.id \
         JOIN ec2_internet_gateways g ON g.id = r.target_id AND g.vpc_id = v.id \
         WHERE v.host_id = ? AND r.target_kind = 'igw' \
           AND (a.route_table_id = t.id OR (a.id IS NULL AND t.is_main = TRUE))",
    )
    .bind(host)
    .fetch_all(pool)
    .await?;
    out.internet_routes.extend(
        ec2_igw
            .into_iter()
            .map(|(source, destination)| InternetRoute {
                source,
                destination,
                uplink: None,
            }),
    );

    let ec2_blackholes: Vec<(String, String)> = crate::db::query_as(
        "SELECT s.cidr, r.destination \
         FROM ec2_routes r \
         JOIN ec2_route_tables t ON t.id = r.route_table_id \
         JOIN cloud_vpcs v ON v.id = t.vpc_id \
         JOIN cloud_subnets s ON s.vpc_id = v.id \
         LEFT JOIN ec2_route_table_assocs a ON a.subnet_id = s.id \
         WHERE v.host_id = ? AND r.target_kind = 'blackhole' \
           AND (a.route_table_id = t.id OR (a.id IS NULL AND t.is_main = TRUE))",
    )
    .bind(host)
    .fetch_all(pool)
    .await?;
    out.blackholes.extend(
        ec2_blackholes
            .into_iter()
            .map(|(source, destination)| BlackholeRoute {
                source,
                destination,
            }),
    );

    // Explicit subnet ACL association wins; otherwise use the VPC default ACL.
    let acl_rows: Vec<(String, i64, bool, String, String, String, Option<i64>, Option<i64>)> = crate::db::query_as(
        "SELECT s.cidr, e.rule_number, e.egress, e.protocol, e.rule_action, e.cidr, e.port_from, e.port_to \
         FROM cloud_subnets s \
         JOIN cloud_vpcs v ON v.id = s.vpc_id \
         JOIN ec2_network_acls n ON n.id = COALESCE( \
             (SELECT a.acl_id FROM ec2_acl_assocs a WHERE a.subnet_id = s.id), \
             (SELECT d.id FROM ec2_network_acls d WHERE d.vpc_id = v.id AND d.is_default = TRUE LIMIT 1) \
         ) \
         JOIN ec2_network_acl_entries e ON e.acl_id = n.id \
         WHERE v.host_id = ? ORDER BY s.cidr, e.egress, e.rule_number",
    )
    .bind(host)
    .fetch_all(pool)
    .await?;
    out.acls.extend(acl_rows.into_iter().map(
        |(subnet, n, egress, protocol, action, cidr, from, to)| AclRule {
            subnet,
            egress,
            rule_number: n.max(0) as u32,
            protocol,
            action,
            cidr,
            port_from: from.and_then(|v| u16::try_from(v).ok()),
            port_to: to.and_then(|v| u16::try_from(v).ok()),
        },
    ));

    out.peer_routes
        .sort_by(|a, b| (&a.source, &a.destination).cmp(&(&b.source, &b.destination)));
    out.peer_routes.dedup();
    out.peer_ingress
        .sort_by(|a, b| (&a.source, &a.destination).cmp(&(&b.source, &b.destination)));
    out.peer_ingress.dedup();
    out.local_routes
        .sort_by(|a, b| (&a.source, &a.destination).cmp(&(&b.source, &b.destination)));
    out.local_routes.dedup();
    out.internet_routes
        .sort_by(|a, b| (&a.source, &a.destination).cmp(&(&b.source, &b.destination)));
    out.internet_routes.dedup();
    out.blackholes
        .sort_by(|a, b| (&a.source, &a.destination).cmp(&(&b.source, &b.destination)));
    out.blackholes.dedup();
    Ok(out)
}

pub async fn push_host(pool: &DbPool, host: Uuid) -> anyhow::Result<()> {
    let addr: Option<String> = crate::db::query_scalar(
        "SELECT agent_grpc_addr FROM hosts WHERE id = ? AND state = 'online'",
    )
    .bind(host)
    .fetch_optional(pool)
    .await?;
    let addr = addr.ok_or_else(|| anyhow::anyhow!("the host is not online"))?;
    let cfg = desired_for_host(pool, host).await?;
    let mut client = crate::agent_client::connect(&addr).await?;
    crate::agent_client::host_libvirt_invoke(
        &mut client,
        "cloud.fabric.sync",
        &serde_json::to_value(&cfg)?,
    )
    .await?;
    Ok(())
}

/// Push every host that can be affected by `vpc`; useful for synchronous API
/// feedback after peering/route changes.
pub async fn push_vpc(pool: &DbPool, vpc: Uuid) -> anyhow::Result<()> {
    let mut hosts = BTreeSet::new();
    hosts.insert(vpc_host(pool, vpc).await?);
    let peers: Vec<Uuid> = crate::db::query_scalar(
        "SELECT CASE WHEN requester_id = ? THEN accepter_id ELSE requester_id END \
         FROM cloud_peerings WHERE status = 'planned' AND (requester_id = ? OR accepter_id = ?)",
    )
    .bind(vpc)
    .bind(vpc)
    .bind(vpc)
    .fetch_all(pool)
    .await?;
    for p in peers {
        hosts.insert(vpc_host(pool, p).await?);
    }
    for h in hosts {
        push_host(pool, h).await?;
    }
    Ok(())
}

/// Reconcile the shared WireGuard transport immediately instead of waiting for
/// the normal VM-network-policy tick. Two passes are intentional: the first
/// learns each host's WireGuard public key, the second can then install peers.
pub async fn reconcile_overlay_now(pool: &DbPool) {
    let hosts = super::bpf::online_hosts(pool).await;
    let host_addrs = super::vm_netpol::host_addresses(pool).await;
    let vms = super::vm_netpol::inventory(pool).await;
    super::vm_overlay::reconcile(pool, &hosts, &host_addrs, &vms).await;
    super::vm_overlay::reconcile(pool, &hosts, &host_addrs, &vms).await;
}

/// Apply host policy and the WireGuard control plane in one API-visible
/// convergence step. This is used after peering/route changes; periodic loops
/// still repair drift afterwards.
pub async fn converge_vpc(pool: &DbPool, vpc: Uuid) -> anyhow::Result<()> {
    push_vpc(pool, vpc).await?;
    reconcile_overlay_now(pool).await;
    // Re-apply host policy after overlay creation so a first-time deployment
    // never depends on rule ordering between the two reconcilers.
    push_vpc(pool, vpc).await
}

pub async fn peering_has_routes(pool: &DbPool, peering: Uuid) -> anyhow::Result<bool> {
    let native: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM cloud_routes WHERE target = 'peering' AND target_id = ?",
    )
    .bind(peering)
    .fetch_one(pool)
    .await?;
    if native > 0 {
        return Ok(true);
    }
    let ec2: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM ec2_routes WHERE target_kind = 'peering' AND target_id = ?",
    )
    .bind(peering)
    .fetch_one(pool)
    .await?;
    Ok(ec2 > 0)
}

/// Accepted peerings add both VPC CIDRs to each peer's WireGuard AllowedIPs.
/// Configured routes are a separate set and are the only VPC CIDRs installed
/// as destination routes by bpfd.
pub async fn overlay_policy(pool: &DbPool) -> anyhow::Result<OverlayPolicy> {
    let mut p = OverlayPolicy::default();
    let accepted: Vec<(Uuid, String, Uuid, String)> = crate::db::query_as(
        "SELECT a.host_id, a.cidr, b.host_id, b.cidr \
         FROM cloud_peerings x \
         JOIN cloud_vpcs a ON a.id = x.requester_id \
         JOIN cloud_vpcs b ON b.id = x.accepter_id \
         WHERE x.status = 'planned'",
    )
    .fetch_all(pool)
    .await?;
    for (ah, acidr, bh, bcidr) in accepted {
        let ah = ah.to_string();
        let bh = bh.to_string();
        p.allowed
            .entry((ah.clone(), bh.clone()))
            .or_default()
            .push(bcidr.clone());
        p.allowed
            .entry((bh.clone(), ah.clone()))
            .or_default()
            .push(acidr.clone());
    }

    // Native routes. Destination identifies the remote VPC/subnet; target_id identifies the peering.
    let native: Vec<(Uuid, String, Uuid, Uuid)> = crate::db::query_as(
        "SELECT r.vpc_id, r.destination, p.requester_id, p.accepter_id \
         FROM cloud_routes r JOIN cloud_peerings p ON p.id = r.target_id \
         WHERE r.target = 'peering' AND p.status = 'planned'",
    )
    .fetch_all(pool)
    .await?;
    for (local_vpc, dest, requester, accepter) in native {
        let remote = if local_vpc == requester {
            accepter
        } else {
            requester
        };
        let local_cidr: String =
            crate::db::query_scalar("SELECT cidr FROM cloud_vpcs WHERE id = ?")
                .bind(local_vpc)
                .fetch_one(pool)
                .await?;
        let lh = vpc_host(pool, local_vpc).await?.to_string();
        let rh = vpc_host(pool, remote).await?.to_string();
        p.routes
            .entry((lh, rh.clone()))
            .or_default()
            .push(dest.clone());
        p.direct.entry(rh).or_default().push(VmOverlayRoute {
            source: local_cidr,
            destination: dest,
        });
    }

    // EC2 route tables: each associated subnet is a separate source policy.
    let ec2: Vec<(Uuid, String, String, Uuid)> = crate::db::query_as(
        "SELECT t.vpc_id, s.cidr, r.destination, r.target_id \
         FROM ec2_routes r JOIN ec2_route_tables t ON t.id = r.route_table_id \
         JOIN cloud_subnets s ON s.vpc_id = t.vpc_id \
         LEFT JOIN ec2_route_table_assocs a ON a.subnet_id = s.id \
         WHERE r.target_kind = 'peering' AND r.target_id IS NOT NULL \
           AND (a.route_table_id = t.id OR (a.id IS NULL AND t.is_main = TRUE))",
    )
    .fetch_all(pool)
    .await?;
    for (local_vpc, source, dest, peering) in ec2 {
        let pair: Option<(Uuid, Uuid)> = crate::db::query_as(
            "SELECT requester_id, accepter_id FROM cloud_peerings WHERE id = ? AND status = 'planned'",
        )
        .bind(peering)
        .fetch_optional(pool)
        .await?;
        let Some((requester, accepter)) = pair else {
            continue;
        };
        let remote = if local_vpc == requester {
            accepter
        } else if local_vpc == accepter {
            requester
        } else {
            continue;
        };
        let lh = vpc_host(pool, local_vpc).await?.to_string();
        let rh = vpc_host(pool, remote).await?.to_string();
        p.routes
            .entry((lh, rh.clone()))
            .or_default()
            .push(dest.clone());
        p.direct.entry(rh).or_default().push(VmOverlayRoute {
            source,
            destination: dest,
        });
    }

    for v in p.allowed.values_mut() {
        v.sort();
        v.dedup();
    }
    for v in p.routes.values_mut() {
        v.sort();
        v.dedup();
    }
    for v in p.direct.values_mut() {
        v.sort_by(|a, b| (&a.source, &a.destination).cmp(&(&b.source, &b.destination)));
        v.dedup();
    }
    Ok(p)
}

pub fn decorate_overlay(plans: &mut BTreeMap<String, VmOverlay>, policy: &OverlayPolicy) {
    for (local, cfg) in plans {
        if let Some(extra) = policy.direct.get(local) {
            cfg.direct_routes.extend(extra.iter().cloned());
            cfg.direct_routes
                .sort_by(|a, b| (&a.source, &a.destination).cmp(&(&b.source, &b.destination)));
            cfg.direct_routes.dedup();
        }
        for peer in &mut cfg.peers {
            let key = (local.clone(), peer.host.clone());
            if let Some(extra) = policy.allowed.get(&key) {
                peer.prefixes.extend(extra.iter().cloned());
                peer.prefixes.sort();
                peer.prefixes.dedup();
            }
            if peer.routes.is_empty() {
                // Compatibility with a plan created before VmOverlayPeer.routes existed.
                peer.routes = peer.prefixes.clone();
            }
            if let Some(extra) = policy.routes.get(&key) {
                peer.routes.extend(extra.iter().cloned());
                peer.routes.sort();
                peer.routes.dedup();
            }
        }
    }
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
            let hosts: Vec<Uuid> = crate::db::query_scalar(
                "SELECT DISTINCT h.id FROM hosts h JOIN cloud_vpcs v ON v.host_id = h.id WHERE h.state = 'online'",
            )
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();
            for h in hosts {
                if let Err(e) = push_host(&state.pool, h).await {
                    tracing::warn!(host = %h, "cloud fabric sync: {e:#}");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use machina_bpf::api::VmOverlayPeer;

    #[test]
    fn overlay_distinguishes_allowed_sources_from_installed_routes() {
        let mut plans = BTreeMap::from([(
            "h1".into(),
            VmOverlay {
                enabled: true,
                peers: vec![VmOverlayPeer {
                    host: "h2".into(),
                    prefixes: vec!["100.96.2.0/24".into()],
                    routes: vec!["100.96.2.0/24".into()],
                    ..Default::default()
                }],
                ..Default::default()
            },
        )]);
        let p = OverlayPolicy {
            allowed: BTreeMap::from([(("h1".into(), "h2".into()), vec!["10.20.0.0/16".into()])]),
            routes: BTreeMap::from([(("h1".into(), "h2".into()), vec!["10.20.4.0/24".into()])]),
            direct: BTreeMap::from([(
                "h1".into(),
                vec![VmOverlayRoute {
                    source: "10.10.0.0/16".into(),
                    destination: "10.20.4.0/24".into(),
                }],
            )]),
        };
        decorate_overlay(&mut plans, &p);
        let peer = &plans["h1"].peers[0];
        assert!(peer.prefixes.contains(&"10.20.0.0/16".into()));
        assert!(peer.routes.contains(&"10.20.4.0/24".into()));
        assert!(!peer.routes.contains(&"10.20.0.0/16".into()));
        assert_eq!(plans["h1"].direct_routes.len(), 1);
    }
}
