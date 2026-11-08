# Machina for Mac

Native SwiftUI client for a Machina daemon (`:5092`) and its fleet controller. macOS 15 or later, Apple silicon and Intel. No server changes needed.

**Install:** download `Machina-<version>.dmg` from the [`macos-v*` releases](https://github.com/zyvorai/zyvor-machina/releases), open it and drag Machina to Applications. The DMG is signed with a Developer ID and notarized by Apple.

## Connect
Server `https://HOST:5092` and a PAM user. The first connect asks you to trust the host's self-signed certificate; the SHA-256 pin is kept in the Keychain and a changed certificate is refused. Several servers can be saved and switched between (passwords and pins stay in the Keychain). SSO (OIDC/SAML) is not available in the app: the daemon's callback sets an HttpOnly cookie on its own origin and never redirects to a custom URL scheme, so it needs a server change (redirect to `machina://` with a one-time code).

## What is in it
- **VMs:** list, detail tabs (Overview, Disks, Network, Snapshots, Backups, Metrics, Logs, Console, Doctor, Settings), New VM, clone, rename, delete, disk attach/resize/detach, CD-ROM eject, NICs, snapshots, backups. Power actions with live state from `/ws/v1/watch`.
- **Consoles:** a VNC client (RFB 3.3/3.7/3.8, Raw/CopyRect/Hextile, keyboard, pointer, clipboard). Not supported: VNC password auth, ZRLE/Tight, SPICE/RDP. The serial console is disabled until the daemon bug below is fixed.
- **Operations forms:** VMM guard, XDP shield, sched_ext and VM edge forms (admin-only, leases, typed confirmations); bare metal BMC login, image deploy, virtual media and serial-over-LAN; chaos experiment editor; stack drift, converge and auto-heal; VPC routes and subnet NAT; verified self-update (SHA-256, same Developer ID team, Gatekeeper). Fleet pages apply the same forms to one chosen host through the controller (never all hosts). Bare metal also has boot-device override (with a power step), an image catalog (import by URL with a checksum, delete) and a deploy OS picker. The chaos editor takes peer VMs, imports and exports JSON (never auto-run, no targets) and has a run timeline with abort.
- **Fleet and platform:** fleet hosts, tasks, alerts, DRS, approvals, controller users, API keys, security groups, containers and pods, Kubernetes (with scale, restart and delete pod), security and eBPF pages, bare metal, autopilot, chaos, preemptible VMs, topology, AI operator.
- **Fleet Cloud:** instances (with a six-step launch wizard), flavors, images, volumes, key pairs, networks, floating IPs, load balancers, stacks, projects, VPCs and subnets, server groups.
- **Visual insight:** topology map, VM grid with sparklines, capacity heatmap, "what changed" timeline.
- **Productivity:** Cmd-K fuzzy palette, pinned VMs, a VM filter language (`is:running vcpus>=4 mem>=2048 ip:10.0. name`) with saved filters, bulk actions (shift-cmd-B), CSV/JSON export and copy-as-curl on every resource page, a shortcut cheat sheet (cmd-/), drag-and-drop ISO upload.
- **Mac-native:** health-aware menu bar extra, actionable approval notifications (Touch ID gated), quiet hours, Settings window (cmd-,), Dock menu, Spotlight indexing of VMs, App Intents, `machina://` deep links, VM-per-window, Check for Updates, English and German strings, VoiceOver labels. Opt-in extras: a global hotkey for the palette (off by default), a morning digest notification (off by default, only while the app runs), `machina://vm/<name>` share links with Handoff (needs a signed build and the same iCloud account), and a floating read-only VM glance panel.
- Most list pages are data-driven: adding one is a `ResourceSpec` in `FeatureRegistry.swift`.

## Updates
Check for Updates reads the highest `macos-v*` release. Install from the app verifies the DMG's SHA-256 against the release notes (when present), that the app inside has the same bundle id, a newer version, the same Developer ID team and passes `codesign` and Gatekeeper, then asks before replacing the running app. A dev (ad-hoc signed) build never auto-installs. The end-to-end flow has not been run yet and needs a Developer ID signed build to test.

## Safety rules
Every write goes through a confirmation and the role gate (admin-only controls are disabled for operators). eBPF enforcement needs an explicit lease (60 to 900 s) and never relies on the server default. Node isolation starts as a dry run; arming needs the typed phrase `ISOLATE NODE`, a lease, an allowlist that keeps TCP 22 (or an exempt CIDR) and a second confirmation. Bulk and destructive actions list what will happen first. The copied curl never contains credentials, and a request carrying a password or token is refused unless the server URL is https. BMC passwords are masked, never pre-filled and never shown after entry.

## Known limits
- **Bare metal serial-over-LAN is unverified:** the daemon now proxies `/ws/v1/baremetal/{id}/sol` to the controller (needs a daemon build with that route), but no real SOL session has been run.
- **Serial console is disabled** (`VMConsoleTab.serialEnabled = false`): opening and closing the serial WebSocket was followed by the daemon exiting and being restarted by systemd. The daemon now opens the PTY with `O_NOCTTY` (the suspected cause) but this is not yet confirmed on a host, so the app keeps it off until someone tests with a fixed daemon.
- **UI has not been screenshot-verified.** The app was built, unit-tested (183 tests) and checked against a live host through its API layer, but no one has looked at every screen.
- Shortcuts and Siri only discover App Intents from metadata Xcode generates, so build once with Xcode to use them. There is no widget (needs a signed `.appex` and an app group).
- Not built: in-place editing of chaos steps; a custom hotkey recorder; a digest while the app is quit; German strings for the newest UI; profile create/edit for bare metal; SSO ([#224](https://github.com/zyvorai/zyvor-machina/issues/224)). Server-side gaps: load balancers have one listener and route tables have no association API in the native controller (only the SigV4 EC2 API), and there is no converge preview for stacks.
- Writes were live-tested only on throwaway resources created and deleted by name. Container writes, approve/reject, notification actions, bulk actions, Kubernetes and DRS writes, HA policy, ISO upload and the network, VPC and load balancer writes have unit tests but were not exercised against a live host.

## Layout
- `MachinaKit/Sources/MachinaKit`: API client (`MachinaClient`), models, trust-on-first-use TLS pinning (`PinnedTLS.swift`), watch stream (`Watch.swift`), generic data layer (`JSONValue`, `Endpoint`, `Resource`, `FeatureRegistry`, `FeatureRegistryOps`), RFB client, and the pure logic for each feature (`Ambient`, `Productivity`, `VisualInsight`, `IsoTransfer`, `InstanceLaunch`, `UpdateCheck`).
- `MachinaKit/Sources/Machina`: the app (`AppModel`, `Views`, `ResourceListView`, consoles, charts, wizard, settings).
- `Resources/*.lproj`: `Localizable.strings`. `scripts/`: `bundle.sh`, `test.sh`, `release.sh`.

## Build and test
```bash
macos/scripts/bundle.sh debug    # builds macos/build/Machina.app with swiftc (ad-hoc signed)
open macos/build/Machina.app
macos/scripts/test.sh            # swiftc + Swift Testing, runs in seconds
```
`bundle.sh` and `test.sh` call `swiftc` directly because SwiftPM's build backend stalled on some volumes; `swift build` and `swift test` in `MachinaKit/` also work but can be very slow. Keep source files flat in the two `Sources` folders: the scripts glob them.

## Releasing
```bash
macos/scripts/release.sh                                   # local DMG, ad-hoc signed, not notarized
# Developer ID + notarization, with a stored profile ...
SIGN_IDENTITY="Developer ID Application: Name (TEAMID)" NOTARY_PROFILE=machina-notary MACHINA_VERSION=0.3.0 macos/scripts/release.sh
# ... or with an App Store Connect API key (Developer role or higher; leave the issuer out for an individual key)
SIGN_IDENTITY="Developer ID Application: Name (TEAMID)" MACHINA_VERSION=0.3.0 \
  ASC_API_KEY=<key id> ASC_API_KEY_PATH=~/path/AuthKey_<key id>.p8 ASC_API_ISSUER=<issuer uuid> macos/scripts/release.sh
```
The script signs the app, builds and signs the DMG, submits it to Apple, waits and staples the ticket. Verify with `xcrun stapler validate` and `spctl -a -vv -t open --context context:primary-signature <dmg>` (expect `source=Notarized Developer ID`). If signing fails with `errSecInternalComponent`, unlock the login keychain first.

Publish the DMG as a GitHub release tagged **`macos-v<version>`**, not marked Latest: the repository's `v*` tags and Latest release belong to the Machina platform. The in-app update check reads the highest `macos-v*` release.
