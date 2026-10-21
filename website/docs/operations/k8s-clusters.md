---
sidebar_position: 10
title: Fleet Kubernetes clusters (k3s)
description: Create, scale, upgrade and delete k3s clusters across enrolled hosts from the controller.
---

# Fleet Kubernetes clusters (k3s)

The controller can build a k3s cluster out of enrolled hosts: one **server** node and any number of **agent** nodes, each on its own
host. It asks each host's agent to run the k3s installer, keeps the join token and admin kubeconfig encrypted, and records progress so
you can poll it. This is separate from the daemon's single-host [cluster bootstrap](../getting-started/quickstart.md).

Because it runs root installers on your hosts, it is **off twice over**:

| Where | Setting | Effect |
|---|---|---|
| Controller | `MACHINA_K3S_FLEET=1` | Turns on `/api/v1/k8s-clusters`. Without it every call returns `k8s_fleet_disabled`. |
| Each host | `MACHINA_AGENT_K3S_ENABLED=1` in the agent environment | The agent only obeys k3s requests on hosts that opted in. |

Only an `admin` can create, change or delete clusters or read a kubeconfig; `operator` can list and view. Every change is audited.

## Use it

```bash
# Create: a server on one host, agents on two more (versions are pinned; there is no floating channel)
curl -sk -X POST https://CTL:5093/api/v1/k8s-clusters -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"name":"dev","version":"v1.31.4+k3s1","server_host_id":"<uuid>","agent_host_ids":["<uuid>","<uuid>"]}'

curl -sk -H "Authorization: Bearer $TOKEN" https://CTL:5093/api/v1/k8s-clusters/<id>   # poll until status is "ready"
curl -sk -H "Authorization: Bearer $TOKEN" https://CTL:5093/api/v1/k8s-clusters/<id>/kubeconfig > dev.yaml

# Scale out / in, upgrade, delete
curl -sk -X POST   .../k8s-clusters/<id>/nodes   -d '{"host_id":"<uuid>"}'
curl -sk -X DELETE .../k8s-clusters/<id>/nodes/<node-id>
curl -sk -X POST   .../k8s-clusters/<id>/upgrade -d '{"version":"v1.31.5+k3s1"}'
curl -sk -X DELETE .../k8s-clusters/<id>
```

`upgrade` takes `"skip_drain": true` and node removal takes `?force=true` to skip the drain.

Cluster status is `provisioning`, `ready`, `upgrading`, `deleting` or `error` (with the reason in `message`). Node status is `pending`,
`ready`, `removing` or `error`. Upgrades go servers first (the bootstrap server leading), then agents, one node at a time, and stop at the first failure. Each node
is **drained** (`kubectl drain --ignore-daemonsets --delete-emptydir-data`, run on another ready server) before it is upgraded and
**uncordoned** after; a drain that fails stops the upgrade before that node is touched. A single-server cluster cannot drain its
own server, so that node is upgraded in place. Removing an agent drains it first and deletes its Node object afterwards. There is no
`--force`: a pod no controller manages blocks the drain instead of being deleted.

A control plane is **one server** (SQLite) or an **HA control plane of 3 or 5 servers** with embedded etcd: pass `server_host_ids`
(the first bootstraps with `--cluster-init`, the others join it) instead of `server_host_id`. An even number is refused because etcd
needs an odd quorum.

The server host needs an `address` recorded (the API URL is `https://<address>:6443`, and the address is added as a TLS SAN).
Pass `install_url` to use an internal mirror of the installer script on an [air-gapped site](./air-gapped.md).

## Limits

- **Control-plane size is fixed at creation** (1, 3 or 5 servers). Servers cannot be added or removed afterwards; delete and recreate.
- Work runs in the controller process. A controller restart during an operation leaves the cluster in `provisioning`, `upgrading` or
  `deleting`: delete it (or retry) once the hosts are checked.
- No rollback and no CNI choice (k3s defaults). Draining waits for pod disruption budgets; a stuck drain fails after 10 minutes.
- Hosts need `curl`, `sh` and systemd, and reach the installer and the k3s release downloads (or your mirror).
- Not yet run against real hosts: see claims C61.
