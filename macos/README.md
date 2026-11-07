# Machina for Mac

Native SwiftUI client for a Machina daemon (`:5092`): VM list, power actions, live updates, menubar extra, notifications. macOS only; no server changes needed.

## Layout
- `MachinaKit/Sources/MachinaKit` — API client (`MachinaClient`), models mirroring `core/src/state.rs`, trust-on-first-use TLS pinning (`PinnedTLS.swift`), live watch stream (`Watch.swift`, `/ws/v1/watch` with a single-use `/api/v1/ws-token` per connection).
- `MachinaKit/Sources/Machina` — the app: `AppModel` (state), `Views` (login, main window, menubar), `Keychain`.

## Build and run
```bash
./scripts/bundle.sh debug      # builds build/Machina.app with swiftc (ad-hoc signed)
open build/Machina.app
```
`bundle.sh` and `test.sh` compile with `swiftc` directly because SwiftPM's build backend stalled on some volumes. `swift build` in `MachinaKit/` also works but can be slow.

## Test
```bash
./scripts/test.sh          # swiftc + Swift Testing, runs in seconds
```
Covers model decoding, 401 re-login, power URLs (spaces and `/` in VM names, session scope), HTTP errors and the watch URL. `cd MachinaKit && swift test` runs the same tests but can be very slow.

## Connect
Server `https://HOST:5092`, a PAM user. The first connect asks to trust the host's self-signed certificate; the SHA-256 pin is kept in the Keychain and a changed certificate is refused.

## Status
Following `~/.claude/plans` phases toward parity with the web UI.
- Done: login, VM list/detail/power, live updates, menubar, notifications; Fleet, Tasks, Alerts; **Phase 1 foundations**: generic `Endpoint`/`JSONValue`, `ResourceSpec` + `ResourceListView`, a data-driven sidebar (`FeatureRegistry`), Cmd-K palette, role from `/auth/session`, and 14 read-only pages (host info, libvirt, health, services, audit, jobs, networks, storage pools, network filters, devices, templates, snapshots, backups, HA events).
- Adding a read-only page = one `ResourceSpec` line in `FeatureRegistry.swift` (empty `columns` derives them from the data).
- **Phase 2** (writes, all behind confirmations and role checks): VM detail tabs (Disks, Network, Snapshots, Backups, Settings), New VM dialog, clone/rename/delete, disk attach/resize/detach/eject, NIC attach/detach, snapshot take/revert/delete, backup run/verify/restore/delete, and row actions on Networks, Storage pools, Snapshots and Backups pages. Writes were live-tested only on a throwaway VM the test created and deleted.
- Next: metrics charts, VNC/serial consoles, then core platform, security/AI/bare metal, Fleet Cloud, and Mac-native polish (App Intents, widgets, notarization).
