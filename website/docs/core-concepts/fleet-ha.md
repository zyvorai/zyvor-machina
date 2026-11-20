---
sidebar_position: 4
title: Fleet, HA and DRS
description: Turn hypervisors into one pool with HA failover, DRS and live migration.
---

# Fleet, HA and DRS

Add hypervisors by installing `machina-agent` on each one. The controller then treats them as one pool.

```bash
./scripts/deploy-remote.sh USER@HOST --platform --platform-bind 0.0.0.0
```

## What the controller does

- **Inventory and desired state.** The controller records what should be running where and reconciles drift.
- **HA failover.** When a host stops responding, its protected VMs are restarted on healthy hosts, with host fencing
  to avoid split-brain.
- **DRS.** Distributed resource scheduling rebalances load across hosts by live-migrating VMs.
- **Live migration.** Move running VMs between hosts from the UI or API.
- **Placement.** New VMs land on a host chosen by capacity and policy.
- **Fleet-wide eBPF.** Policies, shield, isolation and telemetry fan out to every host's `machina-bpfd` through its
  agent.

How failover, fencing, DRS and multi-controller leader election work in detail: [Controller HA](controller-ha.md).

![High availability and host fencing](/machina-fleet.png)

## Daemon-only fleets

Without the controller, `machina-daemon` can still aggregate peer daemons for a merged inventory:

```toml
[fleet]
enabled = true
primary_peer = "hv-east"
standby_peer = "hv-west"

[[fleet.peers]]
name = "hv-east"
url = "https://hv-east.example.com:5092"
api_token = "mach_…"
```

| Endpoint | Purpose |
| --- | --- |
| `GET /api/v1/fleet/status` | Peer health and versions |
| `GET /api/v1/fleet/vms` | Merged VM list (local and peers) |
| `POST /api/v1/fleet/peers/{name}/proxy` | Forward an API call to a peer |

This mode is for inventory and active/standby operation; automatic failover comes from the controller.
