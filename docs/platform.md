# Machina Platform Control Plane

vCenter-style centralized management for libvirt/KVM. libvirt remains the engine; operators interact with a declarative API and UI, never raw domain XML.

## Architecture

```text
Web UI (/platform/*)  →  machina-daemon proxy  →  machina-controller  →  gRPC  →  machina-agent  →  libvirt
                                                         ↓
                                       embedded SQLite (/var/lib/machina/controller.db)
```

The controller needs no external database: state lives in an embedded SQLite file
created and migrated on first start (`controller/migrations/`). For very large fleets
there is a PostgreSQL build; see [the database guide](guides/database.md).

## Crates

| Crate | Role |
|-------|------|
| `spec` | Declarative VM/cluster types (`virt.zyvor.dev/v1`) |
| `translate` | Spec → libvirt domain XML |
| `agent` | Per-host gRPC executor + VNC console proxy |
| `controller` | Central API, inventory, task orchestration |

## Capabilities

Task engine (create, power, delete, migrate, clone, maintenance), host enrollment, templates and linked clones,
storage pools and networks, console proxying, snapshots and backups, HA with IPMI/command fencing, DRS
recommendations and auto-balancing, migration pre-checks, optional agent mTLS, RBAC users, API keys, webhooks with
HMAC signing and retries, OIDC + JWT, Prometheus metrics, NATS task fan-out and DB-lease leader election. Fleet-wide
native eBPF (policies, shield, isolation, telemetry) fans out through each agent. History is in
[../CHANGELOG.md](../CHANGELOG.md).

## Quick start

**State store** (optional — this is the default)

```bash
export DATABASE_URL=sqlite:///var/lib/machina/controller.db
```

**Host agent** (each KVM node)

```bash
cargo run -p machina-agent -- --listen 0.0.0.0:50051 --console-listen 0.0.0.0:50052
```

**Controller**

```bash
export MACHINA_SKIP_AUTH=1   # dev only
cargo run -p machina-controller
```

**Web UI** — open the daemon UI at `https://HOST:5092/`, sign in with PAM (or OIDC/LDAP when enabled) and pick a platform section from the top bar. Controller calls go through the daemon's same-origin proxy (`/api/v1/platform/controller`), so no controller URL needs configuring on co-located installs; `VITE_MACHINA_CONTROLLER_URL` overrides it for standalone web development.

## Host enrollment

1. In UI: **Platform → Enroll Host** → generate token
2. On the KVM node:

```bash
curl -fsSL http://127.0.0.1:5093/install.sh | sudo bash -s -- \
  --controller http://127.0.0.1:5093 --token join-XXXX
```

Or:

```bash
machina-agent join --controller http://127.0.0.1:5093 --token join-XXXX
```

## API reference

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/health` | Liveness (+ DB check) |
| GET | `/api/v1/cluster` | Cluster summary |
| GET/PATCH | `/api/v1/cluster/settings` | DRS + HA cluster settings |
| GET | `/api/v1/tasks` | List tasks (`?status=`) |
| POST | `/api/v1/tasks/{id}/cancel` | Cancel pending task |
| GET | `/api/v1/events` | Event stream (`?kind=`) |
| GET | `/api/v1/audit` | Audit log |
| GET | `/api/v1/migrations` | Migration job history |
| GET | `/api/v1/fence/events` | Host fence events |
| GET | `/api/v1/enrollment/tokens` | List join tokens |
| POST | `/api/v1/enrollment/tokens` | Create join token |
| POST | `/api/v1/hosts/join` | Register host (public, token) |
| GET/POST | `/api/v1/hosts` | List / add host |
| GET/PATCH/DELETE | `/api/v1/hosts/{id}` | Host CRUD |
| GET | `/api/v1/hosts/{id}/detail` | Extended host info |
| POST | `/api/v1/hosts/sync-all` | Sync all host inventories |
| POST | `/api/v1/hosts/{id}/sync` | Sync one host |
| POST | `/api/v1/hosts/{id}/maintenance` | Enter/exit maintenance + evacuate |
| GET/POST | `/api/v1/vms` | List (`?project=`, `?host_id=`) / create VM |
| GET | `/api/v1/vms/{id}/spec` | VM declarative spec JSON |
| GET/POST | `/api/v1/vms/{id}/ha` | Read / set HA policy |
| POST | `/api/v1/tasks/{id}/retry` | Retry failed task |
| PATCH | `/api/v1/vms/{id}` | Patch desired_state / project |
| GET | `/api/v1/vms/{id}/disks` | VM disk inventory |
| DELETE | `/api/v1/vms/{id}/snapshots/{name}` | Delete snapshot |
| POST | `/api/v1/hosts/{id}/fence` | Manual host fence |
| GET/POST/DELETE | `/api/v1/maintenance/schedules` | Scheduled maintenance |
| GET/POST/DELETE | `/api/v1/users` | Platform users (admin) |
| GET | `/api/v1/users/me` | Current user |
| GET/POST/DELETE | `/api/v1/api-keys` | API key tokens |
| GET/POST/DELETE | `/api/v1/webhooks` | Event webhooks |
| POST | `/api/v1/webhooks/{id}/toggle` | Enable/disable webhook |
| GET | `/api/v1/projects` | VM project groups |
| GET | `/api/v1/reports/capacity` | Capacity report |
| GET | `/api/v1/metrics/prometheus` | Prometheus metrics |
| GET | `/api/v1/health/ready` | Readiness probe |
| GET | `/api/v1/openapi.json` | OpenAPI stub |
| DELETE | `/api/v1/enrollment/tokens/{token}` | Revoke join token |
| GET/POST | `/api/v1/vms/{id}/snapshots` | Snapshot queue |
| GET/POST | `/api/v1/vms/{id}/backups` | Backup queue |
| GET | `/api/v1/vms/{id}/migrations` | VM migration history |
| DELETE | `/api/v1/templates/{name}/{version}` | Remove template |
| DELETE | `/api/v1/storage/pools/{id}` | Remove storage pool |
| DELETE | `/api/v1/networks/{id}` | Remove network |
| POST | `/api/v1/vms/from-template` | Deploy linked-clone from template |
| POST | `/api/v1/vms/{id}/migrate/precheck` | Migration pre-check |
| POST | `/api/v1/vms/{id}/migrate` | Live migration |
| POST | `/api/v1/vms/{id}/ha` | Enable/disable HA policy |
| POST | `/api/v1/vms/{id}/clone` | Full clone |
| GET | `/api/v1/placement/recommendations` | DRS-style recommendations |
| GET/PATCH | `/api/v1/cluster/settings` | DRS auto-migrate + thresholds |
| GET | `/api/v1/ha/status` | HA dashboard |
| GET | `/api/v1/vms/{id}/console` | Console info + WS path |
| GET/POST | `/api/v1/templates` | Template library |
| GET/POST | `/api/v1/storage/pools` | Storage classes |
| GET/POST | `/api/v1/networks` | Network inventory |
| GET | `/install.sh` | Host enrollment script |

WebSocket console: `GET /ws/v1/platform/vnc/{vm_id}?token=...`

## mTLS (optional)

Agent server:

```bash
export MACHINA_AGENT_TLS_CERT=/etc/machina/agent.crt
export MACHINA_AGENT_TLS_KEY=/etc/machina/agent.key
```

Controller client:

```bash
export MACHINA_AGENT_CA=/etc/machina/ca.crt
export MACHINA_AGENT_CLIENT_CERT=/etc/machina/controller.crt
export MACHINA_AGENT_CLIENT_KEY=/etc/machina/controller.key
```

## Host fencing and HA

HA recovery waits for the failed host to be fenced (IPMI from the controller, or `MACHINA_FENCE_COMMAND` on the
agent). Details, DRS and multi-controller leader election: [controller-ha.md](controller-ha.md).

## Desired vs observed state

The controller stores **desired** state separately from **observed** state. The task worker reconciles via the host agent — Kubernetes-style reliability without Kubernetes.

## Remote deploy + E2E

Deploy the daemon plus platform stack to a remote KVM host:

```bash
VSPASS='…' ./scripts/deploy-remote.sh operator <ephemeral-ip> \
  --quick --platform --e2e --bind 0.0.0.0 --open-firewall
```

This rsyncs sources, runs `make release web` on the server, installs **machina-daemon** (`:5092`), then `scripts/install-platform.sh` (**machina-controller** `:5093` with its embedded SQLite store + **machina-agent**).

With `--platform --e2e`, deploy runs the **full E2E suite** (`e2e-full-test-remote.sh`): daemon libvirt checks, host checklist (no nbd SMART noise), UI platform proxy via `/api/v1/platform/controller`, read-only controller smoke, and VM lifecycle on `:5093`.

### Comprehensive platform E2E (recommended sign-off)

**Mock Playwright alone is not sufficient** for platform UX sign-off — use the orchestrator against a deployed host with real PAM credentials:

```bash
VSPASS='…' ./scripts/e2e-platform-complete-remote.sh operator <ephemeral-ip>
```

This runs, in order: full API/daemon E2E, UX API flow (create + delete VM), live UX wiring manifest (~84 routes), and live Playwright (create/delete VM, platform routes, host smoke). Results are written to `docs/e2e-last-run.json` (not committed).

Optional local mocked CI: `E2E_INCLUDE_MOCK=1` prepends `npm run test:e2e` in the web tree.

### Full E2E (API phases only)

From your laptop:

```bash
VSPASS='…' ./scripts/e2e-full-test-remote.sh operator <ephemeral-ip>
```

Platform-only (skip libvirt daemon tests):

```bash
VSPASS='…' ./scripts/e2e-full-test-remote.sh operator <ephemeral-ip> --platform-only
```

Phases (each skippable via flags on `e2e-full-test.sh`):

1. **Install smoke** — `systemctl` + controller health on the remote host
2. **Daemon E2E** — PAM login, libvirt VM lifecycle
3. **Host health** — `/api/v1/health/problems` and linux-observability exclude nbd SMART
4. **UI platform proxy** — authenticated calls through daemon to controller (browser path)
5. **Platform controller** — read-only API smoke + VM create/snapshot lifecycle

### Auth, sessions, and rate limits

**Browser sessions (daemon `:5092`)** — in `/etc/machina/config.toml`:

```toml
[auth]
max_sessions_per_user = 0   # 0 = unlimited concurrent logins per username (default)
max_sessions_global = 1000
```

When `max_sessions_per_user` is `0`, multiple browsers can stay signed in as the same PAM user without evicting each other. Set a positive value only if you want oldest-session eviction per user.

**Daemon login rate limiting (`:5092`)** — `/auth/login`, `/auth/oidc/login`,
`/auth/oidc/callback` and `/auth/native/exchange` are throttled to 10 attempts per (client IP, path)
per 60s (in-memory fixed window, not configurable via env/config). Every
other daemon route is unaffected. This bounds brute-force/credential-
stuffing against PAM/LDAP login and caps how often an anonymous caller can
force an outbound request to the OIDC IdP.

**Native-app SSO (OIDC)** — the Mac app signs in without a browser cookie.
`GET /api/v1/auth/oidc/login?native=1&redirect_uri=machina://auth&code_challenge=<S256>&code_challenge_method=S256[&state=<opaque>]`
(only that exact `redirect_uri` is accepted) runs the normal OIDC flow; on success
the callback redirects to `machina://auth?code=<code>[&state=...]` instead of
setting a cookie. The code is single use and valid for 60 seconds, held in
memory. `POST /api/v1/auth/native/exchange {code, code_verifier}` checks the PKCE
verifier and returns the usual session JSON with the `machina_session`
`Set-Cookie`. Without `native=1` the web flow is unchanged. SAML has no login
route yet, so native SSO is OIDC only. `GET /auth/providers` reports
`native_sso: true` on servers that have this.

**Controller rate limits (`:5093`)** — in `/etc/default/machina-platform`:

```bash
MACHINA_RATE_LIMIT_PER_MIN=600
MACHINA_RATE_LIMIT_ENABLED=1
# CI only — E2E scripts send header X-Machina-E2E when this is set:
MACHINA_E2E_BYPASS_SECRET=<openssl rand -hex 16>
```

Limits are keyed by **username** (Basic auth) or JWT subject, not the raw `Authorization` header, so E2E and UI traffic for different users do not share one bucket.

Env vars:

- `VSPASS` — PAM password for daemon login (`:5092`)
- `E2E_PLATFORM_USER` / `E2E_PLATFORM_PASS` — controller Basic auth when `MACHINA_SKIP_AUTH` is off (default platform E2E user `machina-e2e`; use `operator` on lab hosts via orchestrator export)
- `MACHINA_E2E_BYPASS_SECRET` — must match the controller env for automated smoke to skip rate limits

### Individual scripts

```bash
# Controller only (direct :5093)
./scripts/e2e-platform-test-remote.sh operator <ephemeral-ip>

# Daemon only (:5092)
VSPASS='…' ./scripts/e2e-test-remote.sh operator <ephemeral-ip>

# Post-install service check on remote
./scripts/e2e-platform-install-smoke-remote.sh operator <ephemeral-ip>
```

Services: `machina-controller`, `machina-agent`. Config: `/etc/default/machina-platform`. Dev E2E sets `MACHINA_SKIP_AUTH=1`; use `--require-auth` on install for production.

## Operations

Playbooks (host down, HA restart, migration, rolling upgrade, support bundle) are in
[handbook/runbook.md](handbook/runbook.md); the certification matrix is in
[platform-cert-matrix.md](platform-cert-matrix.md). Tooling: [`scripts/platformctl`](../scripts/platformctl), the
Terraform provider under `terraform/machina/`, and chaos/soak scripts.
