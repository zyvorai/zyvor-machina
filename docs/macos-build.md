# Developing on macOS (remote Rust builds)

Machina is a **Linux + libvirt/KVM** product. The Rust workspace (`machina-daemon`, `machina-agent`, `machina-controller`, and most crates that link libvirt) **does not compile on macOS**.

**Do not run `cargo build`, `make`, or `make release` in the Machina repo on your Mac.** You will hit missing libvirt/PAM symbols and wasted link time. Compile and verify on a **remote Linux hypervisor** instead.

## What to do locally (macOS)

| Task | Where | Command |
|------|--------|---------|
| Edit Rust / docs / scripts | Mac | normal git workflow |
| Web UI build & mock E2E | Mac | `cd web && npm run build` · `npm run test:e2e` (mock specs) |
| Web UI dev server | Mac | `cd web && npm run dev` (proxies to a running remote daemon) |
| **Rust compile / link / install** | **Remote Linux host** | see below |

Point the web dev proxy at a remote daemon (see `web/vite.config.ts` / platform controller URL) when exercising APIs beyond Playwright mocks.

## Remote compile & deploy (required for Rust)

Sources are rsync’d to `~/.deployment/machina` on the server; **builds run only there**.

```bash
# Full deploy (rsync → make release web → install.sh → systemd)
./scripts/deploy-remote.sh USER@HOST --bind 0.0.0.0

# Compile smoke only (after rsync)
./scripts/deploy-remote.sh USER@HOST --remote-check      # make check on server

# Release build only (after rsync, no full install)
./scripts/deploy-remote.sh USER@HOST --remote-build      # make release on server
```

One-time on the build host:

```bash
sudo ./install.sh --deps-only    # Rust, libvirt-dev, clang for pam-sys, Node/npm
```

Customer tarball without local Rust:

```bash
./scripts/package-binary-remote.sh HOST USER --fetch
```

See [PACKAGE_BINARY_REMOTE.md](PACKAGE_BINARY_REMOTE.md) and [runbook.md](runbook.md).

## Why local Machina Rust builds fail on macOS

- **libvirt** — daemon and agent link `libvirt.so`; macOS has no supported libvirt dev stack for this workspace.
- **PAM / host integration** — password login, host cockpit actions, and fleet agent paths assume Linux.
- **Partial crate success is misleading** — `machina-core` or `machina-controller` may type-check in isolation, but the **product binaries** you need (`machina-daemon`, `machina-agent`) require the full Linux link graph.

## Live E2E against a remote stack

```bash
cd web
PLAYWRIGHT_LIVE_URL=https://HOST:5093 npm run test:e2e -- --config=playwright.live.config.ts
```

Or use the remote UX scripts documented in [ux-e2e-coverage.md](ux-e2e-coverage.md).

## Summary

| ✅ On Mac | ❌ On Mac |
|-----------|-----------|
| `web/` npm build, lint, mock E2E | `cargo build` / `make` / `make release` for Machina Rust |
| Edit code, docs, commit | Expect `machina-daemon` or `machina-agent` to link |
| `deploy-remote.sh` / `package-binary-remote.sh` (SSH to Linux) | Install libvirt on macOS “for local dev” |

**Compile remote. Ship from Linux.**

## macOS app

The native SwiftUI client lives in `macos/` (see `macos/README.md`). It builds on macOS only: `macos/scripts/bundle.sh debug`.

## The native Mac app

`macos/` holds a SwiftUI client for the daemon and controller. Unlike the Rust workspace it builds on a Mac: `macos/scripts/bundle.sh debug` (swiftc, no SwiftPM) and `macos/scripts/test.sh`. Releases are Developer ID signed and notarized by `macos/scripts/release.sh` and published as `macos-v<version>` GitHub releases. See [macos/README.md](../macos/README.md) and the site page "Machina for Mac".
