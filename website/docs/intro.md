---
sidebar_position: 0
title: Introduction
description: What Machina is, what it replaces, and where to start reading.
---

# Introduction

Machina is a private cloud for KVM hosts you own. It gives you VM management, browser consoles, a multi-host fleet
with HA and DRS, an OpenStack-style self-service cloud, a native eBPF network datapath and AI-assisted operations,
from a handful of Rust services on plain Linux.

## Who it is for

- **Teams leaving VMware** who want HA, DRS and live migration on open KVM/libvirt.
- **Teams that looked at OpenStack** and wanted the same primitives without a six-week deployment project.
- **Operators of labs, branches and sovereign regions** who need one person to install, understand and upgrade the
  whole stack.

## The pieces

| You run | You get |
| --- | --- |
| `machina-daemon` on a host | VMs, storage, networks, consoles, sign-in and RBAC, a REST API and the web UI |
| `+ machina-controller` and an agent per host | A fleet: HA failover with fencing, DRS, live migration, Fleet Cloud, Zyra AI |
| `+ machina-bpfd` on each host | eBPF load balancing, Kubernetes CNI, DDoS shield, VM isolation and flow visibility |
| `+ BMC addresses` of machines with no OS | [Bare-metal provisioning](core-concepts/bare-metal.md): discover, commission, deploy an image, join |
| `+ VPCs` that must reach each other or the internet | [VPC forwarding and network ACLs](core-concepts/vpc-forwarding.md) |
| `+ a production site` | [Production hardening](operations/production-hardening.md): strict profile, key rollover, audit retention |

See [Architecture](core-concepts/architecture.md) for how they connect.

## What it replaces

| Instead of | Machina uses |
| --- | --- |
| Nova, Glance, Cinder, Heat, Octavia | Fleet Cloud on the controller |
| Masakari and Watcher | Built-in HA and DRS |
| A VNC/SPICE gateway | Console proxies in the daemon |
| Cilium, Tetragon, kube-proxy, a firewall agent | `machina-bpfd` and `machina-cni` |
| MariaDB/Galera and RabbitMQ | Embedded SQLite or PostgreSQL, optional NATS |
| Nova, Neutron or Keystone clients | [The EC2-compatible API](core-concepts/ec2-api.md) and Machina's REST API |

Moving off OpenStack? Read [Replacing OpenStack](getting-started/from-openstack.md), which includes what Machina does not cover.

## Where to start

1. [Requirements](getting-started/requirements.md), then the [Quickstart](getting-started/quickstart.md).
2. [Virtual machines](core-concepts/virtual-machines.md) and [Consoles](core-concepts/consoles.md) for day one.
3. [Fleet, HA and DRS](core-concepts/fleet-ha.md) when you add a second host.
4. [Native eBPF](networking/ebpf-overview.md) for networking and security.
5. [Configuration](operations/configuration.md), [Upgrade and backup](operations/upgrade-backup.md) and
   [Troubleshooting](operations/troubleshooting.md) for operations.
6. The [Reference](reference/ports.md) section for ports, environment variables and the API.
