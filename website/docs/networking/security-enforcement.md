---
sidebar_position: 4
title: Security enforcement
description: Policies, DDoS shield, node isolation, VM edge, QEMU sandbox and the VMM guard.
---

# Security enforcement

All enforcement follows the same rule: **observe first, enforce under a lease,
never persist**. Turn a feature on, watch what it would have blocked, then
enforce for a bounded time.

## Policies

Create policies per host or fleet-wide from **Platform → Security**. Kinds:

| Kind | Blocks |
|---|---|
| `deny_ip`, `deny_port` | Traffic to an address, CIDR or port |
| `tc_allow`, `allow_port` | Everything except an allowlist (VM interfaces only) |
| `deny_process`, `deny_file`, `deny_cap` | Exec of a binary, opening a path, a capability |
| `deny_dns` | Resolution of a name |
| `rate_limit` | New connections per VM above a rate (`100/s`, `50/s burst 200`) |

Fleet policies are scoped to the whole fleet, one host, one VM or one cgroup.

## Host protection

- **DDoS shield** — per-source token buckets at XDP for SYN, UDP, ICMP and other
  traffic, with allow and deny CIDRs. Audit counts what it would drop.
- **Node isolation** — an emergency switch that cuts a host off except for an
  allowlist (SSH and Machina's own ports by default). It has its own lease of at
  most 15 minutes and refuses to arm without SSH or an exempt address.

## VM protection

- **VM edge** — isolation groups and per-VM bandwidth / packet-rate limits on
  the VM's tap.
- **QEMU sandbox** — restricts which devices QEMU may open and where QEMU's own
  sockets may connect (loopback, migration and NBD ports).
- **VMM guard** — BPF-LSM hooks that flag or block unexpected exec, W+X memory
  and device opens by QEMU. Enforce requires BPF-LSM enabled in the kernel
  (`lsm=...,bpf`); otherwise it stays in audit.

Inside guests, see [Guest policy](guest-policy.md).

## Leases

| Feature | Lease |
|---|---|
| Policies, shield, VM edge, QEMU sandbox | Shared enforcement lease (default 900 s, `MACHINA_BPF_ENFORCE_LEASE_SECS`) |
| Node isolation | 10–900 s, mandatory |
| VMM guard, scheduler, guest policy | 1–3600 s |

When a lease ends the kernel stops dropping by itself.
