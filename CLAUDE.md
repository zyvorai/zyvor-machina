# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

New to the project? Start with [docs/ENGINEERING_ONBOARDING.md](docs/ENGINEERING_ONBOARDING.md) — a day-by-day plan from repo access to a verified first deploy.

---

## Build & Development Commands

**Do not build Rust locally on macOS** — the workspace depends on Linux libvirt headers. Build on a Linux host or via remote deploy.

### Rust workspace (Linux only)
```bash
make build          # debug build (all workspace members)
make release        # optimized release build
make test           # run all Rust tests (cargo test --workspace)
make lint           # clippy with -D warnings
make fmt            # rustfmt all crates
make fmt-check      # check formatting (CI gate)
cargo test -p machina-controller   # single crate
cargo test -p machina-daemon       # single crate
```

### Web frontend (works on macOS)
```bash
cd web && npm run dev       # hot-reload dev server on port 3000
cd web && npm run build     # typecheck + production bundle (web/dist/)
cd web && npm test          # vitest unit tests
cd web && npm run test:e2e  # Playwright smoke tests
```

### Live regression (CDP page sweep + API; needs deployed host)
```bash
make regression-setup
export MACHINA_BASE_URL=https://HOST:5092 MACHINA_USER=sus MACHINA_PASS=max
./scripts/regression/chrome-launch.sh &   # CDP :9222
make regression-api LOOPS=1               # no Chrome needed
make regression-ops                       # power / screenshot / volumes
make regression-mission                   # fleet activity / reports / Atlas
make regression-catalog                   # guest-health / network CRUD / HA
make regression-pages LOOPS=1             # needs CDP
# continuous: cd scripts/regression && node page-sweep.js --forever
# see scripts/regression/README.md
```

### Deploy & run
```bash
./machinactl deploy          # build + install + start (recommended)
./machinactl reinstall       # rebuild + reinstall + verify
make && sudo make deploy     # manual equivalent
./scripts/deploy-remote.sh user@host --remote-build   # build on remote Linux host
```

### Dev mode (no install, Linux only)
```bash
./target/debug/machina-daemon    # daemon on :5092
cd web && npm run dev             # UI dev server on :3000, proxied to :5092
```

---

## Architecture Overview

This is a **two-layer platform**: a hypervisor daemon layer and an enterprise control plane.

### Process diagram
```
Web UI (React, :3000 dev / :5092 prod)
    │
    ├─► machina-daemon (:5092)      ← REST + WebSocket, libvirt, PAM auth
    │       │
    │       └─► libvirt / QEMU/KVM (same host)
    │
    └─► machina-controller (:5093)  ← Fleet control plane, embedded SQLite, NATS
            │
            └─► machina-agent (:50051 gRPC)  ← per-host gRPC agent
                    │
                    └─► libvirt / QEMU/KVM (remote host)
```

**`machina-daemon`** is the single-host hypervisor manager (the original product). It owns the REST API at `/api/v1`, handles VNC/SPICE/serial/SSH console proxying, PAM+RBAC auth, and speaks directly to libvirt.

**`machina-controller`** is the multi-host enterprise control plane. It persists state in embedded SQLite (via SQLx), distributes tasks via an in-memory or NATS bus, and communicates with hypervisor hosts through `machina-agent` over gRPC (TLS).

**`machina-agent`** runs on each managed hypervisor. It exposes a gRPC service (`spec/` proto), executes libvirt operations, and provides console WebSocket proxying.

The **web UI** proxies all `/api/...` and `/ws/...` requests to `machina-daemon` in development (see `web/vite.config.ts`). Platform (controller) calls go through `/api/v1/platform/controller` on the daemon, which reverse-proxies to the controller at `:5093`.

---

## Workspace Crates

| Crate | Binary | Role |
|-------|--------|------|
| `core` | — | Shared types: VM/host/config structs, libvirt helpers, XML builders, audit, fleet placement |
| `daemon` | `machina-daemon` | Single-host REST+WS API server (Axum/Tokio), auth, console proxies |
| `controller` | `machina-controller` | Multi-host control plane: fleet, HA, DRS, AI engine, SQLite embedded, NATS tasks |
| `agent` | `machina-agent` | Per-host gRPC agent (tonic) executing libvirt ops for the controller |
| `spec` | — | Declarative VM/cluster spec types (Serde structs mirroring the gRPC proto) |
| `translate` | — | libvirt domain XML → internal type translation, QEMU command helpers |
| `rvb` | — | RVB (reverse bridge) helpers |
| `virt-image-build` | — | virt-builder / Packer golden image job runner |
| `vessel-core` | — | Podman/Docker container engine (bollard + Podman libpod API) |

---

## Controller Architecture

`controller/src/` layout:

- **`api/`** — Axum route handlers; one file per feature area (e.g. `api/vms.rs`, `api/fleet.rs`, `api/ai.rs`). All routes are assembled in `api/mod.rs`.
- **`engine/`** — Background engine modules: `ha.rs` (HA failover), `drs.rs` (distributed resource scheduling), `reconcile.rs` (desired-state reconciliation), `scheduler.rs`, `webhook_worker.rs`. The AI sub-engine lives in `engine/ai/` with dozens of specialized modules (`llm.rs`, `agents.rs`, `actions.rs`, `providers.rs`, etc.).
- **`db/`** — database layer for both backends (SQLite default, PostgreSQL build): `Db`/`DbPool` aliases, `query*` wrappers, `dialect.rs` (SQL rewriting for PostgreSQL), `begin_write`, `testing.rs`, migrations (`controller/migrations/` and `controller/migrations_pg/`), bootstrap. See "Database backends" below. `scripts/db/machina-db.sh` (`machinactl db ...`) sets up, inspects and backs up the database (managed Postgres pod, distribution packages, external server); test it with `--sandbox DIR`, never with env vars under sudo.
- **`tasks/`** — Async task bus abstraction: `InMemoryTaskBus` + optional `NatsTaskBus`; `worker.rs` processes tasks; `nats_subscriber.rs` bridges NATS → local bus.
- **`state.rs`** — `AppState` holds config, DB pool, task bus, and agent client.
- **`agent_client.rs`** — gRPC client to `machina-agent`.
- **`consolehub.rs`** — Console session management (native VNC/SPICE/serial proxies).
- **`auth.rs`**, **`jwt.rs`** — JWT-based auth for the controller (separate from daemon PAM auth).
- **`sync.rs`** — Periodic sync loops (KubeVirt inventory, storage, etc.).

### Database backends

The controller builds for exactly one backend: `sqlite` (default feature) or `postgres` (`cargo build -p machina-controller --no-default-features --features postgres`, Linux host only). Guide: [docs/guides/database.md](docs/guides/database.md). Rules that keep both working:

- Never name `sqlx::Sqlite*`/`Pg*` types or call `sqlx::query*` outside `controller/src/db/`. Use `crate::db::{DbPool, DbConn, query, query_as, query_scalar}`; the wrappers pass SQLite SQL through and rewrite it for PostgreSQL (`db/dialect.rs`: `?` to `$n`, `CURRENT_TIMESTAMP`, `LIKE` to `ILIKE`, `CAST ... AS REAL/INTEGER`, `INSERT OR IGNORE`, `rowid` to `seq`).
- Write SQL both backends accept. SQLite date/JSON helpers (`datetime`, `strftime`, `julianday`, `hex`, `printf`, `json_extract`, `json_each`, two-argument `max`/`min`) exist as functions in the PostgreSQL schema. Use `TRUE`/`FALSE` for flag columns, `ON CONFLICT ... DO UPDATE` (qualify self-references: `table.col + 1`) instead of `INSERT OR REPLACE`, and no unqualified `rowid` outside `tasks`, `vm_restore_points`, `ha_events`.
- **Every schema change is written twice**: `controller/migrations/NNN_*.sql` (SQLite) and `controller/migrations_pg/NNN_*.sql` (PostgreSQL), same number. The PostgreSQL baseline `000_postgres_schema.sql` covers SQLite migrations 000-060 and is drafted by `scripts/db/sqlite_to_pg_schema.py`. Column types follow how Rust reads and binds the column: ids are `UUID`, flags `BOOLEAN` (add the name to `BOOL_NAMES` in the generator and use `TRUE`/`FALSE`), JSON and timestamps `TEXT`, integers `BIGINT`.
- No `u32`/`u64`/`usize` binds (PostgreSQL has no unsigned types): cast to `i64`. A machine in `metric_samples.subject` binds as `db::subject_id(id)`.
- Serialized check-then-write sections (address/quota allocation, tags, Elastic IPs) start with `crate::db::begin_write(&pool)` (SQLite `BEGIN IMMEDIATE`, PostgreSQL advisory lock), never `begin_with("BEGIN IMMEDIATE")`.
- `scripts/db/check_pg_sql.py` (CI job `controller-postgres`) PREPAREs every literal SQL statement against the PostgreSQL schema; run it after touching SQL (`PGURL=... python3 scripts/db/check_pg_sql.py`, schema loaded from every file in `controller/migrations_pg/`, in order: the baseline `000_postgres_schema.sql` plus the later migrations). SQLite tolerates bare columns next to `GROUP BY`, `SUM(boolean)`, unaliased subqueries and boolean-vs-integer comparisons; PostgreSQL does not. SQL built with `format!()` is not covered, so exercise those paths in a test.
- `scripts/ci/check-migrations.py` enforces the pairing: a SQLite migration numbered after the PostgreSQL baseline needs a `migrations_pg` file with the same number. Once a release ships the baseline it must not be edited (sqlx would refuse to start on "migration 0 was previously applied but has been modified"); change the schema with a new numbered migration.
- `machina-dbtool` (`dbtool/`) copies a SQLite database into PostgreSQL. A new column type or storage quirk in SQLite (a BLOB in a TEXT column, as metric subjects have) needs a rule in `dbtool/src/convert.rs`.
- Tests start from `crate::db::testing::pool()` (in-memory SQLite, or a clone of a migrated template database via `TEST_DATABASE_URL` on PostgreSQL). The PostgreSQL build uses a vendored, patched `sqlx-postgres` (`vendor/sqlx-postgres/MACHINA_PATCHES.md`); reapply the patch on sqlx upgrades.

### Controller config (env vars)
```
DATABASE_URL          sqlite:///var/lib/machina/controller.db   (embedded, no PostgreSQL needed; the PostgreSQL build takes postgres://user:pw@host/db)
NATS_URL              nats://127.0.0.1:4222   (optional, enables NATS task fan-out)
MACHINA_AGENT_ADDR    http://127.0.0.1:50051
MACHINA_JWT_SECRET    (random per-process secret if unset; controller refuses to boot on the well-known dev literal unless MACHINA_ALLOW_DEV_SECRETS=1)
MACHINA_PUBLIC_URL    http://127.0.0.1:5093
MACHINA_WEB_URL       http://127.0.0.1:5173
GUESTKIT_ENABLED      true
MACHINA_BPF_ENFORCE_LEASE_SECS 900
```

---

## Web Frontend Structure

`web/src/`:

- **`pages/`** — One file per route. Classic daemon-backed pages (e.g. `VMList.tsx`, `Dashboard.tsx`) live at the top level. Platform (controller) pages live in `pages/platform/` and are prefixed `Platform*`. Fleet Cloud pages live at `/fleet-cloud/*` and are prefixed `FleetCloud*` (e.g. `FleetCloudInstances.tsx`). The legacy external-cloud client integration this used to gate on has been fully removed — Fleet Cloud is entirely native now, backed by the controller's own APIs.
- **`components/`** — Shared UI components. Glass components (`GlassCard`, `GlassModal`) are in `components/glass/`. AI/Zeus components are in `components/ai/`. Platform shell components (`GlobalBar`, `MobileNavSheet`, `ChapterBar`) are in `components/nav/`; `components/consolehub/` holds the console UI (`CinemaShell`, etc.), not the shell chrome.
- **`api/`** — One TypeScript module per API domain. `client.ts` is the base fetch wrapper. `platform.ts` sets the controller proxy base URL. Files prefixed `platform*` call the controller; others call the daemon.
- **`contexts/`** — React contexts: `AuthContext`, `ThemeContext`, `WebSocketContext` (WS live updates), `AiContext`, `PlatformInfoContext`, `ToastContext`, `BreadcrumbNameContext` (per-page breadcrumb label override).
- **`hooks/`** — Custom hooks for keyboard shortcuts, fleet settings, console access policy, SSH, K8s context, etc.

### Route structure
- `/` and `/vms/*` — Classic daemon-backed hypervisor UI
- `/containers` and `/containers/pods` — Local Podman/Docker containers and Podman pods (Vessel)
- `/platform/*` — Enterprise platform shell (controller-backed); uses `PlatformLayout` (top-bar-only navigation, no sidebar)
- `/fleet-cloud/*` — "Fleet Cloud" UI (renamed from its old route; no redirect — old links 404). Backed entirely by Machina's own native controller APIs for every page: flavors, images/templates, instances/VMs, volumes, security groups, stacks, projects, server groups, floating IPs via port-forwards, keypairs, and load balancers (`controller/src/api/load_balancers.rs` + `controller/src/engine/load_balancer.rs` — a weighted round-robin iptables rule set pushed to the owning host's agent, no amphora VM). The legacy external-cloud client integration (compute/image/network/identity management, disk push/pull) has been fully removed from core and daemon.
- `/fleet` — Multi-host fleet overview
- `/platform/zyra/security/network-policies` — VM network policy (CiliumNetworkPolicy schema → `machina-bpfd` VM edge; compiler in `bpf/machina-bpf/src/netpol/`, daemon `routes/netpol.rs`, controller `api/vm_network_policies.rs` + `engine/vm_netpol.rs`; native toFQDNs, L7 (`netpol/l7.rs` + bpfd `server/vml7.rs`: the edge holds a request's first segment, bpfd allows it or answers 403/RST/REFUSED; terminatingTLS/originatingTLS and header rewrites go through bpfd's transparent rustls proxy `server/vmproxy.rs` via sk_assign on the inject veth), CiliumCIDRGroup/toGroups and authentication + source guard (cross-host: controller issues host certs from its CA, `bpf/machina-bpf/src/authca.rs`; bpfd `server/vmauth.rs` runs mTLS on 4250)) and the Flows terminal (`components/flow/FlowTerminal.tsx`). Flow history (map, learn, replay, L7 metrics, alerts), quarantine, JIT, threat feeds, plain-English drafts (`DraftPanel.tsx`), project isolation and egress (controller table `vm_netpol_projects`; bpfd applies egress IPs as nftables `ip machina_egress`; the controller sets `VmEdgeState.node_is_host` so every node address counts as `host`) and sealed segmentation evidence (`netpol/evidence.rs`, `GET …/vm-network-policies/evidence[?format=md]`). CLI: `machinactl [--fleet] netpol|flow|vm label|vm quarantine` (`scripts/lib/netpol-ctl.sh`). Tests: `scripts/bpf/vm-edge-smoke.sh` (veth/netns), `scripts/bpf/vm-netpol-realvm.sh` (disposable real VMs). See `docs/ebpf/vm-network-policy.md`.

### Dev proxy
In `npm run dev` mode, Vite proxies `/api` and `/ws` to `https://localhost:5092` (daemon). Platform API calls flow through `/api/v1/platform/controller` on the daemon, which reverse-proxies to the controller at `:5093`.

---

## Daemon Config

Config resolution order: `--config` CLI flag → `/etc/machina/config.toml` → `~/.machina/config.toml` → built-in defaults.

Key sections: `[daemon]` (host/port), `[libvirt]` (URI), `[auth]` (PAM service, OIDC, SAML, LDAP, run-as-user), `[tls]`, `[backup]`, `[fleet]` (peer list), `[vessel]` (Podman/Docker socket), `[metrics_history]`, `[observability.otlp]`, `[fluxvm]`.

**FluxVM backend** (`[fluxvm] enabled = true`, `base_url` default `http://127.0.0.1:7788`, `token`/`token_file`, `default_backend`): VMs from `../fluxvm`'s fluxvm-api are merged into `GET /api/v1/vms` (libvirt wins on a name clash) and carry `backend: "fluxvm"`. VM routes take `?backend=fluxvm` (and fall back to FluxVM when the name isn't a libvirt domain): get, create (`backend: "fluxvm"`, `fluxvm_backend`, `fluxvm_image`; network is eBPF-first like `../fabric`'s fluxvm-driver: `fluxvm_direct_uplink` (+`fluxvm_direct_mode` `l2-uplink`|`peer-veth`, `fluxvm_direct_guest_ips`) = bridge-less TC/eBPF redirect, else `fluxvm_bridge` = bridged tap, else a tap with `netns: true` and a generated MAC — all three get FluxVM's eBPF VM edge; `fluxvm_network: "user"` (QEMU only) / `"none"` are opt-ins without eBPF; a NoCloud seed is always sent so guest DHCP comes up), start/stop/shutdown/reboot/pause/resume, delete, metrics, snapshots (list/create/delete/revert → FluxVM `restore`), backups (`POST /api/v1/backups {vm_name, backend: "fluxvm", compress}`, list `?backend=fluxvm&vm=`, restore needs the VM stopped, delete `?backend=fluxvm`; any engine on default or shared storage, a running VM only on QEMU with default storage; restore converts back to the disk's format), hot-add vCPU/memory (`/vcpus/{n}`, `/memory/{mb}` and the live variants; add-only, a shrink is 400; the live size comes from the `fluxvm.dev/live-*` labels), extra NICs (`/nic/attach {network: <bridge>}` returns the generated `mac`, `/nic/detach/{mac}`; QEMU only; a netns VM gets each host-bridge tap as an inherited fd at hot-add and relaunch), live migration (`POST …/migrate {dest_uri: "local" | <fluxvm-api URL>, dest_token?, listen_host?, advertise_host?, bandwidth_mbps?, max_downtime_ms?}`: QEMU on `fluxvm_shared_disk` only, never after a hot-add until restarted, never with an ISO still in a drive; flow in `core/src/fluxvm/migrate.rs`), install media (`fluxvm_isos` on create, QEMU, at most 4 → CD-ROMs `install`, `cd2`…; shown as `cdrom` disks in details; `POST …/cdrom/eject/{drive}` ejects live, `…/cdrom/insert` is refused), the serial console WebSocket (interactive on QEMU, read-only `console.log` on the other engines) and the guest-agent shell `/ws/v1/fluxvm-console/{name}?token=&cols=&rows=` (`vms:write`; binary keystrokes, resize as JSON text). Create also takes `fluxvm_kernel`/`fluxvm_initrd`/`fluxvm_kernel_args`, `fluxvm_agent: false`, `fluxvm_shared_disk` (a raw file used in place under a `.fluxvm-lock`) and `fluxvm_isos`. Every other libvirt-only handler returns 400 via `spawn_libvirt_actor`. Client in `core/src/fluxvm/`, routes in `daemon/src/routes/fluxvm.rs` (`GET /api/v1/fluxvm/status`); `wss://` FluxVM works (`[fluxvm] insecure_tls` skips verification). The web UI carries FluxVM VMs as `libvirt_connection: 'fluxvm'`, and `appendVmConnection` turns that into `?backend=fluxvm`; VM detail has **Manage** (`FluxvmManagePanel.tsx`, gated per engine by `fluxvmCaps`) and **Agent console** tabs.

**FluxVM in the fleet**: `machina-agent` reads `MACHINA_FLUXVM_URL` / `MACHINA_FLUXVM_TOKEN` / `MACHINA_FLUXVM_INSECURE_TLS` (else the host's `[fluxvm]` config; `agent/src/fluxvm.rs`), adds FluxVM VMs to `ListVms` (`backend: "fluxvm"`, engine, storage, record JSON; `fluxvm_ok` says fluxvm-api answered), routes `ApplyVm`/`VmPower`/`DeleteVm` with `backend: "fluxvm"` to it, and exposes the migration RPCs (`ExportFluxvmRecord`, `PrepareFluxvmReceiver`, `StartFluxvmMigration`, `GetFluxvmMigrationStatus`, `FinishFluxvmMigration`, `AdoptFluxvmVm`, `AbortFluxvmMigration`). The controller keeps them as `vms.inventory_source = 'fluxvm'` rows (migration 068: `fluxvm_engine`, `fluxvm_storage`, `fluxvm_record_json`; pruned only when `fluxvm_ok`, never mid-migration) in `controller/src/engine/fluxvm_fleet.rs`: `vm.power`/`vm.delete` go through the agent, `vm.migrate` (and DRS, which enqueues it) runs receiver on the destination → start on the source → poll → finish → adopt (same host allowed), and `ha.recover` re-creates the VM from its last record on the target host with `shared_takeover` (shared-disk QEMU only; hot-add doesn't block it, ejected drives are dropped). The migration pre-check (`mobility_blocker`) also refuses extra hot-plug state and loaded install media. Discovered rows are unmanaged, so the reconciler leaves them alone; HA still needs an HA policy. `POST /api/v1/vms/{id}/fluxvm/recover {host_id?}` (operator) runs the same re-create on demand. Regression: `make regression-fluxvm` (`scripts/regression/ops-fluxvm.js`; `FLUXVM_ONLY=b` runs a subset). Guide: `docs/fluxvm.md`.

**Golden Forge** (`POST /api/v1/jobs/packer-golden-build`): Linux guest ids run `contrib/packer/build-linux-image.sh`. Windows `win10` / `win11` run `contrib/packer/build-windows-dockur.sh` (Podman + KVM) only when `[libvirt] dockur_windows_allowed = true`. See `contrib/packer/windows-dockur/README.md` and `docs/handbook/admin-configuration.md`.

Default port: **5092** (daemon), **5093** (controller), **50051** (agent gRPC).

---

## Docs graphics and animations

README and website graphics are generated, not drawn by hand. Cards: HTML in `docs/social/readme/*.html`, rendered to `docs/ux/readme-*.jpg` by `docs/social/readme/build.sh` (Chrome + `sips`, macOS). Animated deploy SVGs: `node docs/social/anim/gen.mjs` (CSS-only, no script); `docs/social/anim/build.sh` renders GIF fallbacks. Edit the source, rerun the build, commit both. The EC2 action tables in `docs/cloud-ec2-api.md` come from `python3 scripts/ec2/action_table.py` (`--check` fails when stale).

---

## Design System

The UI follows **apple.com / Zeus OS** contracts — see [docs/design/APPLE-UX-CONTRACT.md](docs/design/APPLE-UX-CONTRACT.md) and [docs/design/DAYLIGHT-CONTRACT.md](docs/design/DAYLIGHT-CONTRACT.md).

- **Shell:** `GlobalBar` (48px top bar: Z mark, Machine Finder link, six product groups with Netra mega-panel flyouts) + `MobileNavSheet` (≤1024px burger) + `ChapterBar` in `layouts/PlatformLayout.tsx`; the sidebar, icon rail, Mac menubar and dock are gone. Mission Control has the fleet pulse band and `FleetHero` SVG. Default theme: **Apple light** (`tahoe-light`). The current look lives in `web/src/styles/netra-look.css` (loaded last; namespace new tokens `--nl-*`). Audit UI changes with `node web/scripts/ux-audit.mjs` (see `docs/design/APPLE-UX-CONTRACT.md`).
- **Story / Browse / Work** tiers: `apple-story-stack`, `TahoeToolbar`, `.tahoe-glass-card`; VM detail leads with [`VmConsoleHeroPreview`](web/src/components/vm/VmConsoleHeroPreview.tsx).
- **Interactive blue:** apple.com `#0071e3` (CTAs, links, focus, active nav). Dark links `#2997ff`.
- **Box fonts:** Apple shop `.form-selector` 1:1 — `--text-primary/secondary/muted` (`#1d1d1f` / `#6e6e73` / `#86868b` light; `#f5f5f7` / `#a1a1a6` / `#86868b` dark) in `zeus-parity.css`.
- **Story type:** AirPods-scale `.apple-display` / `.apple-lede` (SF Pro Display).
- **Login:** [`PremiumLoginShell`](web/src/components/PremiumLoginShell.tsx) + [`zyvor-premium-login.css`](web/src/styles/zyvor-premium-login.css) — one centered composition, hero wordmark **machina**.
- Glass primitives remain in `web/src/components/glass/`. Framer Motion handles spring animations on modals/toasts.

Platform desktop has three density tiers (Normal / Power User / Advanced), switchable via Settings → Appearance. The `usePlatformDesktopTier` hook reads the current tier.

---

## Testing

- **Rust unit tests**: `cargo test --workspace` or `cargo test -p <crate>`
- **Web unit tests**: `cd web && npm test` (vitest)
- **Web E2E**: `cd web && npm run test:e2e` (Playwright; requires a running daemon)
- **API smoke test**: `./machinactl verify` or `VSPASS=… ./scripts/e2e-test.sh https://HOST:5092 USER`
- **Media / guest-tools features**: `./scripts/feature-test.sh HOST USER PASS` — ISO upload+download jobs, CD-ROM lifecycle, guest-agent channel, console plan. Default VM is `win10-msedge` (`os_hint=windows`); for Linux smoke VMs set `VM=chrome-e2e-vm` or `VM=iw-e2e-1` (`os_hint=linux`). CD-ROM auto-target must be unoccupied (virtio-root Linux often gets free SATA `sda`). Asserts each feature's failure mode too, not just the happy path.
- **GuestKit live matrix**: `./scripts/guestkit-live-matrix.sh` (suites A–F; `--with-offline` for G; `--case ID` to retest). Auth via SSH + `https://127.0.0.1:5092` on the host. Report: `/tmp/guestkit-matrix.md`.
- **Customer site readiness**: [docs/CUSTOMER_SITE_READINESS.md](docs/CUSTOMER_SITE_READINESS.md) — pilot gate, checklist, acceptance tests.
- **Live regression RESULTS**: [scripts/regression/RESULTS.md](scripts/regression/RESULTS.md)
- **30-step API demo**: `sudo ./scripts/demo.sh`

---

## Adding a New Platform Page

Three steps are always required:

**Step 1** — Create `web/src/pages/platform/PlatformFoo.tsx`.

**Step 2** — Lazy-import and register the route in `web/src/App.tsx`:
```tsx
const PlatformFoo = lazy(() => import('./pages/platform/PlatformFoo'))
// inside AuthenticatedShellRoutes:
<Route path="/platform/foo" element={<PlatformFoo />} />
```

**Step 3** — Add a nav entry and breadcrumb label in `web/src/utils/routes.ts`:
```ts
// in navGroups, under the appropriate section:
{ to: '/platform/foo', icon: <SomeIcon className="w-4 h-4" />, label: 'Foo' }

// in routeLabels:
'/platform/foo': 'Foo',
```

Classic daemon pages follow the same pattern but live in `web/src/pages/` (no `Platform` prefix) and use non-`/platform/*` routes.

---

## Web API Patterns

**Helper functions** (`web/src/api/client.ts`):
- `apiGet<T>(path)`, `apiPost<T>(path, body)`, `apiPut`, `apiPatch`, `apiDelete`
- `readJsonArray<T>(path)` — response is a JSON array
- `readJsonItemsList<T>(path)` — response is `{ items: T[] }`

**Daemon vs platform routing**:
- Daemon calls — `apiGet('/api/v1/vms')` (dev proxy routes to `:5092`)
- Platform/controller calls — prefix with `PLATFORM_CONTROLLER_PROXY = '/api/v1/platform/controller'` from `web/src/api/platform.ts`. Never call `:5093` directly.

**Error handling**: wrap thrown errors with `formatUserError(e)` (`web/src/utils/apiError.ts`) before passing to `toast.error()`. This extracts `error_code`/`remediation` from JSON error bodies and handles HTML proxy error pages gracefully.

**Toasts**: `const toast = useToastContext()` → `toast.success(msg)` / `toast.error(msg)` / `toast.warning(msg)`. Already wired in `App.tsx`.

---

## Controller Handler Pattern

All handlers live in `controller/src/api/<feature>.rs` and are registered in `controller/src/api/mod.rs`.

**Signature**:
```rust
pub async fn my_handler(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,  // omit if public
    Path(id): Path<Uuid>,                   // or Query(q): Query<MyQuery>
    Json(req): Json<MyRequest>,             // omit for GET/DELETE
) -> Result<Json<MyResponse>, ApiError> {
    // sqlx query, task dispatch, etc.
    Ok(Json(response))
}
```

**Error builders** (`controller/src/api/error.rs`):
```rust
ApiError::not_found("vm not found")
ApiError::bad_request("invalid field")
ApiError::conflict("already exists", "delete it first")
ApiError::internal("unexpected failure")
// chain: .with_code("my_code").with_remediation("try X")
```
`sqlx::Error::RowNotFound` auto-converts to 404; pg unique-constraint violation (23505) auto-converts to 409.

**Async tasks** — for long-running operations return a task instead of blocking:
```rust
let task = enqueue_task(&state, "vm.migrate", Some(vm_id), json!({"target": host_id})).await?;
Ok(Json(task))  // TaskResponse { task_id, status, operation }
```
Frontend polls `/api/v1/tasks/{task_id}`.

---

## Bare-metal provisioning (Metal as a Service)

Controller-side pipeline that takes a machine from a BMC address to a joined host. Guide:
[docs/baremetal-provisioning.md](docs/baremetal-provisioning.md); verification status:
`docs/claims.md` C49 to C51.

- **Modules** (`controller/src/engine/`): `baremetal.rs` (registry, `load_bmc`, power state,
  boot override, virtual-media helpers, BMC poll), `baremetal_bmc.rs` (drivers: Redfish
  client with retries/async tasks, IPMI via `ipmitool`, libvirt for labs, fake; the Redfish
  read model; virtual media), `baremetal_lifecycle.rs` (state machine, single-use
  commissioning token, inventory, auto-enrol), `baremetal_boot.rs` (dnsmasq proxyDHCP and
  iPXE script rendering, embeds `contrib/metal/deploy.sh`), `baremetal_images.rs` (verified
  image catalog), `baremetal_alloc.rs`, `baremetal_deploy.rs` (installer and image deploy),
  `baremetal_profiles.rs` (profiles, one-step provision, top-up loop), `baremetal_discover.rs`
  (BMC scan, bulk import), `host_post_join.rs` (networks and pools named on a token);
  `controller/src/baremetal_sol.rs` (serial-over-LAN WebSocket). Routes are in
  `api/baremetal.rs`, registered in `api/mod.rs`; boot/plan/report routes are public by design
  (token-gated) and also on `enrollment_router`.
- **Ramdisk and media**: `contrib/mkosi-defs/metal` + `scripts/metal/build-image.sh` (kernel,
  initrd, squashfs; Linux build host only), `scripts/metal/build-ipxe-iso.sh` (virtual-media
  ISOs), `contrib/metal/` (`machina-metal-run`, `deploy.sh`), served from `MACHINA_DIST_DIR`
  by `api/dist.rs`; `machinactl dist publish-metal` keeps their checksums in `SHA256SUMS`.
- **CLI**: `scripts/lib/metal-ctl.sh` (`machinactl metal ...`), `machinactl bootstrap`,
  `machinactl host add --file`. Web: `web/src/pages/platform/PlatformBareMetal.tsx`.
- **Migrations** 074 to 083 (pairs): BMC credentials, lifecycle, allocation, deployments,
  images, written marker, profiles, token labels, BMC info, boot method.
- **Rules that bit before**: BMC passwords are stored with `crypto::store_api_key` and read
  with `load_api_key` (never log them; `ipmitool` gets `IPMI_PASSWORD`, not argv);
  validate any address/user before it reaches a command line; deploys need the pinned TLS
  join listener (a plain-http join leaves the agent serving on loopback); power calls are
  idempotent and a boot override is followed by a power step; the libvirt BMC driver only
  touches `metal-test-*` domains.
- **Testing**: unit tests use a local mock Redfish server and a fake BMC for credentials
  (see `baremetal_bmc.rs`, `baremetal_discover.rs`); end to end on a lab host with
  sushy-tools (`quay.io/metal3-io/sushy-tools`, restrict `SUSHY_EMULATOR_ALLOWED_INSTANCES`)
  and VirtualBMC (`quay.io/metal3-io/vbmc`) against throwaway `metal-test-*` VMs. Never
  point a test at a real BMC or an existing VM, and never `rustfmt` a `mod.rs` (it
  reformats every module it declares).

---

## Extra Environment Variables

**Controller**:
- `MACHINA_SKIP_AUTH=1` — disables JWT auth (dev only)
- `MACHINA_CONTROLLER_ID` — unique instance ID
- `MACHINA_DAEMON_URL` — URL the controller uses to reach the daemon
- `MACHINA_API_KEY_MASTER_KEY` — 64-char hex (32 bytes) AES-256-GCM master key for encrypting LLM provider API keys at rest. Unset = plaintext (dev/legacy). Generate: `openssl rand -hex 32`
- `MACHINA_NETPOL_PROJECT_APPROVAL=1` — every project network change (isolation, egress allowlist/IPs) becomes a two-person `vm_netpol.project` approval
- `MACHINA_NETPOL_EGRESS_MANAGE=0` — hosts no longer add missing project egress IPs to their uplink (default on: bpfd adds a `/32`/`/128`, announces it with GARP / unsolicited NA, removes only what it added); `MACHINA_NETPOL_EGRESS_INTERFACE` — interface for them instead of the default route's
- WireGuard overlay (`netpol overlay enable --fleet`, `/api/v1/vm-network-policies/overlay`) is a DB setting, not an env var: needs `wireguard-tools` on every host and UDP 51871 between them; keys stay in bpfd's state dir and are set over netlink (Ubuntu's AppArmor profile blocks `wg` reading keys outside `/etc/wireguard`)
- `MACHINA_NETPOL_EVIDENCE_DIR` (default `/var/lib/machina/netpol-evidence`), `MACHINA_NETPOL_EVIDENCE_EVERY_HOURS` (default 24, 0 = off), `MACHINA_NETPOL_EVIDENCE_KEEP_DAYS` (default 90) — scheduled signed segmentation-evidence exports

**Atlas storage integration** (controller ↔ `../atlas` Zyvor storage control plane; VM disks as Ceph/NFS/ZFS volumes, snapshot/backup/restore via Atlas):
- `ATLAS_ENABLED=1` — enable the Atlas integration (default off). Surfaces the Platform → **Storage (Atlas)** page and `/api/v1/atlas/*`.
- `ATLAS_BASE_URL` — Atlas gateway URL (default `http://127.0.0.1:5110`)
- `ATLAS_TOKEN` — service-account JWT (Atlas `POST /auth/tokens`), sent as bearer when Atlas runs with `ATLAS_AUTH_REQUIRED=1`
- `ATLAS_INSECURE_TLS=1` — accept a self-signed Atlas gateway cert
- `ATLAS_TENANT_ID` (default `machina`), `ATLAS_DEFAULT_POLICY` (default `general`) — recorded on VM volumes / intent→placement
- `ATLAS_BACKUP_BUCKET_ID` — default bound RGW bucket for VM backups
- `ATLAS_RBD_MON_HOSTS` (comma `host:port`), `ATLAS_RBD_AUTH_USER`, `ATLAS_RBD_SECRET_UUID` — Ceph connection params used to attach an Atlas RBD volume as a libvirt network disk. Atlas supplies the per-volume `pool/image`; these supply the monitors + cephx secret (a libvirt `ceph` secret). Empty = rely on the hypervisor's `ceph.conf`/keyring. Create a VM on Atlas storage by passing `atlas_root_disk: true` (+ optional `atlas_policy`) to `POST /api/v1/vms`.

**Native eBPF** (`bpf/` — Pure Rust/Aya; replaces Cilium, Tetragon, PacketWolf and Netra, none of which Machina integrates with any more):
- Crates: `bpf/machina-bpf-ebpf` (kernel programs, `bpfel-unknown-none`, nightly `-Z build-std=core`), `bpf/machina-bpf-common` (shared map/event structs), `bpf/machina-bpf` (loader, policy compiler, `machina-bpfd` service, `BpfdClient`). `make bpf-deps` installs nightly + rust-src + the prebuilt musl `bpf-linker`; without them `build.rs` stages an empty object and bpfd reports `programs_compiled: false`.
- `machina-bpfd` runs as root (`contrib/machina-bpfd.service`) and speaks newline-delimited JSON on `/run/machina-bpf/bpfd.sock` (`MACHINA_BPFD_SOCK`). Daemon exposes it at `/api/v1/bpf/*` (status, policies, mode, interfaces, flows, events, dns, processes, anomalies, health, `l7` (TLS SNI/ALPN, HTTP request line, SSH banner from each TCP flow's first client payload), `accounting` (+`/reset`; per-VM tx/rx/drops folded every 60s and persisted), captures → pcapng, qos, telemetry, `stream` SSE, `vm-edge`, `vm-sandbox`); the agent forwards controller calls via `BpfCall` / `BpfSyncPolicies` gRPC.
- Controller (`controller/src/engine/bpf/`) fans out to every host and backs `/api/v1/zeus-security/*`, the zeus-firewall anomalies/capture, SOC ingest (`machina.bpf` dataset), network canvas and AI root-cause. Controller-owned policies are prefixed `ctl-`; scope is `fleet` | `host:<id>` | `vm:<name>` | `cgroup:<path>`.
- Policy kinds: `deny_ip`, `deny_port`, `tc_allow`, `allow_port`, `deny_process`, `deny_file`, `deny_cap`, `deny_dns`, `rate_limit` (`100/s`, `600/m`, `50/s burst 200` — token bucket on new workload connections per tap, VM taps only). Invalid matches are rejected (HTTP 400 `enforcement_rejected`) before anything is stored.
- Enforcement is **lease-gated and fail-open**: `enforce` needs a lease (`MACHINA_BPF_ENFORCE_LEASE_SECS`, default 900) checked in the datapath itself, and mode is never persisted — a bpfd restart or lapsed lease means `observe`. Auto-attach only matches `vnet*`/`tap*`; never attach default-deny (`tc_allow`) to a host uplink.
- Tests: `make bpf-test` (sudo; netns + veth smoke that never touches real VM taps — covers deny/allow/process/file/port enforcement, capture, QoS, rate_limit, L7, accounting and deny_dns). `make bpf-cni-test` runs the real CNI plugin against two netns pods and a private bpfd (routing, redirect, NetworkPolicy, socket-LB services); safe on hosts that already run another CNI.
- **VM edge + QEMU sandbox** (`bpf/machina-bpf-ebpf/src/vm.rs`, `bpf/machina-bpf/src/server/vm.rs`; fluxvm parity): `PUT /api/v1/bpf/vm-edge` (`vm_edge_sync`) takes `{vms:[{name, group?, addresses, taps?, isolate_ingress, isolate_egress, egress_mbps, ingress_mbps, pps}], policy:[{group, peer: any|world|<group>, egress, proto, port}]}`; bpfd chains `mn_vm_edge_in/out` (TCX) on each VM's taps (libvirt discovery unless `taps` is given), with group identities, conntrack reply bypass and per-direction Mbit/s + PPS token buckets. Rate limits always apply; isolation misses drop only under the enforcement lease, otherwise they count as `observed`. `PUT /api/v1/bpf/vm-sandbox` configures `mn_qemu_device` (cgroup device allowlist: libvirt's default nodes + kvm/vhost/tun/vfio/pts + `extra_devices` like `c 10:232 rw`) and `mn_qemu_egress` (QEMU's own sockets: loopback + `egress_ports`, default migration `49152-49215` and NBD `10809`); `auto: true` sandboxes every `machine.slice/machine-qemu*` scope, `POST/DELETE /api/v1/bpf/vm-sandbox/{vm}` pins one VM. Both are bpf_link multi-attach (AND-ed with libvirt's own device program), record violations per cgroup, and deny only with `mode: enforce` *and* a live lease. The daemon pokes bpfd (`vm_refresh`) on VM start/stop/delete. Smoke: `sudo ./scripts/bpf/vm-edge-smoke.sh ./target/release/machina-bpfd` (test veth + throwaway cgroup; never real taps/scopes).
- **TCP / ICMP diagnostics** (`bpf/machina-bpf-ebpf/src/tcp.rs`, `server/tcp.rs`): `mn_sockops` on the root cgroup (telemetry `tcp`, default on; observe only; `MACHINA_BPF_SOCKOPS_CGROUP` overrides the cgroup) times active connects by socket cookie into `CONNECT_HEALTH` (per remote addr:port count/failures/avg/max + 8-bucket histogram) and snapshots srtt/cwnd/ssthresh/MSS/retransmits/delivery rate per peer into `TCP_PRESSURE` on state-change and retransmit callbacks; both appear in `GET /api/v1/bpf/health` (`connect`, `pressure`). The tc programs count ICMP unreachable / time-exceeded / param-problem / packet-too-big per datapath interface and direction: `GET /api/v1/bpf/icmp-errors`. Sockops context fields must be read with volatile loads before branching (the verifier rejects pre-offset ctx pointers).
- **TLS fingerprints + OpenSSL L7** (`bpf/machina-bpf-ebpf/src/tls.rs`, `server/tls.rs`, JA3/JA4 in `bpf/machina-bpf/src/l7.rs`; all **off by default**): `PUT /api/v1/bpf/tls` `{fingerprints, fingerprint_rate (50/s), ssl_uprobes, ssl_comms: ["curl", …], ssl_all_processes, ssl_rate (200/s)}`. `fingerprints` attaches `mn_tlsfp` (cgroup_skb egress on the root cgroup, `MACHINA_BPF_TLSFP_CGROUP` overrides) which samples up to 2 KB of each ClientHello, and also fingerprints ClientHellos seen by the tap L7 path; `GET /api/v1/bpf/tls/fingerprints` lists JA3 (+md5) / JA4 with SNI, ALPN, workload and a `truncated` flag. `ssl_uprobes` probes `SSL_write(_ex)` / `SSL_read(_ex)` in every libssl found via `/proc/*/maps` (container libs through `/proc/PID/root`) plus the system copies, rescanned every 30 s; only processes in `ssl_comms` are captured and only HTTP method/host/path/status leave bpfd (`GET /api/v1/bpf/tls/ssl`, admin) — the 256-byte plaintext head is dropped after parsing.
- **XDP DDoS shield** (`bpf/machina-bpf-ebpf/src/shield.rs`, `server/shield.rs`): `PUT /api/v1/bpf/shield` (`shield_configure`) `{iface, mode: off|audit|enforce, protected: [addr] | protect_all, syn_pps (1000), udp_pps (5000), icmp_pps (100), other_pps (0 = unlimited), burst_secs (2), allow/deny: [cidr]}` runs inline at the head of the `mn_xdp_uplink` dispatcher (so it shares the uplink with `MACHINA_CNI_XDP` NodePort — one uplink per host). Per-source per-class token buckets live in an LRU (`SHIELD_SOURCES`, 64k); `GET` returns counters and the top over-rate sources. Over-rate/denied/malformed packets drop only in `enforce` with the enforcement lease live, else they count as `audited`. Covered by `netns-smoke.sh`.
- **Node isolation** (`bpf/machina-bpf-ebpf/src/nodeiso.rs`, `server/nodeiso.rs`): `node_iso_configure` `{enabled, iface, lease_secs (mandatory, 10..=900), dry_run, allow_tcp ([22, 6443, 5092, 5093, 50051, 10250]), allow_udp, exempt: [cidr], allow_icmp (true)}` is an emergency drop-all on the uplink — ingress as the `XDP_F_NODEISO` bit at the very head of `mn_xdp_uplink` (before the shield and NodePort redirect), egress as `mn_nodeiso` attached **first** on TCX. Non-IP, IPv6 ND and DHCP always pass; allowlisted ports match either side (so server and client sessions survive); exempt CIDRs bypass entirely. Refuses to enable without 22 allowlisted or an exempt CIDR. Its lease is separate from the policy lease: the datapath stops dropping at `NodeIsoCfg.deadline_ns` by itself and the maintenance tick detaches (`lease_expired: true`). Nothing is persisted, so — like every lease-gated enforcer listed in `ModeState.covers` (policies, shield, VM edge, sandbox, node isolation) — a bpfd restart comes up in observe. **Never test on a real uplink**; `netns-smoke.sh` exercises it on the test veth only.
- **Workload attribution**: flow, net-event, DNS, L7, proc, TLS-fingerprint and SSL records carry `workload: {kind: vm|pod|container|service, ns?, name}`. Taps map to VMs, CNI host veths to pods, cgroup paths via `attribution::cgroup_workload` (machine-qemu scopes → VM; kubepods pod UID → pod, joined to `ns/name` every 10 s from kubelet's `/var/log/pods/<ns>_<name>_<uid>` directories (covers hostNetwork pods) and by finding each CNI endpoint's sandbox `container_id` under the kubepods tree; runtime scopes → container; `.service` → service). SSL records resolve the pid's cgroup from `/proc`.
- **Native data plane API/UI**: daemon `GET|PUT /api/v1/bpf/node-iso` (PUT admin), `GET /api/v1/bpf/cni` (`cni_status`), `GET /api/v1/bpf/cni/services` (`cni_services`: each synced service with `maglev` and live `affinity_entries`), alongside the existing `/bpf/shield`, `/bpf/vm-edge`, `/bpf/vm-sandbox`, `/bpf/tls*`, `/bpf/icmp-errors`, `/bpf/health`. Controller fleet views (operator): `GET /api/v1/zeus-security/native-dataplane` (per-host CNI / VM edge / sandbox / shield / node-iso / TLS status + `isolating_hosts`), `GET /api/v1/zeus-security/tls/fingerprints` (records + `top_ja4`), `GET /api/v1/zeus-security/icmp-errors`; per-host changes go through the existing `POST /api/v1/zeus-security/hosts/{id}/bpf` passthrough (admin). UI: Platform → Security → **Native eBPF** (`PlatformNativeBpf.tsx`) has tabs **Service LB, VM Edge, Shield, TCP Health, TLS / JA4, Node Isolation, Net changes, L7 sampling, VM runtime, VMM guard, Direct redirect, QUIC LB, AF_XDP, Scheduler**, each a self-contained panel in `web/src/components/bpf/` (Node Isolation defaults to dry run and blocks arming without SSH or an exempt CIDR).
- **Kubernetes CNI** (`bpf/machina-cni`, **opt-in**, replaces Cilium/flannel/kube-proxy only when chosen): the cluster bootstrap keeps k3s defaults (flannel, NetworkPolicy controller, kube-proxy) unless `POST /api/v1/k8s/cluster-bootstrap` gets `cni: "machina"` (`CniChoice` in `daemon/src/cluster_bootstrap.rs`; UI checkbox in `K8sOverview.tsx`). The agent and the bootstrap `cni` phase refuse to start when another `*.conf`/`*.conflist`/`*.json` is in the CNI conf dirs (agent exits 78, unit has `RestartPreventExitStatus=78`) unless `MACHINA_CNI_TAKEOVER=1`. one binary is both the CNI plugin (when `CNI_COMMAND` is set: veth `eth0`/`mcXXXXXXXXXXXX`, pod `/32` + link-local gateway `169.254.1.1`, file-based IPAM under the node's podCIDR) and `machina-cni agent` (`contrib/machina-cni.service`). The agent polls `kubectl get … -o json`, installs itself + `05-machina.conflist` into the standard and k3s CNI dirs, adds direct routes (proto 233) to other nodes' podCIDRs, an nft masquerade table `machina_cni` and `/etc/sysctl.d/99-zzz-machina-cni.conf` (`mc*` veths need `rp_filter=0` + `accept_local=1` for NodePort replies; systemd-sysctl would otherwise reset them), then compiles NetworkPolicies (label identities, `ipBlock` CIDRs, ports) and Services (ClusterIP/externalIP/LB via cgroup socket-LB, NodePort via tc on the uplink) into one `CniSync` for bpfd, which owns the `CNI_*` maps. Env: `NODE_NAME`, `MACHINA_CNI_TAKEOVER`, `MACHINA_CNI_CLUSTER_CIDR` (10.42.0.0/16), `MACHINA_CNI_CONF_DIRS`, `MACHINA_CNI_BIN_DIRS`, `MACHINA_CNI_MTU`, `MACHINA_CNI_INTERVAL_SECS`, `MACHINA_CNI_CILIUM_POLICIES=1` (opt-in: also enforce `cilium.io/v2` CiliumNetworkPolicy + CiliumClusterwideNetworkPolicy — endpoint/CIDR/entity peers, `toPorts`, `enableDefaultDeny`; L7 rules enforce at L4, `toFQDNs`/`toServices` fail open on their ports, `*Deny` rules and host policies are ignored, all with agent warnings; `bpf/machina-cni/src/compile/cilium.rs`), `MACHINA_CNI_CLUSTER_CIDR6` (set = dual-stack: nodes need an IPv6 podCIDR, pods get a second `/128` with gateway `fe80::1` on the host veth, IPv6 forwarding is enabled after bumping `accept_ra=1` interfaces to 2, nft table is `inet`), `MACHINA_CNI_LB_MODE` (`snat` default / `dsr`: NodePort to remote backends — eTP=Cluster — either SNATs through the node address or IPIP-encapsulates with the NodePort in the outer IP ID so the backend replies directly; DSR is IPv4 only), `MACHINA_CNI_XDP=1` (NodePort → local backend DNAT at XDP on the uplink via the `mn_xdp_uplink` dispatcher; remote backends/fragments fall through to tc). Services are dual-stack (`clusterIPs`, IPv6 EndpointSlices, one NodePort frontend per `ipFamilies` entry), multi-backend services get a Maglev table (M=1021, built in bpfd from sorted backends), `sessionAffinity: ClientIP` honours `timeoutSeconds` (socket LB keys clients by netns cookie). All CNI map keys are 16-byte (IPv4-mapped); `CniState.version` must equal `CNI_ABI_VERSION` or bpfd rejects the sync. Smoke: `sudo ./scripts/bpf/cni-smoke.sh ./target/release` (netns pods + a test veth uplink; NodePort is probed with raw SYNs because cgroup socket-LB hooks see every netns). The daemon's cluster bootstrap installs k3s with `--flannel-backend=none --disable-network-policy --disable-kube-proxy --disable=servicelb` and its `cni` phase (alias `cilium`) enables `machina-cni`. Named ports resolve against the destination pod's container ports (not towards `ipBlock` peers).
- **Network change audit** (`rtnl.rs`, `server/rtnl.rs`; observe only): `mn_rtnl` kprobes `rtnetlink_rcv_msg` and records every state-changing link/addr/route/neigh/rule/qdisc/filter request with the sending process. `GET|PUT /api/v1/bpf/rtnl` `{enabled, host_netns_only (true), kinds: [] = all but class}`, records at `GET /api/v1/bpf/rtnl/events` (in-memory, capped at 2000). UI tab **Net changes**.
- **Sampled L7** (`l7sample.rs`, `server/l7sample.rs`, decoder `bpf/machina-bpf/src/l7sample.rs`; off by default): `mn_l7s_ingress/egress` (cgroup_skb on the root cgroup, `MACHINA_BPF_L7S_CGROUP` overrides) copy at most the head of one payload segment per flow+direction per `flow_gap_ms` (250) under a host-wide `rate` for Redis, PostgreSQL, MySQL, Kafka and HTTP/2 + gRPC ports (`ports: [{port, proto}]`, default 6379/5432/3306/9092/50051). bpfd keeps only the verb / gRPC method path and drops the payload; records land in the L7 store with `protocol` set. `GET|PUT /api/v1/bpf/l7-sample`, UI tab **L7 sampling**.
- **VM runtime intelligence** (`vmintel.rs`, `server/vmintel.rs`; opt-in): tracepoints (kvm_exit, sched wakeup/switch/migrate, block queue/complete, fault + reclaim, IRQ) gated by shared tracking maps that follow libvirt's `machine-qemu*` scopes (plus `extra: [{name, cgroup|pid}]`). `features: flight | io | mem | topology` (empty = all) build per-VM histograms: KVM exit reasons, vCPU run-queue latency, migrations, block/vhost latency, fault/reclaim latency, first KVM entry, per-CPU IRQ time. `GET|PUT /api/v1/bpf/vm-intel`, per-VM report `GET /api/v1/bpf/vm-intel/vms/{name}`, UI tab **VM runtime**. Smoke: `scripts/bpf/vmintel-smoke.sh`.
- **VMM guard** (`vmmguard.rs`, `server/guard.rs`): BPF-LSM `bprm_check_security` / `file_mprotect` / `file_open` hooks scoped to QEMU cgroups — exec outside QEMU's binaries + `allow_exec`, W+X mappings, char-device opens outside libvirt's defaults + `allow_devices`. `GET|PUT /api/v1/bpf/guard` `{enabled, mode: audit|enforce, lease_secs (1..=3600, enforce only), exec, wx, devices, allow_exec, allow_devices}`, events at `/bpf/guard/events`. Audit by default; enforce needs `bpf` in `/sys/kernel/security/lsm` (otherwise `lsm_inactive` is reported and enforce is refused) and stops by itself at the lease deadline checked in the hook. Struct offsets come from vmlinux BTF (`bpf/machina-bpf/src/btf.rs`). `MACHINA_BPF_GUARD_ASSUME_LSM=1` (tests only) attaches with BPF-LSM inactive. Smoke: `scripts/bpf/guard-smoke.sh`.
- **Direct tap redirect** (`direct.rs`, `server/direct.rs`; opt-in per VM): `PUT /api/v1/bpf/direct` `{vm, outer_iface, enabled, force, tap?, mac?, ips: [], reverse (true)}` redirects frames for the guest MAC (or `ips`) arriving on `outer_iface` straight into the VM tap and, with `reverse`, the VM's frames straight out — bypassing the bridge. Refuses a physical NIC without `force`; entries stay idle unless the enforcement lease is live. UI tab **Direct redirect**; covered by `netns-smoke.sh` on veths only.
- **QUIC LB** (`quiclb.rs`, `server/quiclb.rs`): `mn_xdp_quiclb` is tail-called from the uplink XDP dispatcher (`XDP_F_QUICLB`, before NodePort). `PUT /api/v1/bpf/quic-lb` `{iface, vip, port, backends: [{addr, mac?, server_id?}], cid_len (3..=20), config_id (0..=6), mode: dsr|ipip, encap_src?, enabled}` routes short-header packets by the server id in the QUIC-LB connection ID and Initials by Maglev (shared with the CNI service LB); delivery is a DSR MAC rewrite or IPIP encap + `XDP_TX`. Backend MACs default to the uplink's ARP entries. UI tab **QUIC LB**. Smoke: `scripts/bpf/quiclb-smoke.sh` (veths; sets `MACHINA_BPF_XDP_SKB=1` because native veth `XDP_TX` loses frames without NAPI on the peer).
- **AF_XDP** (`afxdp.rs`, `server/afxdp.rs`): `PUT /api/v1/bpf/afxdp` `{iface, enabled}` attaches `mn_xdp_afxdp` (XSKMAP + per-queue gate, max 64 queues); refuses the default-route and uplink interfaces. A consumer opens its own XSK socket and hands the fd to bpfd with `BpfdClient::register_xsk(iface, queue, fd)` (SCM_RIGHTS over the bpfd socket); bpfd drops its copy after `XskMap::set`, so the kernel clears the entry when the consumer exits and frames fall back to the stack (`no_socket` counter). UI tab **AF_XDP**. Smoke: `scripts/bpf/afxdp-smoke.sh` (Python XSK consumer on a veth).
- **sched_ext VM scheduler** (`bpf/machina-scx`: C struct_ops `src/bpf/scx_machina.bpf.c` + libbpf-rs skeleton, vendored scx headers, `vmlinux.h` generated by bpftool at build time — needs clang, bpftool and `libelf-dev`; feature `scx`): bpfd supervises the `machina-scx` helper (next to `machina-bpfd`, or `MACHINA_SCX_BIN`) over a stdin/stdout JSON protocol. `PUT /api/v1/bpf/scx` `{enabled, lease_secs (1..=3600, required), vms: [], extra: [{name, pid}], latency_target_us?}` moves the targets' `CPU n/KVM` threads to `SCHED_EXT` (partial switch — everything else stays on the fair class) and reports per-VM enqueues/dispatches/queue delay. Lease expiry, helper exit or a kernel ejection restores `SCHED_OTHER` and stops the helper. UI tab **Scheduler**. Smoke: `scripts/bpf/scx-smoke.sh`.
- **Guest per-container policy** (GuestKit, `../guestkit`): `crates/guestkit-ebpf` (aya kernel programs, built by `crates/guestkit-ebpf-runtime/build.rs` and embedded in `guestkitd` on Linux) ports fluxvm's guest cgroup netpolicy (`gk_np_egress/ingress`, cgroup_skb on the container's cgroup: peer CIDR + optional proto/port allow rules per direction) and guest LSM MAC (`gk_lsm_exec/mprotect/open`: exec allowlist, W+X, device allowlist, writes restricted to allowlisted filesystems), keyed by cgroup id. RPCs `guestkit.netpolicy.apply|status` and `guestkit.lsm.apply|status` (`{container | cgroup, mode: audit|enforce|off, lease_secs, …}`, containers resolved through Docker/Podman/crictl to their cgroup subtree) are gated by `capabilities.ebpf` in the guest's `/etc/guestkit/agent-policy.yaml` (off by default). Audit by default; enforce needs a 1..=3600 s lease whose deadline lives in the policy map, so the guest kernel reverts by itself; nothing is persisted. Machina relays them with `guest_agent_actions::guestkit_call` (QGA guest-exec of `guestkitctl --json call`, allowlisted to those four methods) at `GET|PUT /api/v1/vms/{name}/guest-policy` and `/guest-lsm` (PUT admin); VM detail → **Guest policy** tab. Guest smoke: `sudo GK_BIN=target/debug ../guestkit/scripts/ebpf-policy-smoke.sh` (scratch cgroup + netns only).
- **Kernel features** (`bpfd` status `features`): `btf`, `tcx`, `lsm_bpf` (BPF-LSM active), `cgroup2`, `fentry`, `sched_ext` + `sched_ext_state`, `xsk` (AF_XDP socket probe); shown on the Native eBPF overview.

**Black Box, interference and migration planning** (observe/recommend-only; none of these changes a VM, a host or enforcement):
- **Black Box recorder** (`bpf/machina-bpf/src/server/blackbox.rs`): bpfd keeps a rolling per-VM buffer (120 s, 8,192 events) of records it already emits, freezes it on `POST /api/v1/bpf/blackbox/{vm}/trigger` (default 15 s post-trigger, max 300 s) or a high/critical anomaly/alert/guard event; no new BPF programs. `scripts/bpf/machina-blackbox.py` builds the same timeline from the REST telemetry. Docs `docs/ebpf/blackbox*.md`.
- **Root cause** (`controller/src/engine/ai/blackbox_rca.rs`): `GET /api/v1/ai/incidents/blackbox?vm_name=…[&host_id=…]` asks hosts for the frozen capture and ranks hypotheses (storage stall, vCPU starvation, network, …) with evidence.
- **Noisy neighbour** (`engine/ai/noisy_neighbor.rs`): `GET /api/v1/ai/interference/noisy-neighbors` builds an `aggressor -> victim` graph from VM-intel and sched_ext data.
- **Migration** (`engine/migration_oracle.rs`, `engine/adaptive_migration.rs`): `POST /api/v1/vms/{id}/migration-oracle` predicts whether pre-copy converges; `POST /api/v1/migrations/adaptive/decision` turns a telemetry sample into the next control step. Docs `docs/migration/`.
- **Performance autopilot** (`engine/ai/performance_autopilot.rs`): `GET /api/v1/ai/performance-autopilot`, one ranked optimisation plan per VM. Docs `docs/ai/performance-autopilot.md`.
- Status of each is in `docs/claims.md` (C28, C29): unit-tested, not run on a real fleet.

**EC2 Query API** (`controller/src/api/ec2/`; served by the controller on :5093, not the daemon): one SigV4 handler (`mod.rs`: `query`, the `ec2` action match; `sigv4.rs`) answers `POST /ec2`, `/monitoring`, `/autoscaling` and `/elbv2`, and `services.rs` picks the action table, XML namespace, `Version` and error envelope from the service in the credential scope (`ec2`, `monitoring`, `autoscaling`, `elasticloadbalancing`; anything else is `AuthFailure`). `autoscaling.rs` (Auto Scaling on the instance groups) and `elbv2.rs` + `elbv2_model.rs` (ELBv2 on the native layer-4 balancer) each own a `dispatch`; `foundation.rs` holds `DryRun`, `ClientToken`, the filter tables (`FILTER_SPECS`) and `MaxResults`/`NextToken`, which apply to the `ec2` service only. The rest of the `ec2` actions are one module per area: instances and images (`more`, `instance_attrs`, `run_options`, `launch_templates`, `placement_groups`, `spot`, `images`, `images_ext`, `volumes_ext`, `volume_attrs`, `tagspec`), VPC networking (`vpc`, `gateways`, `route_tables`, `nacls`, `sg_rules`, `eni`, `netcommon`, `peering`, `addresses`), balancers and groups (`lb_members`, `groups`), monitoring (`monitoring`, `status`) and Machina's own actions (`machina`, `ops`, `schedules`, `gameday`, `platform`, `capacity`, `fleet`), plus `page.rs`. They call the REST handlers (so project scoping and role checks stay in one place). Resource ids come from `resource_ids.rs`; an id that is looked up by prefix must be exactly 17 hex characters. SQL in these modules is not compile-checked: `api::ec2::tests::every_sql_literal_prepares_against_the_schema` prepares every literal against the migrated schema, so add each new file to its list, and CI's `scripts/db/check_pg_sql.py` does the same on PostgreSQL (compare booleans with `FALSE`, not `0`). Every option is applied, recorded or refused with `UnsupportedOperation`, never dropped; `docs/cloud-ec2-api.md` holds the per-action status table, which `python3 scripts/ec2/action_table.py` regenerates from the action matches (`--check` fails when it is stale: run it when you add an action and give the action a status in the script). Client scripts and Terraform are in `scripts/ec2/` (`docs/cloud-ec2-clients.md`); everything after the original endpoint (claims C39 to C44) is unit-tested only.
**Live migration actuator** (`controller/src/engine/adaptive_actuator.rs`, `core/src/libvirt/migration_control.rs`, agent `GetMigrationStatus` / `ControlMigration` RPCs in `agent/proto/agent.proto`): `POST /api/v1/migrations/adaptive/step` takes a decision from the adaptive engine and applies it to a running migration on the owning host (speed, max downtime, post-copy, abort, bounded vCPU quota, restore); status comes from `virsh domjobinfo --rawstats`. Compression is not applied dynamically. Docs `docs/migration/live-actuator.md`; claim C29 (unit-tested only: never run against a real migration).

**Web (Vite)**:
- `VITE_MACHINA_CONTROLLER_URL` — point the web UI directly at the controller (bypasses daemon proxy; useful for standalone web dev)
