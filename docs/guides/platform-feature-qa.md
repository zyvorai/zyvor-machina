# Platform feature QA matrix

Walk through platform VM flows **one feature at a time** (F01–F18). Each ID maps to an automated Playwright test in mock and live suites.

See also: [Platform VM Detail UX](platform-vm-detail-ux.md), [VM daily access (Connect hub)](vm-daily-access.md), deck **11-vm-disks-compute-operations** in [client-presentations](../client-presentations/).

## Feature index — Connect & Console (F01–F13)

| ID | Feature | UI steps (manual) | Expected | Mock test | Live test |
|----|---------|-------------------|----------|-----------|-----------|
| F01 | VM detail hero | Open VM detail Overview | Hero strip with gradient icon, state, pills (blockers / Doctor / SSH) | `npm run test:e2e:features -- -g F01` | `npm run test:e2e:features:live -- -g F01` |
| F02 | Action bar | Header: Cinema, SSH, Power & more | Primary CTAs visible; overflow shows Studio / force reboot | `-g F02` | `-g F02` |
| F03 | Attention stack | Overview banners; click secondary chip | Top issue expanded; chip click swaps banner | `-g F03` | `-g F03` |
| F04 | Connect hub (Overview) | Connect card: Cinema, SSH, copy, laptop path | `vm-daily-access`, checklist, copy feedback | `-g F04` | `-g F04` |
| F05 | Access tab | Tab **Access** or `?tab=access` | Full NAT panel + export buttons | `-g F05` | `-g F05` |
| F06 | SSH dialog | Header **SSH** | NAT banner, expose SSH action | `-g F06` | `-g F06` |
| F07 | NAT presets | Network tab → Expose SSH | POST `/port-forwards` 2222→22 | `-g F07` | `-g F07` |
| F08 | Overview body | Overview scroll | Usage bars + Doctor one-liner | `-g F08` | `-g F08` |
| F09 | Console tab | `?tab=console` | Cinema + Studio guidance links | `-g F09` | `-g F09` |
| F10 | Doctor tab | Doctor → Run migrate plan | Deep-link to Guest health | `-g F10` | `-g F10` |
| F11 | ConsoleHub / Cinema | Open Cinema; Serial lens; Ops shelf | `cinema-shell`, recovery card, ops shelf NAT | `-g F11` | `-g F11` |
| F12 | Machine Finder | Table SSH; Gallery → Cinema | SSH dialog; consolehub navigation | `-g F12` | `-g F12` |
| F13 | Mission Control | Platform home → VM card → SSH | Fleet command center SSH dialog | `-g F13` | `-g F13` |

## Feature index — Disks, compute & devices (F14–F18)

| ID | Feature | UI steps (manual) | Expected | Mock test | Live test |
|----|---------|-------------------|----------|-----------|-----------|
| F14 | Compute | Overview → **Edit CPU** / **Edit memory** | CPU topology modal; memory GiB fields | `-g F14` | `-g F14` |
| F15 | Disks | Tab **Disks** → Attach / Resize | `vm-disks-panel`, libvirt inventory, POST attach | `-g F15` | `-g F15` |
| F16 | Network / NIC | Tab **Network** (More menu) | NIC list, **Attach NIC** button | `-g F16` | `-g F16` |
| F17 | Snapshots | Tab **Snapshots** (More menu) | `vm-snapshots-panel`, Create snapshot | `-g F17` | `-g F17` |
| F18 | Devices / ISO | Tab **Devices**; Disks insert ISO + eject | virtiofs panel; ISO scan + browse + `cdrom.insert` on Disks | `-g F18` | `-g F18` |

## Run all features (mocked, local)

From `web/`:

```bash
npm run build
npm run test:e2e:features
```

Runs serially (`workers: 1`) in feature order F01→F18.

## Run single feature (mock)

```bash
npm run test:e2e:features -- -g "F15"
```

## Run live lab matrix

Requires a running platform with at least one libvirt VM:

```bash
export PLAYWRIGHT_LIVE_URL=https://<ephemeral-ip>:5092
export PLAYWRIGHT_LIVE_USER=operator
export PLAYWRIGHT_LIVE_PASS=max
export PLAYWRIGHT_LIBVIRT_VM_ID=87ddc1a8-b0e9-43c4-b866-faf219663f7f  # optional; auto-picks NAT VM

cd web && npm run test:e2e:features:live
```

Live tests are **read-only** (no delete, force reboot, attach disk, or destructive power actions).

## Remote runner (mock + live on lab host)

From repo root:

```bash
VSPASS=max ./scripts/e2e-feature-matrix-remote.sh operator <ephemeral-ip>
```

Optionally set `PLAYWRIGHT_LIBVIRT_VM_ID` before running.

## Include in full platform E2E

```bash
E2E_FEATURE_MATRIX=1 VSPASS=max ./scripts/e2e-platform-complete-remote.sh operator <ephemeral-ip>
```

## Test files

| Suite | File |
|-------|------|
| Mock matrix | [`web/e2e/platform-feature-matrix.spec.ts`](../../web/e2e/platform-feature-matrix.spec.ts) |
| Live matrix | [`web/e2e/platform-live-feature-matrix.spec.ts`](../../web/e2e/platform-live-feature-matrix.spec.ts) |
| Helpers | [`web/e2e/helpers/featureMatrix.ts`](../../web/e2e/helpers/featureMatrix.ts) |

## Success criteria

- All 18 mock tests pass in CI without live env.
- Live matrix passes or skips gracefully when VM state differs (stopped guest, SSH already exposed, etc.).
