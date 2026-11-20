---
sidebar_position: 2
title: Load balancing
description: Maglev service load balancing, QUIC-LB at XDP, and Fleet Cloud load balancers.
---

# Load balancing

Machina has three load balancers for different jobs.

## Kubernetes services (Maglev)

Clusters that opt in to `machina-cni` use it instead of kube-proxy.
Services are load-balanced in the kernel:

- **ClusterIP, externalIP and LoadBalancer** are rewritten at `connect()` time
  (cgroup socket-LB), so no NAT happens per packet.
- **NodePort** is handled on the uplink at TC, or at XDP with `MACHINA_CNI_XDP=1`.
- Services with several backends use a **Maglev** consistent-hash table, so
  adding or removing a backend moves as few connections as possible.
- `sessionAffinity: ClientIP` is honoured, as is dual-stack.
- Remote backends use SNAT (default) or direct server return
  (`MACHINA_CNI_LB_MODE=dsr`).

The **Service LB** tab shows every service with its Maglev table and live
affinity entries.

## QUIC-LB

For QUIC services, bpfd can steer packets at XDP on the uplink. Packets are
routed by the server id in the QUIC connection ID, so a connection keeps its
backend even when the client's address changes; new connections use Maglev.
Delivery is direct server return or IPIP. Configure it in the **QUIC LB** tab or
with `PUT /api/v1/bpf/quic-lb`.

## Fleet Cloud load balancers

Fleet Cloud tenants create load balancers in front of their instances. These
are a separate engine: weighted round-robin iptables rules pushed to the owning
host's agent, with no load-balancer VM.
