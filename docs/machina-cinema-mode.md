# Machina Cinema Mode

Machina ConsoleHub ships three operator experiences for VM access:

| Mode | URL | Best for |
|------|-----|----------|
| **Machina Cinema** | `/platform/vms/:id/consolehub?mode=cinema` | VNC, SPICE, WebRTC display — full-screen, player-style HUD |
| **Machina Studio** | `/platform/vms/:id/consolehub?mode=studio` | Serial, Shell, split panes, recovery cards, lens bar |
| **Mission Control wall** | `/platform/mission-control/live` | Fleet live preview grid (max 6 concurrent thumbnails) |

Public UI copy uses **Machina Cinema**, **Machina Studio**, and **Ops Shelf** — not “Netflix mode” or generic “ConsoleHub tabs.”

## Cinema (default display experience)

- **Hero layout:** VM canvas fills the viewport; platform sidebar and checklist are hidden.
- **Access Note pill:** Guest/NAT/SSH warnings collapse into one expandable pill (`AccessNotePill`).
- **Control strip:** Bottom HUD with power, scale (Fit/Fill/Native/Scroll/Stretch), screenshot, Studio toggle, Ops Shelf handle. **Labels use dark ink on light glass** (`text-slate-900`) — the strip must not inherit light-theme `--text-primary` on its frosted buttons. Auto-hides after ~3.5s idle; move mouse to reveal.
- **Command palette:** ⌘K scoped console actions (`ConsoleCommandPalette`).
- **Entry points:** VM detail “Open Cinema”, Machine Finder gallery tiles, Fleet Command Center, Spotlight, Live Preview Wall.

Helper paths live in [`web/src/utils/consoleExperienceMode.ts`](../web/src/utils/consoleExperienceMode.ts):

- `cinemaHubPath(vmId)` — explicit Cinema deep link
- `studioHubPath(vmId)` — engineer layout
- `cinemaPopoutPath(vmId)` — centered popout window

## Studio (engineer layout)

- Lens bar: Display · Serial · Shell · AI
- Full banners and recovery cards (`ConsoleLoginRecoveryCard`, `GuestAccessBanner`, `ShellAccessBanner`)
- Ops Shelf slide-over (refactored `CommandCenterPanel`) with port forwards, health, timeline
- Serial-recommended VMs auto-redirect from Cinema → Studio

## Mode persistence

Operators who prefer Studio get their choice restored per VM:

- Key: `localStorage['machina-console-mode:{vmId}']`
- Applied when opening `/platform/vms/:id/consolehub` **without** a `mode` query param
- Explicit `?mode=cinema` links (Open Cinema CTAs) always honor Cinema

## Fleet surfaces

- **Gallery lens** — Machine Finder → Gallery → Netflix-style rows with poster screenshots
- **Live Preview Wall** — Mission Control launchpad → `/platform/mission-control/live`

## Graphics (VNC + SPICE)

New VMs default to `graphics.type: both`. Existing VMs can add/remove listeners via VM Settings → Graphics panel or API:

- `POST /api/v1/vms/{id}/graphics/add`
- `POST /api/v1/vms/{id}/graphics/remove`

## Tests

| Suite | Coverage |
|-------|----------|
| `web/e2e/platform-consolehub.spec.ts` | Cinema default, pill, strip idle-hide, Studio, Ops Shelf, mode restore |
| `web/e2e/platform-machine-finder.spec.ts` | Gallery lens → Open Cinema |
| `web/e2e/platform-mission-control-live.spec.ts` | Live wall tiles |
| `web/src/utils/consoleExperienceMode.test.ts` | Path helpers + persistence |
| `web/src/utils/guestAccessHints.test.ts` | Access Note pill aggregation |

Run locally:

```bash
cd web && npm run build && npx playwright test e2e/platform-consolehub.spec.ts
cd web && npm test -- consoleExperienceMode
```

## Enterprise console (Phase 5)

When `CONSOLEHUB_RECORDING_ENABLED=1` on the controller:

- Cinema shows a **Rec** badge and diagonal **Recorded · {user}** watermark
- Session history rows mark `rec` for recorded sessions
- **Break-glass** in Ops Shelf starts a mandatory audited + recorded session (`POST .../consolehub/break-glass`)
- **Spectator links**: `/platform/vms/:id/consolehub?spectator={token}&session={sessionId}` — read-only view with watermark (validated via `GET /api/v1/consolehub/spectator/validate`)
- **Share view** (collaborative console): Cinema control strip **Share** button or Ops Shelf → `POST /api/v1/vms/{id}/consolehub/collaborate` mints a time-limited read-only link for teammates
- **Session replay**: when recording is enabled, Cinema captures the VNC canvas to `.webm` on exit; Ops Shelf session history shows **replay** when `GET /api/v1/consolehub/sessions/{id}/replay` is available (`CONSOLEHUB_RECORDING_DIR`, default `/var/lib/machina/console-recordings`)
- **Clipboard sync**: Cinema control strip **Clipboard** button opens a panel to send laptop text into the guest via noVNC and copy guest clipboard back when the VM pushes it over VNC
- **Guest file transfer**: Ops Shelf **Send file to guest** builds an `scp` command (NAT or direct guest IP) after you pick a local file
- **SPICE audio**: Performance/SPICE lenses pass `audio=1` to spice-html5 and enable browser autoplay; toggle in Cinema **More** menu (quality depends on guest vdagent and spice-html5 build)
- **Multi-monitor**: when the guest framebuffer is ultra-wide (e.g. 3840×1080), Cinema infers side-by-side panels and shows **All / M1 / M2** chips in the control strip; selecting a monitor scrolls the native viewport to that region (display map shows dividers)

RBAC from the console plan (`permissions` on `/consolehub/plan`):

| Role | Power | Snapshots | Send keys |
|------|-------|-----------|-----------|
| admin / operator | yes | yes | yes |
| viewer / readonly | no | no | no |

## Deferred (later enterprise)

None — Cinema enterprise backlog is complete for v1.
