<!-- Copyright (c) 2026 ZyvorAI Labs Private Limited. All rights reserved. -->
# Atlas Storage Integration

Machina's control plane integrates with **[Atlas](https://zyvor.dev)** — the Zyvor
storage control plane — to provision VM disks as backend volumes (Ceph RBD / NFS /
ZFS) and to route snapshot / backup / restore through Atlas. Products request
*intent* ("give me production block storage"); Atlas owns the backends, inventory,
ownership bindings and audit, keeping machina decoupled from Ceph.

```
machina-controller ──REST──► Atlas gateway ──► StorageDriver (Ceph RBD / NFS / ZFS)
      │                        (:5110)              │
      └─ provision volume · snapshot · backup       └─ k8s CSI (PVC / VolumeSnapshot) + rbd CLI
```

## Enabling

Set on the controller (env / systemd `EnvironmentFile`):

| Variable | Default | Purpose |
|---|---|---|
| `ATLAS_ENABLED` | `0` | Enable the integration (surfaces Platform → **Storage (Atlas)** + `/api/v1/atlas/*`) |
| `ATLAS_BASE_URL` | `http://127.0.0.1:5110` | Atlas gateway URL |
| `ATLAS_TOKEN` | — | Service-account JWT (Atlas `POST /auth/tokens`); sent when Atlas runs with `ATLAS_AUTH_REQUIRED=1` |
| `ATLAS_INSECURE_TLS` | `0` | Accept a self-signed Atlas cert |
| `ATLAS_TENANT_ID` | `machina` | Tenant recorded on machina's volumes |
| `ATLAS_DEFAULT_POLICY` | `general` | Intent → placement policy for VM root disks |
| `ATLAS_BACKUP_BUCKET_ID` | — | Default bound RGW bucket for VM backups |
| `ATLAS_RBD_MON_HOSTS` | — | Comma `host:port` Ceph monitors (for RBD disk attach) |
| `ATLAS_RBD_AUTH_USER` | — | cephx user (libvirt `<auth username>`) |
| `ATLAS_RBD_SECRET_UUID` | — | UUID of a libvirt `ceph` secret holding the key |

`ATLAS_RBD_*` are cluster-wide Ceph connection params — Atlas supplies the per-volume
`pool/image`; these supply the monitors + cephx secret needed to attach an Atlas RBD
volume as a libvirt network disk. Empty ⇒ rely on the hypervisor's `ceph.conf`/keyring.

## REST surface (`/api/v1/atlas/*`, controller)

Thin proxy over the Atlas gateway, plus VM-oriented orchestration:

- `GET  /atlas/status` — reachability + version probe
- `GET  /atlas/backends` · `/pools` · `/clusters` · `/policies` · `/metrics/summary`
- `GET|POST /atlas/volumes` · `GET|DELETE /atlas/volumes/{id}` · `POST /atlas/volumes/{id}/expand`
- `POST /atlas/volumes/{id}/snapshots` · `GET /atlas/snapshots`
- `POST /atlas/snapshots/{id}/clone` · `/restore` · `DELETE /atlas/snapshots/{id}[?force]`
- `GET|POST /atlas/buckets` · `GET|POST /atlas/backups` · `DELETE /atlas/backups/{id}` · `POST /atlas/restore-jobs`
- `GET /atlas/forecast[?minutes=1440]` — capacity projection (`days_to_full`, `growth_bytes_per_day`)
- `GET /atlas/alerts[?state=open]` — alerts from Atlas' monitor worker
- `GET /atlas/jobs` · `GET /atlas/jobs/{id}`
- `GET|POST /atlas/vms/{id}/volumes` · `POST /atlas/vms/{id}/snapshot` · `POST /atlas/vms/{id}/backup`

### Prometheus gauges

With `ATLAS_ENABLED=1`, `GET /api/v1/metrics/prometheus` also carries `machina_atlas_up`,
`machina_atlas_capacity_raw_bytes`, `machina_atlas_capacity_used_bytes`,
`machina_atlas_growth_bytes_per_day`, `machina_atlas_days_to_full` (absent while capacity is
not growing) and `machina_atlas_alerts_open`. The Atlas probe is bounded to 4 s, so a dead
gateway shows `machina_atlas_up 0` instead of stalling the scrape. A decision engine such as
Zyntra (`packs/virtualization` in the zyntra repo) reads storage headroom from this one scrape.
Unit-tested only (not yet scraped against a live Atlas).

Errors propagate the upstream Atlas status (4xx pass through; Atlas 5xx → 502); an
unreachable gateway is a typed `503 atlas_unavailable`.

## VM disks on Atlas (Ceph RBD)

Pass `atlas_root_disk: true` (+ optional `atlas_policy`) to `POST /api/v1/vms`. The
controller then:

1. provisions an Atlas volume bound to the VM (owner `machina/virtual_machine/<id>/root_disk`);
2. resolves its backend RBD `pool/image` and rewrites the VM spec's storage `source`
   to `rbd:<pool>/<image>?mon=…&auth=…&secret=…`;
3. `vm.apply` builds the libvirt domain with a `<disk type='network' protocol='rbd'>`
   (cephx `<auth>` + `<secret>`), so the VM **boots off the Atlas Ceph volume**.

Deleting the VM (`POST /api/v1/vms/{id}/delete`) cascades: the bound Atlas volume →
PVC → Ceph RBD image are removed.

**Reverse routing** — for an Atlas-backed VM, machina's normal snapshot/backup/restore
flows detect the binding (`vm_atlas_volumes` table, migration `008`) and route through
Atlas instead of a local libvirt snapshot / qcow2 copy.

## Implementation

| Area | File |
|---|---|
| REST client + DTOs | `controller/src/engine/atlas_bridge.rs` |
| VM orchestration + binding table | `controller/src/engine/atlas_vm.rs`, `controller/migrations/008_atlas_volumes.sql` |
| API handlers | `controller/src/api/atlas.rs` (routes in `api/mod.rs`) |
| VM-create wiring | `controller/src/api/vms/mod.rs` (`atlas_root_disk`) |
| Snapshot/backup/restore/delete routing | `controller/src/tasks/worker.rs` |
| RBD domain XML | `translate/src/domain_xml.rs`, `core/src/libvirt/create.rs` |
| Web console | `web/src/pages/platform/PlatformAtlasStorage.tsx`, `web/src/api/platformAtlas.ts` (`/platform/storage-atlas`) |

## Verified end-to-end

Against a real Rook-Ceph cluster (k3s), driven through machina's own API: provision /
snapshot / clone / restore / backup / delete of Ceph RBD volumes, RGW bucket + backup
(`rbd export-diff` → S3), and a VM booted off an Atlas-provisioned RBD volume
(`vda → rbd-nvme-prod/csi-vol-…`, cephx). Set `ATLAS_CEPH_DRIVER_MODE=real` and
`KUBECONFIG` on the Atlas gateway for the real driver.
