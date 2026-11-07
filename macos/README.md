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
- **Phase 3**: Swift Charts. Host utilisation history on Fleet; per-VM Metrics tab (CPU %, memory, disk and network throughput, selectable window) and a Logs tab. Rates are derived from the server's cumulative counters in `MetricsMath` (counter resets and zero gaps are skipped).
- **Phase 4 (consoles)**: RFB/VNC client (`RFB.swift`: handshake 3.3/3.7/3.8, security None, Raw/CopyRect/Hextile/DesktopSize, keyboard, pointer, clipboard) with an AppKit surface that captures keys and mouse; Console tab on each VM. Verified live on a throwaway VM (720x400 display, 10 frames decoded, input accepted, clean close). Not supported yet: VNC password auth, ZRLE/Tight, SPICE/RDP.
  **Serial console is disabled** (`VMConsoleTab.serialEnabled = false`): on the test host, opening and closing the serial WebSocket was followed by the daemon exiting and being restarted by systemd, in every run. Suspected cause: the daemon opens the VM's PTY (`daemon/src/routes/ws.rs`, `handle_console`) without `O_NOCTTY`, so it can acquire it as a controlling terminal. The terminal code (`Terminal.swift`, `SerialConsoleModel`) is complete and unit-tested; flip the flag once the daemon is fixed.
- **Phase 5 (core platform)**: pages for the container engine, containers (start/stop/restart/remove) and Podman pods; Kubernetes (cluster overview, nodes, namespaces, pods, deployments, services, events; read-only); DRS consolidation, controller users, API keys, security groups, firewall targets and firewall approvals (approve/reject). Pages can now use dotted column paths (`metadata.name`), flattened key-value layouts, and actions with a query string or fixed body. New approval requests are polled every 30 s: sidebar badge plus a notification with **Review** and **Reject** (Reject needs the Mac unlocked). Update (ambient phase): the notification now also has **Approve**, which needs an unlocked Mac and Touch ID (Settings > Security). `/k8s/pods` appears to list only the default namespace (the overview counts 55 pods, the page shows 2).
  Live-checked read-only against the test host (16 pages). Not live-tested: container writes, approve/reject, and the notification actions (nothing was pending, and no container was created).
- **Phase 6 (security, bare metal, insight; read-only)**: Security (this host's native eBPF telemetry: status, flows, events, anomalies, traffic per VM, process events, DNS, enforcement policies, packet drop reasons, VMM guard, node isolation, native CNI), Fleet security (native data plane per host, firewall status/anomalies/policies, VM network policies, segmentation evidence), Bare metal (servers, image catalog, profiles) and Insight (cost, capacity, compliance score and checks, right-sizing, noisy neighbours, AI action history, AI providers). Sidebar sections are collapsible and long ones start collapsed; Cmd-K reaches every page. **No Phase 6 page changes anything**: enforcement modes, node isolation, bare-metal provisioning and the AI operator are deliberately left out until they can be tested against disposable targets. Checked live read-only (28 pages). Still missing: the AI incidents page (`/ai/incidents/active` timed out on the test host) and per-VM performance autopilot (needs a `vm_id`).
- **Phase 7 (Fleet Cloud, 17 pages)**: instances (with start/shut down/reboot/force stop), flavors, images, volumes, volume snapshots, key pairs, networks, ports, floating IPs / port forwards, elastic IPs, load balancers, stacks, projects, server groups, VPCs, instance groups and the sleep summary. Three generic additions: **New…** forms (`ResourceCreate`: typed, validated fields; flavors, volumes, key pairs, projects), **fan-out** pages that list a parent and fetch each child list (`FanOut`: port forwards per VM, VPCs and instance groups per project; one failing parent is skipped), and **derived** pages (`RowTransform`: server groups come from `anti-affinity:` VM tags, as in the web UI). Checked live: all 17 pages read-only, and a flavor and a key pair created, listed and deleted (controller records only). Not live-tested: instance power actions, volume create/delete, and the delete actions on the other pages. Not built: instance create wizard, stack/heat editing, load balancer members, VPC subnets and route tables, the autopilot/chaos/preemptible/topology pages.
- **Phase 8 (Mac polish and distribution)**: saved servers with a switcher (passwords and certificate pins stay in the Keychain; switching signs in with the stored password), `machina://vm/<name>`, `machina://page/<id>`, `machina://fleet|tasks|alerts` deep links, VM-per-window (sidebar context menu), a Dock menu, Spotlight indexing of VMs (cleared on sign-out), App Intents (Start, Shut Down, Reboot, Take Snapshot, List Running VMs) with an app icon, version stamping and `scripts/release.sh` (hardened-runtime signing, DMG, notarization, stapling).
  **Limits you should know**: (1) The intents compile into the app, but Shortcuts and Siri only discover them from App Intents metadata that Xcode's build generates; the `swiftc`-built bundle does not have it (plain `swiftc` did not emit the const-values the metadata tool needs), so build once with Xcode to use Shortcuts. (2) No widget: WidgetKit extensions need a signed `.appex` plus an app group, which this build setup cannot produce. (3) No OIDC/SAML sign-in: the test server has both disabled, so the flow could not be tested, and handing a browser session back to the app needs server support. (4) Signing and notarization were not run: they need a Developer ID Application certificate and notary credentials (`xcrun notarytool store-credentials`), see the header of `scripts/release.sh`. The unsigned path (release build, ad-hoc signature, DMG, checksum verify) was run. (5) No localization or Sparkle-style auto-update; the accessibility pass is only what SwiftUI gives by default.

## Releasing
```bash
macos/scripts/release.sh                                   # local DMG, ad-hoc signed, not notarized
SIGN_IDENTITY="Developer ID Application: Name (TEAMID)" NOTARY_PROFILE=machina-notary MACHINA_VERSION=0.2.0 macos/scripts/release.sh
```
 VNC/serial consoles, then core platform, security/AI/bare metal, Fleet Cloud, and Mac-native polish (App Intents, widgets, notarization).

- **Ambient / Mac-native**: menu bar glyph that follows fleet health (alerts, offline hosts, approvals) with recent VMs and quick power actions; background poll with alert notifications; quiet hours (critical breaks through); per-VM **Doctor** tab (last good backup age, snapshots, open alerts); Touch ID gate for destructive actions, notification approvals and stored-password sign-in; Settings window (refresh interval, notifications, quiet hours, security, trusted certificate revoke). Logic is in `Ambient.swift` and unit-tested; UI is not screenshot-verified.
