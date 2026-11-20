# UX E2E coverage matrix

Maps platform UX surfaces to mock Playwright specs and live validation depth.

## Layers

| Layer | Config | Purpose |
|-------|--------|---------|
| Mock | `web/playwright.config.ts` | Fast widget/API flows via `platformMock.ts` |
| Live manifest | `web/playwright.live-ux.config.ts` | 244 routes from `docs/ux-wiring-live-manifest.json` |
| Live deep | `web/playwright.live.config.ts` + `PLAYWRIGHT_LIVE_URL` | Real libvirt/platform operations |

## VM access / ConsoleHub (Cockpit parity)

| Surface | Mock spec | Live |
|---------|-----------|------|
| `VmDailyAccessStrip` | `platform-vm-operator.spec.ts`, `platform-vm-lifecycle.spec.ts` | `platform-live-access.spec.ts` |
| `VmPortForwardPanel` | `platform-vm-operator.spec.ts` | `platform-live-access.spec.ts` |
| `VmLaptopAccessChecklist` | `platform-consolehub.spec.ts`, `platform-vm-operator.spec.ts` | `platform-live-access.spec.ts` |
| `ConsoleLoginRecoveryCard` | `platform-consolehub.spec.ts` | `platform-live-access.spec.ts` |
| `ShellAccessBanner` | `platform-consolehub.spec.ts` | manifest `consolehub` route |
| `GuestAccessBanner` | `platform-consolehub.spec.ts` | manifest `consolehub` route |
| `CommandCenterPanel` | `platform-consolehub.spec.ts` | manifest actions |
| `VmSshConnectDialog` / NAT | `platform-ssh-connect.spec.ts` | `platform-live-access.spec.ts` |
| Classic `/vms` SSH resolve | `classic-operator-ux.spec.ts` | manifest `classic:/vms/:name` |

## Platform shell (173 manifest routes)

| Domain | Mock spec | Notes |
|--------|-----------|-------|
| Mission Control | `platform-mission-control.spec.ts` | Hero, command center, SSH |
| Machine Finder | `platform-machine-finder.spec.ts`, `platform-finder-inspector.spec.ts` | Topology, table, inspector |
| VM lifecycle | `platform-vm-lifecycle.spec.ts`, `platform-vms.spec.ts` | Create wizard, delete |
| Cockpit parity | `platform-cockpit-parity.spec.ts` | CPU, storage, snapshots |
| Security / Zeus | `zeus-security.spec.ts`, `platform-security-ux.spec.ts` | Firewall, SOC |
| Infrastructure globe | `platform-infrastructure-earth-globe.spec.ts` | WebGL + canvas fallback |
| Cross-shell | `cross-shell.spec.ts`, `shell-bridge.spec.ts` | Classic ↔ platform |

## Classic shell (31 manifest routes)

| Domain | Mock spec |
|--------|-----------|
| VM list / SSH | `classic-operator-ux.spec.ts` |
| Storage bridge | `cross-shell.spec.ts` |
| Host networking | `classic-operator-ux.spec.ts` (partial) |

## Fleet Cloud / K8s manifest routes

K8s routes skipped on hosts without K8s (`requires` in manifest). Fleet Cloud routes are always live (native controller feature, no external config gate). Covered by `platform-kubevirt-crud.spec.ts` when K8s is enabled.

## Deploy post-flight

```bash
VSPASS=max ./scripts/deploy-remote.sh operator HOST --quick --platform --e2e --bind 0.0.0.0 --disable-firewalld
```

Runs: API E2E → live UX manifest (244) → live access + VM lifecycle specs.

## CI alignment

| Suite | Default CI (`web` job) | Manual CI (`live-ux` job) |
|-------|------------------------|---------------------------|
| Mock Playwright | Every push/PR | — |
| Live manifest (244 routes) | **Not run** | `workflow_dispatch` only (`.github/workflows/ci.yml`) |
| Live deep access | Remote deploy `--e2e` | Same secrets as `live-ux` job |

Gap: manifest regressions require `./scripts/deploy-remote.sh … --e2e` or triggering the `live-ux` workflow manually.

## Adding new UX

1. Add `data-testid` on interactive widgets.
2. Extend `platformMock.ts` stubs for new API paths.
3. Add mock spec in the matching domain file.
4. Add manifest entry + optional actions in `docs/ux-wiring-live-manifest.json`.
5. Add live deep case in `platform-live-access.spec.ts` or a focused live spec if behavior needs real libvirt.
