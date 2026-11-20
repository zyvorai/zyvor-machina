---
sidebar_position: 5
title: Controller HA
description: How HA failover, fencing, DRS and multi-controller leader election work.
---

# Controller HA

The controller keeps VMs running when a hypervisor fails, rebalances load, and can itself run as several instances.

## VM failover

1. A host whose agent heartbeat is older than **90 seconds** is marked offline.
2. The host is **fenced** before any of its VMs move.
3. Every **45 seconds** the HA engine restarts HA-protected VMs from offline hosts on healthy ones, least-loaded
   first, checking free memory so VMs spread out instead of piling onto one host.
4. Each outcome is recorded as an HA event (recovered, no capacity, retries exhausted).

Failover needs the VM's disks on storage the target host can reach: NFS, Ceph RBD or Atlas volumes.

## Fencing

Recovery waits until the failed host is **confirmed fenced**, so a partitioned host can never run the same VM twice.

- **IPMI**: the controller powers the host off through its BMC directly, which works even when the host is dead.
- **Command fallback**: if the host's agent is still reachable, it runs `MACHINA_FENCE_COMMAND`, for example
  `ipmitool -H {hostname} -U admin -P secret power off`.
- Failed fences are retried on every scan until they succeed or the host returns.
- Clusters with non-shared storage or external fencing can opt out (`allow_unfenced`), but VMs marked
  `fence_on_failure` always require a fence.

## DRS

DRS scores placements and live-migrates VMs off busy hosts. A move must clear a minimum score, and each destination
receives at most one migration per pass so it is never overcommitted. Maintenance mode evacuates a host with the same
capacity-aware placement.

## Running several controllers

Give each instance its own `MACHINA_CONTROLLER_ID` and point them all at the **same** state database:

- Leader election uses a 15-second lease row, renewed every 5 seconds; the leader steps down 3 seconds before expiry.
- Only the leader runs reconcile, HA, DRS and sync. Every instance serves the API.
- With the embedded SQLite store the instances share the database file (same host or a shared volume). There is no
  built-in database replication.
- Set `NATS_URL` so tasks enqueued on any instance reach all of them.

The web UI reaches the controller through the daemon's proxy, so nothing changes for operators when the leader moves.
