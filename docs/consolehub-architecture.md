# Zeus ConsoleHub architecture

ConsoleHub is the unified VM console product surface inside Machina. It is **not** a separate remote-desktop product: noVNC, SPICE, serial, and native SSH are all served natively by the daemon/agent, and Windows RDP is exposed natively via a hypervisor NAT port-forward rather than an embedded gateway.

## Layers

| Layer | Role |
|-------|------|
| **ConsoleHub UI** | Single shell: VM context, protocol tabs, embedded session, AI companion, audit metadata |
| **Native console** | noVNC / SPICE / serial / native_ssh via machina-daemon or controller→agent WebSocket proxies |
| **Native RDP (Windows)** | Hypervisor NAT port-forward to guest `:3389`; user connects with Microsoft Remote Desktop (macOS) or `mstsc` (Windows), optionally via a downloadable `.rdp` file |
| **Access security** | RBAC, JIT approvals, session audit, optional recording (Phases 2–5) |
| **Marketplace plugins** | Kasm, RustDesk, MeshCentral — optional workloads, not core console |

## Request flow (platform)

```text
Browser → ConsoleHub page
  → GET /api/v1/vms/{id}/consolehub/plan   (controller → agent GetConsoleAccessPlan)
  → POST /api/v1/vms/{id}/consolehub/sessions  (native session token)
  → /ws/v1/platform/vnc|spice|serial/{id}   (native console proxy)
  → GET /vms/{name}/rdp-info + download .rdp  (Windows guest → native RDP client)
```

## Protocol routing (deterministic)

| Signal | Default |
|--------|---------|
| Linux + VNC graphics | **noVNC** (native) |
| Windows guest, agent confirms guest listening on port 3389 | **Native RDP** — NAT port-forward to guest `:3389`; connect with Microsoft Remote Desktop or `mstsc`, or download the generated `.rdp` file |
| Headless Linux | **native_ssh** (in-browser PTY terminal) |
| SPICE | **SPICE native**; **webrtc_spice** tab when available (Phase 3) |
| Serial / recovery | **Serial native** |

RDP is only advertised on the console plan once the agent confirms Remote Desktop is actually enabled and listening inside the guest (port 3389 reachable); otherwise the RDP entry is simply omitted from the plan.

## Install

Nothing extra to install for native consoles — noVNC, SPICE, serial, and native_ssh are all served directly by `machina-daemon` / `machina-agent`. Windows RDP needs no gateway either: enable Remote Desktop inside the guest and the agent auto-detects port 3389, after which the daemon exposes the NAT port-forward and offers the `.rdp` file download from the console's Access Note.

Configure controller env (optional):

```bash
export CONSOLEHUB_REQUIRE_APPROVAL=0   # set 1 for JIT approval workflow
export CONSOLEHUB_RECORDING_ENABLED=0
```

## API

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/vms/{id}/consolehub/plan` | ConsoleHubPlan (recommended protocol, native WS path, RDP reachability) |
| POST | `/api/v1/vms/{id}/consolehub/sessions` | Create short-lived native console session |
| POST | `/api/v1/consolehub/sessions/{id}/end` | End session + audit |
| POST | `/api/v1/vms/{id}/consolehub/access-requests` | JIT access request (Phase 5) |
| POST | `/api/v1/consolehub/access-requests/{id}/approve` | Approve JIT request (operator+) |
| GET | `/api/v1/vms/{name}/rdp-info` | Windows guest RDP reachability + `.rdp` file generation |

Legacy `GET /api/v1/vms/{id}/console` remains for compatibility.

## UI routes

| Route | Purpose |
|-------|---------|
| `/platform/vms/:id/consolehub` | Platform ConsoleHub (primary) |
| `/platform/vms/:id/console` | Redirect → consolehub |
| `/vms/:name/consolehub` | Classic single-host ConsoleHub |
| `/vms/:name/console` | Redirect → consolehub |

## Phased roadmap

1. **Phase 1 (shipped):** ConsoleHub shell, noVNC default, native SPICE/serial/native_ssh, native RDP via NAT port-forward for Windows guests
2. **Phase 2:** `console_sessions` audit, RBAC by protocol, recording metadata, approval hooks
3. **Phase 3:** WebRTC/SPICE high-performance tab
4. **Phase 4:** Kasm / RustDesk / MeshCentral as Marketplace plugins
5. **Phase 5:** Zeus Zero Trust Access — JIT requests, break-glass, federation hooks

## Remaining work (post-MVP)

| Item | Status |
|------|--------|
| Classic single-host ConsoleHub shell (daemon-only, no controller) | Partial — `/vms/:name/consolehub` reuses legacy `Console.tsx` |
| Daemon `/consolehub` proxy for non-platform installs | Not started |
| KubeVirt console in ConsoleHub | **Shipped** — serial lens uses KubeVirt subresource WS; display lens uses KubeVirt VNC when `inventory_source=kubevirt` |
| OpenAPI regen for consolehub endpoints | **Shipped** — `node scripts/generate-openapi.mjs` |
| Playwright `@playwright/test` ≥1.61 stable | Pending — suppress DEP0205 via `playwright-node-env.ts` until then |
| Session history UI (`GET …/consolehub/sessions`) | **Shipped** — Command Center Overview tab |
| OIDC/SAML federation for console auth (Phase 5) | Future |
