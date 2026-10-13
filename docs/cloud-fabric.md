# Cloud fabric: VPC forwarding and network ACLs

Fleet Cloud / EC2 VPC routes are reconciled into the hosts. This page is the engineering reference; the operator
view is `website/docs/core-concepts/vpc-forwarding.md`. Status: **unit-tested; single-host live check passed;
cross-host and real-traffic runs not done** (claim C52).

## Components

| Piece | File | Role |
|---|---|---|
| Controller reconciler | `controller/src/engine/cloud_fabric.rs` | Builds a per-host `HostConfig` (`peer_routes`, `peer_ingress`, `internet_routes`, `blackholes`, `acls`) from `cloud_routes`, `ec2_routes`, `cloud_peerings`, `ec2_network_acls`, `ec2_acl_assocs`; pushes it as agent op `cloud.fabric.sync`; `converge_vpc` runs on any route/peering change, `spawn` repairs drift every 30 s |
| Overlay policy | same file (`overlay_policy`, `decorate_overlay`, `needs_overlay`) | Adds accepted-peering sources to `VmOverlayPeer.prefixes` and only configured destinations to `VmOverlayPeer.routes`; turns the overlay on for peering without changing the VM-network-policy setting |
| Agent dataplane | `agent/src/cloud_fabric.rs` | iptables chain `MACHINA_VPC_FWD` for peering/IGW forwarding and blackholes; nftables table `inet machina_cloud_acl` (forward hook, priority -250) for ACLs |
| bpfd overlay | `bpf/machina-bpf/src/netpol/overlay.rs`, `server/overlay.rs` | Validates `routes` and `direct_routes`, renders direct-route guard exceptions, programs `ip -4`/`ip -6` routes |
| REST/EC2 | `api/cloud/network.rs`, `api/ec2/{peering,route_tables}.rs` | Converge synchronously and report `forwarding_active` / `forwardingActive` |

## Semantics

- **Source ownership vs routes.** Accepting a peering adds the peer VPC CIDR to WireGuard `AllowedIPs` (`prefixes`);
  destinations are installed (`routes`, empty means `prefixes` for old configs) only for a route-table entry that
  selects the peering.
- **One-sided routes.** A reverse route is needed for replies, as in AWS.
- **`forwardingActive`** is true for `igw` and `peering` targets once the host applied the config. `nat` keeps the
  existing masquerade path; `instance` and `eni` targets remain stored plans.
- **ACLs** are stateless, ordered by rule number per subnet and direction, IPv4 only, with an implicit deny.
- **Drift.** `converge_vpc` is immediate; the 30 s loop repairs hand edits.

## Tests

`cargo test -p machina-agent cloud_fabric`, `-p machina-bpf netpol::overlay startup`,
`-p machina-controller engine::cloud_fabric` and the EC2 route-table tests. Live: create two VPCs on one host, peer
them, accept (expect `forwarding_active:false`), add the route (expect `true`), check `nft list ruleset` for
`machina_cloud_acl`, delete the route. Use `zz-*` names.

## Not covered

Cross-host traffic over `machina-wg`, IGW egress with real guests, ACL filtering of real flows, IPv6, transit routing,
and peering delete in the native API.
