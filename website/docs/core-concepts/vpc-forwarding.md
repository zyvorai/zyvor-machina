---
sidebar_position: 6
title: VPC forwarding and network ACLs
description: Peering, Internet Gateway and blackhole routes, and stateless network ACLs, enforced on the hosts.
---

# VPC forwarding and network ACLs

Fleet Cloud VPCs are more than stored objects. Peering routes, Internet Gateway routes, blackhole routes and EC2
network ACLs are reconciled into each host's datapath, so a route that says "forward" actually forwards and an ACL
that says "deny" actually drops.

:::note Verification status
Cross-host peering over WireGuard has been run on two real hosts with network namespaces on the subnet bridges: no
route blocks, routes in both directions forward (ping and TCP), a missing return route blocks, ingress and egress ACL
denies and blackhole routes block, and removing the routes blocks again. **Not yet run: Internet Gateway egress,
IPv6, and ACLs against real guest VMs.** See the claims ledger (`docs/claims.md`, C52).
:::

## What is enforced

| Object | Effect on the host |
| --- | --- |
| Accepted **peering** plus a route to it | The remote VPC CIDR is installed through the `machina-wg` overlay and forwarded for the source subnets that use the route |
| **Internet Gateway** route | Source subnets may forward to the destination through the host uplink (the IPv4 default route unless one is named) |
| **Blackhole** route | An explicit drop that wins over any forward rule |
| **Network ACL** | An ordered, stateless nftables chain with an implicit deny at the end |
| **NAT gateway** | Unchanged: the existing host masquerade for a `0.0.0.0/0` route |

## How a peering becomes traffic

0. **Where the VPCs live.** Each VPC belongs to one host. Peered VPCs on different hosts talk over `machina-wg`; peered
   VPCs on the **same** host are forwarded directly between their bridges. The rules are the same one-sided kind.
1. **Request and accept.** A peering needs an explicit request and acceptance by the target project. Accepting it
   makes each VPC's CIDR an *allowed source* from the other (WireGuard `AllowedIPs`), but routes **nothing**.
2. **Route.** Traffic flows only once a route table has a route whose target is the peering. The destination must fit
   the remote VPC's CIDR.
3. **Return path.** Routes are one-sided, as in AWS. The other VPC needs its own route back, or replies are dropped.

Separating source ownership from installed routes means an accepted peering cannot silently attract traffic.

```bash
# Native API (through the daemon proxy, or the controller on :5093)
curl -X POST $CTL/api/v1/cloud/vpcs/$A/peerings -d '{"accepter_id":"'$B'"}'
curl -X POST $CTL/api/v1/cloud/peerings/$P/accept
# => {"status":"active","forwarding_active":false}      no route yet
curl -X POST $CTL/api/v1/cloud/vpcs/$A/routes \
  -d '{"destination":"10.212.0.0/16","target":"peering","target_id":"'$P'"}'
# => {"status":"active","forwarding_active":true}
```

`forwarding_active` is `true` only for targets the host dataplane handles (peering and Internet Gateway). Instance and
network-interface targets stay stored plans.

## Network ACLs

ACL entries are rendered into an nftables base chain `inet machina_cloud_acl` hooked on `forward` at priority `-250`,
so it runs before libvirt, iptables-compat, Elastic IP and NAT rules and cannot be bypassed by a rule added later.

- Entries are evaluated in rule-number order, per direction and per subnet; the last rule is the implicit deny.
- ACLs are **stateless**: return traffic needs its own allow entry, as in AWS.
- IPv4 only in this release; an IPv6 CIDR is refused rather than ignored.

## Convergence

Route and peering changes, ACL entry changes, Internet Gateway attach and detach converge the VPC's hosts as soon as
the API call returns. A 30-second reconciler repairs drift (for example a rule flushed by hand) and picks up anything
changed by another path, so a change made that way takes up to about 30 seconds. The WireGuard overlay is brought up
for the first peering route and torn down after the last one. The EC2 compatibility API reports the result: `DescribeVpcPeeringConnections`
shows `active` with `forwardingActive=true` once a route exists, and `CreateRoute`/`ReplaceRoute` return
`forwardingActive` for the call.

## Limits

- IPv4 forwarding only.
- A peering across hosts needs the WireGuard overlay (UDP 51871 between hosts and `wireguard-tools` on every host).
  Peering activates the overlay by itself and tears it back down when the last peering route goes.
- No transit routing through a peer, no overlapping CIDRs, no cross-region peering.
- The native API has no peering delete yet, so a VPC that has a peering cannot be deleted through it.

Related: [Fleet Cloud](fleet-cloud.md), [EC2-compatible API](ec2-api.md), [VM network policy](../networking/security-enforcement.md).
