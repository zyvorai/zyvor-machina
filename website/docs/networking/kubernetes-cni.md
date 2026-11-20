---
sidebar_position: 3
title: Kubernetes CNI
description: Opt-in machina-cni replaces flannel, kube-proxy and Cilium; clusters keep their default CNI unless you choose it.
---

# Kubernetes CNI

**`machina-cni`** is opt-in. When Machina bootstraps a k3s cluster it keeps
k3s's default networking (flannel, NetworkPolicy controller, kube-proxy)
unless you ask for machina-cni: tick **Use machina-cni** in Kubernetes →
Cluster bootstrap, or send `"cni": "machina"` to
`POST /api/v1/k8s/cluster-bootstrap`. k3s is then installed without flannel,
kube-proxy and network policy, and machina-cni provides all three.

The agent never takes over a node that already has another CNI configured
(flannel, Calico, Cilium, …): it exits with status 78 and changes nothing.
Set `MACHINA_CNI_TAKEOVER=1` in `/etc/default/machina-cni` only to replace the
existing CNI on purpose.

## What you get

- **Pod networking**: one veth per pod, a `/32` (and `/128` in dual-stack) per
  pod, direct routes between nodes, outbound masquerade.
- **NetworkPolicy**: label selectors, `ipBlock` CIDRs and ports, enforced in
  eBPF.
- **Services**: kernel load balancing with Maglev (see
  [Load balancing](load-balancing.md)).
- **Cilium policy migration** (opt-in, `MACHINA_CNI_CILIUM_POLICIES=1`):
  existing `CiliumNetworkPolicy` and `CiliumClusterwideNetworkPolicy` objects are
  enforced at L3/L4. Rules that cannot be honoured exactly (L7, FQDN, deny
  rules) are downgraded with a warning, so you can migrate first and rewrite
  later.

## Configuration

| Variable | Default | Purpose |
|---|---|---|
| `MACHINA_CNI_CLUSTER_CIDR` | `10.42.0.0/16` | IPv4 pod range |
| `MACHINA_CNI_CLUSTER_CIDR6` | unset | Set to enable dual-stack |
| `MACHINA_CNI_LB_MODE` | `snat` | `dsr` for direct server return |
| `MACHINA_CNI_XDP` | off | NodePort at XDP |
| `MACHINA_CNI_CILIUM_POLICIES` | off | Enforce Cilium policy CRDs |
| `MACHINA_CNI_MTU` | auto | Pod MTU |
| `MACHINA_CNI_TAKEOVER` | off | Replace an already configured CNI |

Inspect the state with `GET /api/v1/bpf/cni` or the **Service LB** tab.
