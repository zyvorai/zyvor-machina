# Machina deploy plan — macOS OS Manager Phases 38–48

> **Historical snapshot (archived).** Kept for reference only; it describes an earlier state of the product and is not maintained. Current docs start at [docs/README.md](../README.md).

> Snapshot saved before remote deploy. Update this file at each ship boundary.

## Deploy target

| Field | Value |
|-------|--------|
| Host | `<ephemeral-ip>` |
| User | `operator` |
| UI | https://<ephemeral-ip>:5092/ (or `/login` — redirects to `/` after sign-in) |
| Platform API | http://<ephemeral-ip>:5093/api/v1/health |
| Remote tree | `~/.deployment/machina` |

## Git snapshot

| Commit | Message |
|--------|---------|
| `8dcf569` | Phase 48 General + E2E fixes + v9s UX polish |
| `a8f6bb9` | v9s macOS Tahoe UX shell |

Branch: `main` (synced with `origin/main`)

## Shipped in this deploy (Phases 38–48)

| Phase | AI | macOS app | Fleet API |
|-------|-----|-----------|-----------|
| 38 | 582–591 | Time Machine | `GET /api/v1/fleet/backups` |
| 39 | 592–601 | Finder | `GET /api/v1/fleet/finder` |
| 40 | 602–611 | Network | `GET /api/v1/fleet/network` |
| 41 | 612–621 | Disk Utility | `GET /api/v1/fleet/storage` |
| 42 | 622–631 | Console | `GET /api/v1/fleet/console` |
| 43 | 632–641 | Software Update | `GET /api/v1/fleet/updates` |
| 44 | 642–651 | Keychain | `GET /api/v1/fleet/keychain` |
| 45 | 652–661 | Users & Groups | `GET /api/v1/fleet/users` |
| 46 | 662–671 | Shortcuts | `GET /api/v1/fleet/shortcuts` |
| 47 | 672–681 | Stage Manager | `GET /api/v1/fleet/spaces` |
| **48** | **682–691** | **General** | **`GET /api/v1/fleet/general`** |

Also in this batch: v9s Tahoe UX shell (Help menu, columns view, host popout). *(Mac dock retired 2026-09 — navigation is sidebar icon rail + Spotlight.)*

## Deploy command

```bash
cd /Users/ssahani/tt/machina
VSPASS=max ./scripts/deploy-remote.sh operator <ephemeral-ip> --quick --platform --e2e --bind 0.0.0.0 --open-firewall
```

## Post-deploy verification

```bash
curl -sk https://<ephemeral-ip>:5092/api/v1/health
curl -s http://<ephemeral-ip>:5093/api/v1/health
./scripts/platformctl fleet general    # Phase 48
./scripts/platformctl fleet spaces     # Phase 47
```

Spotlight smoke: `general settings`, `stage manager`, `mission control`.

## Deploy notes (2026-05-30)

### Last deploy attempt

| Check | Result |
|-------|--------|
| Git push `8dcf569` | **Success** |
| Remote deploy | **Failed** — SSH timeout syncing GuestKit sibling repo (`Operation timed out`) |
| Action | Retry when `<ephemeral-ip>` is reachable: `VSPASS=max ./scripts/deploy-remote.sh operator <ephemeral-ip> --quick --platform --e2e --bind 0.0.0.0 --open-firewall` |

Previous successful deploy (Phases 38–47): commit `001a2d5`, E2E 277 passed / 5 failed (fixed in `8dcf569`).

## Related docs

