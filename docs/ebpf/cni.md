# machina-cni: Kubernetes networking

Back to [native eBPF overview](README.md).

`bpf/machina-cni` is an **opt-in** Kubernetes CNI that replaces flannel,
kube-proxy and Cilium on clusters where you choose it. Clusters keep their
default CNI unless you opt in. One binary plays two roles:

- **CNI plugin** (when `CNI_COMMAND` is set): creates the pod veth (`eth0` in
  the pod, `mcXXXXXXXXXXXX` on the host), assigns a pod `/32` with link-local
  gateway `169.254.1.1`, and runs file-based IPAM under the node's podCIDR.
- **`machina-cni agent`** (`contrib/machina-cni.service`): polls the API server
  with `kubectl get … -o json`, installs the plugin and `05-machina.conflist`
  into the standard and k3s CNI directories, adds direct routes (proto 233) to
  the other nodes' podCIDRs, an nft masquerade table `machina_cni`, and
  `/etc/sysctl.d/99-zzz-machina-cni.conf` (`mc*` veths need `rp_filter=0` and
  `accept_local=1` for NodePort replies).

The agent compiles NetworkPolicies and Services into one `CniSync` message for
bpfd, which owns the `CNI_*` maps. `CniState.version` must equal
`CNI_ABI_VERSION` or bpfd rejects the sync, so upgrade bpfd and the agent
together.

## Opting in

By default the daemon's cluster bootstrap (`POST /api/v1/k8s/cluster-bootstrap`,
Kubernetes page → Cluster bootstrap) installs k3s with its bundled networking:
flannel, the NetworkPolicy controller and kube-proxy (only traefik and
servicelb are disabled). `machina-cni` is not enabled.

To use machina-cni instead, pass `"cni": "machina"` (or tick **Use machina-cni**
in the UI). k3s is then installed with
`--flannel-backend=none --disable-network-policy --disable-kube-proxy --disable=traefik --disable=servicelb`
and the `cni` phase (alias `cilium`, kept for old clients) enables
`machina-bpfd` and `machina-cni`.

```bash
curl -sk -b cookies -H 'Content-Type: application/json' \
  -d '{"phase":"full","cni":"machina"}' https://HOST:5092/api/v1/k8s/cluster-bootstrap
```

On an existing cluster (k3s or any Kubernetes), install the cluster without
its bundled CNI first, then `systemctl enable --now machina-bpfd machina-cni`.

**Takeover guard.** The agent never replaces a CNI that is already configured.
If any other `*.conf` / `*.conflist` / `*.json` exists in the CNI config
directories (flannel, Calico, Cilium, …) it writes nothing and exits with
status 78; the unit's `RestartPreventExitStatus=78` stops the restart loop.
The bootstrap's `cni` phase runs the same check and fails with the file list.
Set `MACHINA_CNI_TAKEOVER=1` in `/etc/default/machina-cni` only to replace the
existing CNI deliberately.

## Policy

- **NetworkPolicy**: label identities, `ipBlock` CIDRs, ports. Named ports
  resolve against the destination pod's container ports.
- **Cilium policies** (opt-in, `MACHINA_CNI_CILIUM_POLICIES=1`): also enforces
  `cilium.io/v2` CiliumNetworkPolicy and CiliumClusterwideNetworkPolicy —
  endpoint / CIDR / entity peers, `toPorts`, `enableDefaultDeny`. L7 rules
  enforce at L4; `toFQDNs` / `toServices` fail open on their ports; `*Deny`
  rules and host policies are ignored. Every downgrade logs an agent warning.
  This exists to migrate Cilium clusters without rewriting policy first.

## Services

| Type | Datapath |
|---|---|
| ClusterIP, externalIP, LoadBalancer | cgroup socket-LB (connect-time rewrite; clients keyed by netns cookie) |
| NodePort | tc on the uplink, or XDP DNAT to local backends with `MACHINA_CNI_XDP=1` |

- Dual-stack: `clusterIPs`, IPv6 EndpointSlices, one NodePort frontend per
  `ipFamilies` entry. Map keys are 16 bytes (IPv4-mapped).
- Multi-backend services use a Maglev table (M = 1021).
- `sessionAffinity: ClientIP` honours `timeoutSeconds`.
- Remote backends (`externalTrafficPolicy: Cluster`): `MACHINA_CNI_LB_MODE=snat`
  (default) or `dsr` (IPIP, NodePort in the outer IP ID; IPv4 only).

Inspect with `GET /api/v1/bpf/cni` and `GET /api/v1/bpf/cni/services`, or the
**Service LB** tab on Platform → Security → Native eBPF.

## Environment

| Variable | Default | Meaning |
|---|---|---|
| `NODE_NAME` | hostname | Kubernetes node this agent serves |
| `MACHINA_CNI_CLUSTER_CIDR` | `10.42.0.0/16` | IPv4 pod CIDR |
| `MACHINA_CNI_CLUSTER_CIDR6` | unset | Set to enable dual-stack (pods get a `/128`, gateway `fe80::1`; nft table becomes `inet`) |
| `MACHINA_CNI_CONF_DIRS` | standard + k3s | Where to install the conflist |
| `MACHINA_CNI_BIN_DIRS` | standard + k3s | Where to install the plugin binary |
| `MACHINA_CNI_MTU` | auto | Pod veth MTU |
| `MACHINA_CNI_INTERVAL_SECS` | — | API poll interval |
| `MACHINA_CNI_KUBECTL` | `kubectl` | kubectl binary |
| `MACHINA_CNI_CILIUM_POLICIES` | off | Also enforce Cilium policy CRDs |
| `MACHINA_CNI_LB_MODE` | `snat` | `snat` or `dsr` for remote NodePort backends |
| `MACHINA_CNI_XDP` | off | NodePort to local backends at XDP |
| `MACHINA_CNI_TAKEOVER` | off | Start even when another CNI config is present (replaces it) |

## Test

```bash
sudo ./scripts/bpf/cni-smoke.sh ./target/release    # make bpf-cni-test
```

Runs the real plugin against two netns pods, a test veth uplink and a private
bpfd; safe on hosts that already run another CNI. NodePort is probed with raw
SYNs because cgroup socket-LB hooks see every netns.
