# Machina — Product Guide

*Part of the [Machina Handbook](README.md) · see also
[Admin & Configuration](admin-configuration.md) · [FAQ](faq.md) ·
[Troubleshooting](troubleshooting.md)*

---

## 1. What Machina is

Machina is an **enterprise Linux hypervisor management platform**. It is a
unified control plane for **VMs, networks, storage, snapshots, and day-two
operations** on bare-metal worker nodes, built directly on **libvirt / QEMU /
KVM**, with a native eBPF networking datapath (`machina-bpfd`) and a
self-service **Fleet Cloud** layer on the same controller. In the Zyvor stack it
owns the metal; Atlas (storage) and GuestKit (guest tooling) plug into it, and
its native eBPF stack replaces separate Cilium/Tetragon-style agents.

The problem it solves: libvirt operations are normally scattered across `virsh`
scripts, consoles need separate gateways, and there is no fleet observability.
Machina folds all of that into one daemon + one API + one UI:

| Problem | Machina answer |
|---------|----------------|
| libvirt ops scattered across `virsh` scripts | One dashboard + REST API for the full VM lifecycle |
| Consoles need separate gateways | Built-in noVNC, SPICE, serial and SSH proxies |
| No fleet observability | Prometheus, alerts, webhooks, PSI/cgroups, OTLP export |
| KubeVirt migration is manual | YAML bundles + documented migration paths |
| Vendor hypervisor lock-in | Open-source Rust daemon on your own metal |

---

## 2. Architecture & tech stack

Machina is a **two-layer platform**:

- **`machina-daemon`** — the single-host hypervisor manager and the original
  product. It owns the REST API at `/api/v1`, WebSocket streams at `/ws/v1`,
  console proxying (VNC/SPICE/serial/SSH), PAM/LDAP/OIDC auth with RBAC, and
  speaks directly to libvirt. Built with **Axum 0.8 / Tokio**. Serves on
  **:5092** (HTTPS by default).
- **`machina-controller`** — the optional multi-host enterprise control plane.
  It persists state in an embedded SQLite database (no external DB),
  distributes tasks over an in-memory or NATS bus, and runs the HA, DRS,
  reconcile and AI engines. Talks to hosts through `machina-agent` over gRPC/TLS.
  Serves on **:5093**.
- **`machina-agent`** — runs on each managed hypervisor, exposes a gRPC service
  (tonic), executes libvirt operations for the controller, and proxies console
  WebSockets. Listens on **:50051** (gRPC) and **:50052** (console).

### Workspace crates

| Crate | Binary | Role |
|-------|--------|------|
| `core` | — | Shared types: VM/host/config structs, libvirt helpers, XML builders, audit, fleet placement |
| `daemon` | `machina-daemon` | Single-host REST+WS API server, auth, console proxies |
| `controller` | `machina-controller` | Multi-host control plane: fleet, HA, DRS, AI engine, tasks |
| `agent` | `machina-agent` | Per-host gRPC agent executing libvirt ops |
| `spec` | — | Declarative VM/cluster spec types |
| `translate` | — | libvirt domain XML → internal type translation |
| `rvb` | — | Reverse-bridge helpers |
| `virt-image-build` | — | virt-builder / mkosi golden-image job runner |
| Packer / dockur scripts | `contrib/packer/` | Golden Forge: Linux Packer (`build-linux-image.sh`); Windows 10/11 via dockur/Podman (`build-windows-dockur.sh`, gated by `dockur_windows_allowed`) |
| `run-as-user-helper` | — | setuid helper for run-as-user impersonation |

### Web UI

React 19 + Vite 8 + TailwindCSS 4, with `@xterm/xterm` terminals, `novnc-core`
consoles, `recharts` charts, and `three` for 3D topology views. Design system is
**Liquid Glass** (macOS-Tahoe-inspired glass primitives in
`web/src/components/glass/`). In dev, Vite serves the UI on **:3000** and proxies
`/api` and `/ws` to `https://localhost:5092`.

---

## 3. Surfaces

Machina exposes three ways to drive it.

### 3.1 `machinactl` CLI

The install/manage/demo CLI at the repo root. It auto-escalates to root via
`sudo` and talks to the API at `${MACHINA_URL:-https://127.0.0.1:5092}`.

| Command | Purpose |
|---------|---------|
| `deps` | Install all system dependencies (libvirt, qemu-kvm, build tools, Rust, Node) |
| `build [--no-web]` | `cargo build --workspace --release` (+ web UI unless `--no-web`) |
| `test` | `cargo test --workspace` |
| `install` / `uninstall` | Install/remove Machina to/from the system (`make install`) |
| `deploy` | Full: deps → build → install → start → verify |
| `reinstall` | stop → build → install → start → verify |
| `upgrade` | `git pull --ff-only` then reinstall |
| `start` / `stop` / `restart` / `status` / `logs` | Manage the `machina-daemon` service |
| `backup now\|enable\|disable\|status\|logs` | VM backup + daily timer control |
| `verify` | Post-install smoke test (API, VMs, backup) |
| `health` | Deep health check (service, API, disk, libvirt, timers) — exit 0/1/2 |
| `doctor` | System readiness (KVM, systemd, required commands) |
| `tls` | Generate a self-signed TLS cert into `/etc/machina/ssl/` |
| `demo` | Interactive REST API demo |
| `audit verify [N]` | Verify the signed audit log (via API or local file) |
| `integrations` | Show KubeVirt/k8s/automation status |
| `bootstrap` | Controller: set the public URL and pinned-TLS join listener, publish the agent, wait until ready |
| `host list\|add\|drain\|undrain\|remove` | Fleet nodes; `host add USER@HOST… \| --file hosts.txt --parallel N --site --rack --tag --network --pool` joins many over SSH |
| `metal …` | Bare-metal provisioning: `discover`, `import`, `commission`, `power`, `allocate`, `deploy`, `image`, `profile`, `provision` (see [baremetal-provisioning.md](../baremetal-provisioning.md)) |
| `dist publish\|publish-metal\|status` | Publish the agent and the bare-metal ramdisk/ISOs for joining nodes |

There is also `scripts/platformctl` — a thin CLI over the **controller** REST API
(`${MACHINA_CONTROLLER_URL:-http://127.0.0.1:5093}`): `health`, `hosts`,
`host validate`, `vms`, `vm get/metrics/adopt`, `content`, `marketplace`,
`tasks`, `events-stream`.

### 3.2 REST API (`/api/v1`)

All REST routes are nested under `/api/v1`; WebSocket routes under `/ws/v1`.
Every `/api/v1` route passes through auth middleware (login/session routes
bypass internally). Highlights:

**VM lifecycle** (`routes/vms.rs`)

| Method & path | Action |
|---------------|--------|
| `GET /vms` · `POST /vms` · `POST /vms/stream` | List / create / create-with-progress |
| `GET /vms/{name}` · `DELETE /vms/{name}` · `GET /vms/{name}/xml` | Details / delete / domain XML |
| `POST /vms/{name}/start\|stop\|shutdown\|reboot\|pause\|resume` | Power ops |
| `POST /vms/{name}/clone` · `/rename` · `/autostart/{enabled}` | Clone / rename / autostart |
| `POST /vms/{name}/vcpus/{count}` · `/memory/{mb}` | Resize CPU / RAM |
| `POST /vms/{name}/disk/attach\|detach/{target}\|resize/{target}` | Disk hot-plug / resize |
| `POST /vms/{name}/nic/attach\|detach/{mac}` | NIC hot-plug |
| `POST /vms/{name}/migrate` (+ `/migrate/max-bandwidth`, `/max-downtime`) | Live migration |
| `POST /vms/{name}/block/commit\|pull\|job/abort` · `GET .../block/job` | Block jobs |
| CPU/mem tuning, vCPU pinning, scheduler | `.../cputune`, `/memtune`, `/scheduler`, `/vcpu/{n}/pin` |

**Storage** (`routes/storage.rs`): `GET /storage/pools`,
`POST /storage/pools/{name}/start\|stop\|refresh\|autostart/{enabled}`,
`GET|POST /storage/pools/{pool}/volumes`, `DELETE .../volumes/{vol}`.

**Networks** (`routes/networks.rs`): `GET|POST /networks`,
`DELETE /networks/{name}`, `POST /networks/{name}/start\|stop\|autostart/{enabled}`,
`GET|PUT /networks/{name}/xml`.

**Snapshots** (`routes/snapshots.rs`): `GET /snapshots`,
`GET|POST /vms/{vm}/snapshots`, `DELETE /vms/{vm}/snapshots/{snap}`,
`POST /vms/{vm}/snapshots/{snap}/revert`.

**Consoles** (`routes/console.rs`, `consolehub.rs`):
`GET /vms/console-info/{name}`, `GET /vms/{name}/viewer.vv`,
`GET /vms/{name}/consolehub/plan`, `GET|POST /vms/{name}/consolehub/sessions`,
`POST /consolehub/sessions/{id}/end`, `GET /vms/{name}/rdp-info`.

**Auth** (`auth.rs`): `POST /auth/login` · `/auth/logout`, `GET /auth/session`,
`GET /auth/providers`, `GET /auth/oidc/login` · `/auth/oidc/callback`,
`POST /ws-token`, `GET /admin/sessions` · `DELETE /admin/sessions/{id}`.

**Metrics & health** (`metrics.rs`, `prometheus.rs`, `health.rs`, `events.rs`):
`GET /metrics` · `/metrics/{name}` · `/metrics/history`, `GET /prometheus`
(scrape), `GET /events/stream` (SSE), `GET /health` · `/health/problems`,
`GET /node`, `GET /openapi.json`.

**Host** (`host_network.rs`, `system.rs`): host interfaces, bridges, routing,
port-forward, firewall, `GET /system/platform-info`, OS-user management, and
runtime auth config (`/system/auth/ldap-settings`, `/oidc-settings`,
`/saml-settings`).

**Fleet** (`fleet.rs`): `GET /fleet/status\|metrics\|alerts\|vms`,
`POST /fleet/placement\|create-vm`, `GET /fleet/prometheus`.

**Integrations** (mounted under `/api/v1`): `vessel` (local Podman/Docker),
`k8s`, `kubevirt`, `fluxvm`, `guestkit`, `zeus_firewall`, `automation`,
`backup`, `jobs`, `templates`, `guest_images`, `platform_controller`
(reverse-proxy to the controller).

> The full machine-readable contract is served at `GET /api/v1/openapi.json`
> and mirrored in the UI at `/api-docs`.

### 3.3 WebSocket streams (`/ws/v1`)

Authenticated with a single-use token minted by `POST /api/v1/ws-token`, passed
as `?token=`:

| Path | Stream |
|------|--------|
| `/ws/v1/watch` | Event / state change stream |
| `/ws/v1/console/{name}` | Serial console |
| `/ws/v1/vnc/{name}` | VNC |
| `/ws/v1/spice/{name}` | SPICE |
| `/ws/v1/rdp/{name}` | RDP |
| `/ws/v1/terminal/{session_id}` | Terminal / PTY |
| `/ws/v1/ssh/{host}` | SSH |
| `/ws/v1/k8s-kubevirt/{ns}/{name}/vnc\|console` | KubeVirt VNC / console |
| `/ws/v1/fluxvm-console/{name}` | FluxVM guest-agent shell (`?token=&cols=&rows=`) |
| `/ws/v1/vessel/containers/{id}/stats\|logs` | Container CPU/mem/network stats or log stream |
| `/ws/v1/platform/vnc\|serial\|spice/{vm_id}` | Platform-proxied consoles |

### 3.4 Web UI

Two shells share one daemon:

- **Classic (daemon-backed)** — `/`, `/vms`, `/create`, `/import`, `/fleet`,
  `/storage`, `/disk-images`, `/snapshots`, `/backups`, `/networks`,
  `/nwfilters`, `/host-networking`, `/secrets`, `/containers` (+ `/containers/pods`
  for Podman pods), `/k8s` (+ `/k8s/workloads`, `/k8s/kata`), `/host-ssh`,
  `/settings`, `/api-docs`, plus the Fleet Cloud
  pages under `/fleet-cloud/*`.
- **Platform (controller-backed)** — `/platform/*` with its own shell:
  Mission Control, Virtual Machines, Hosts, Applications, Launchpad, Datacenter
  View, Disk Utility, Storage Tiers, Networks, Content, Templates, Cloud-Init
  Studio, Migration Assistant, Blueprints, Zyra AI, Security Operations, Policy,
  Webhooks, Backup & Restore, Fleet Snapshots, Disaster Recovery, High
  Availability, Observability, Topology, GPU Command Center, Users & Groups,
  API Keys, Integrations, Marketplace, and more.

  **Hosts** (`/platform/hosts`) is the fleet cockpit: a summary strip (machines online, VMs,
  average CPU, memory, how many need attention), a **Needs attention** list that only names
  machines that need a person (offline, failed validation, stale heartbeat, fenced, not
  schedulable) with a **Re-check** button, and three views of the same machines: **Cloud map**
  (default with two or more machines), **Cards** and **List**. Selecting a machine opens a side
  panel with its address, agent address, versions, the validation checklist with the fix for each
  failing check, and the actions: sync, enter or exit maintenance (with a "move VMs off" choice),
  fence or unfence (asks first) and remove (asks you to type the host name). **Add machine**
  opens the join wizard. The transport badge (mutual TLS / token) and agent version appear only
  when the controller reports them.

The login page uses PAM (Linux host accounts) by default; LDAP and OIDC SSO can
be enabled. See [Admin & Configuration → Authentication](admin-configuration.md#5-authentication--rbac).

---

## 4. Feature deep-dives

### VM lifecycle

Full create → run → migrate → delete lifecycle via API, UI, or `machinactl`.
Creation supports two backends, selected by `[libvirt].create_backend`
(default `virt_install`) or per-request `create_backend`: `virt_install`
(wraps `virt-install`) or `libvirt_xml` (native domain XML). Golden images can be
built asynchronously via `POST /jobs/virt-image-build` (virt-builder / mkosi).
Hot-plug of disks and NICs, vCPU/RAM resize, CPU pinning, block-job (commit/pull)
online disk operations, and live migration (with bandwidth/downtime tuning) are
all first-class. The GuestKit QEMU guest agent can be injected into every new VM
(`[libvirt].guest_agent_by_default = true`) for cloud-init seeding and offline
inject.

### Storage

libvirt storage pools and volumes: list pools, start/stop/refresh, toggle
autostart, and create/delete volumes per pool. Disk images are surfaced in the
UI at `/disk-images`; per-VM disk resize and IO tuning are available.

### Networks

libvirt virtual networks (create/delete/start/stop, autostart, edit XML), plus
host-level networking: interfaces, bridges, routing tables, LLDP, sysctl tuning,
port-forwarding and firewall rules (`routes/host_network.rs`). Network filters
(nwfilter) are managed at `/nwfilters`. Autostart networks are bootstrapped at
daemon boot.

### VM network policy

VM-to-VM segmentation in the CiliumNetworkPolicy schema, enforced by
`machina-bpfd` on each VM tap (observe by default, enforce under a lease).
On top of L3/L4, DNS names and L7 rules: flow history with a service map,
learn mode and replay, L7 metrics, lateral-movement alerts, DNS threat feeds,
quarantine, just-in-time access, plain-English drafts with approval,
Fleet Cloud project isolation with egress allowlists and egress IPs, and
sealed segmentation evidence for audits. UI: VM Network Policies; CLI:
`machinactl [--fleet] netpol|flow`. Details:
[docs/ebpf/vm-network-policy.md](../ebpf/vm-network-policy.md).

### Snapshots & backup

Per-VM snapshots (create/list/revert/delete) plus a scheduled backup subsystem:
`scripts/backup.sh` driven by `machinactl backup` and the `machina-backup.timer`
systemd unit (daily 2 AM). Backups go to `[backup].backup_dir`
(`/var/lib/machina/backups`), optionally to an NFS target, with configurable
retention and optional disk inclusion.

### Consoles

Built-in console proxies remove the need for a separate gateway: **noVNC**
(`/novnc` static client + `/ws/v1/vnc/{name}`), **SPICE HTML5** (`/spice-html5`),
serial (`/ws/v1/console/{name}`), and **native_ssh** (in-browser PTY terminal).
Windows guests are auto-detected and get **native RDP** via a hypervisor NAT
port-forward to the guest's port 3389 — connect with Microsoft Remote Desktop
or `mstsc`, or download the generated `.rdp` file — advertised only once the
agent confirms Remote Desktop is listening in the guest. ConsoleHub brokers
sessions with TTLs and can be gated behind OIDC.

### Fleet, HA & the platform layer

The classic daemon has a lightweight peer-based fleet view
(`[fleet]` config → `/fleet/*` API). For full multi-host operation the
**controller** adds HA failover, DRS (distributed resource scheduling),
desired-state reconciliation, a task bus (in-memory or NATS), and the AI
engine behind Zyra AI. See [../controller-ha.md](../controller-ha.md),
[../daemon-peer-fleet.md](../daemon-peer-fleet.md) and the
[operator runbook](runbook.md).

### Bare-metal provisioning

Machines with no OS yet go from a BMC address to a joined host without hand-copied media:
discover or import BMCs (Redfish, IPMI), commission them over PXE or BMC virtual media, keep
a pool by zone, write a checksum-verified OS image (Ubuntu 26.04/24.04/22.04, Debian 12/13,
Rocky 9, AlmaLinux 9) to the smallest suitable disk, and let the machine join on first boot
over the controller's pinned TLS listener with its site, tags, networks and storage pools.
A profile names what a machine of a kind gets (`machinactl metal provision --profile web
--count 8`) and can keep N deployed. BMC calls are idempotent, read the real power state, set
the next boot device, and include a Redfish read model (health, sensors, firmware, log) and
an IPMI serial-over-LAN console. Guide:
[../baremetal-provisioning.md](../baremetal-provisioning.md); what has actually run:
[../claims.md](../claims.md) C49 to C51.

### Integrations

- **Vessel** — local Podman or Docker on the hypervisor host. List/create
  containers, Podman pod CRUD, lifecycle, and WebSocket stats/logs via
  `/api/v1/vessel/*`. Enabled by default (`[vessel]`); fails soft when no
  engine socket is present. Kubernetes pods remain on `/k8s/workloads`.
- **KubeVirt** — export a VM as a KubeVirt YAML bundle
  (`GET /vms/{name}/kubevirt-bundle`) and apply/upload/start it on a cluster.
  See [../kubevirt-migration.md](../kubevirt-migration.md).
- **FluxVM** (`[fluxvm]`) — VMs on a FluxVM host (QEMU, Cloud Hypervisor,
  Firecracker, flux-vm) appear next to libvirt VMs with `backend: "fluxvm"`.
  Create (image or direct kernel boot, guest agent, shared disk), power,
  serial console (read-only log on non-QEMU engines), guest-agent console,
  snapshots, backups (every engine; a running VM only on QEMU), hot-add
  vCPU/memory (QEMU, Cloud Hypervisor), extra NICs (QEMU), install ISOs
  (attached at create, ejected live; QEMU) and live migration (QEMU on a shared
  disk, ISOs ejected) from VM detail → **Manage**. With `MACHINA_FLUXVM_URL` on each
  host's `machina-agent`, the controller lists them, powers and deletes
  them, migrates them between hosts (`vm.migrate`, DRS) and re-creates them
  on another host for HA (`ha.recover`, `POST /vms/{id}/fluxvm/recover`).
  Fleet migration and HA re-create need QEMU on a shared disk. See
  [../fluxvm.md](../fluxvm.md).
- **Fleet Cloud** (`/fleet-cloud/*` in the UI) — manage instances, flavors,
  networks, images, security groups, stacks, keypairs, and load balancers
  (kernel-level weighted round-robin, not an Octavia amphora) entirely via
  Machina's own native controller APIs — no external cloud connection, no
  credentials to wire up. The legacy external-cloud client integration
  (compute/image/network/identity management, and pushing local VMs to an
  external cloud) has been fully removed.
- **hyper2kvm / GuestKit** — VM migration into KVM and offline assurance.
- **Observability** — Prometheus scrape, remote-write ingest, OTLP/HTTP export
  to Grafana Alloy / OpenTelemetry Collector, Linux audit integration. See
  [../guides/observability.md](../guides/observability.md).

---

*Next: [Administration & Configuration](admin-configuration.md)*
