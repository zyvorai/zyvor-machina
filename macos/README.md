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
P1 done (client, login, VM list, power, live updates, menubar). P2 partly done: Fleet (this host's metrics + controller hosts), Tasks (cancel/retry, active filter) and Alerts views, refreshed every 10 s while open. Not yet: VNC/serial consoles, App Intents, widgets, notarization (see plan).
