# Engineering Onboarding

Machina is a two-layer Linux hypervisor platform — a single-host daemon plus a multi-host fleet
control plane. This is the path from `git clone` to shipping a real, verified change.

| | |
|---|---|
| Repo | `machina` |
| Stack | Rust + React/Vite |
| Daemon port | `5092` |
| Controller port | `5093` |
| Agent gRPC port | `50051` |
| Source of truth | [`CLAUDE.md`](../CLAUDE.md) (repo root) |

---

## Day 0 — Before you start

Nothing here needs deep understanding yet — it just needs to exist before day one starts, so the
first real day isn't lost to access requests.

- [ ] GitHub access to the `machina` repo, confirmed with a clone and `git log`
- [ ] An SSH key on a Linux host you're allowed to build and deploy to — this matters more than it
      sounds, see Day 1
- [ ] Read `CLAUDE.md` at the repo root once, top to bottom. Don't try to retain it — you'll be
      back
- [ ] Skim two or three recently merged PRs to calibrate what "done" looks like on this team

## Day 1 — Orient: what talks to what

Three main Rust services (daemon, controller, agent), one web app, plus the root eBPF service
`machina-bpfd` on each host (see [ebpf/README.md](ebpf/README.md)). The daemon is the original
product — a single-host hypervisor manager. The controller is the newer, optional fleet layer on
top of it.

```
Web UI (React, :3000 dev / :5092 prod)
    │
    ├─► machina-daemon (:5092)      ← REST + WebSocket, libvirt, PAM auth
    │       │
    │       └─► libvirt / QEMU/KVM (same host)
    │
    └─► machina-controller (:5093)  ← Fleet control plane, embedded SQLite, NATS
            │
            └─► machina-agent (:50051 gRPC)  ← per-host gRPC agent
                    │
                    └─► libvirt / QEMU/KVM (remote host)
```

A single host only ever needs `machina-daemon`. The controller and its agents are there for
multi-host fleets, HA, and DRS — optional, not the default case.

> **The one rule that will cost you an afternoon if you skip it**
>
> Never build the Rust workspace on macOS. It links against Linux libvirt headers that don't exist
> there. Build on a Linux host, or validate with `--remote-check` below. The web frontend is the
> exception: it builds and runs fine locally on macOS.

## Days 2–5 — Your first change, end to end

Two separate dev loops, because the two halves of the stack live in different places.

**Web loop — runs on your laptop**

```bash
# hot-reload dev server, proxied to a daemon at :5092
cd web && npm run dev

# your fastest feedback loop for any frontend change — typechecks
# the whole app, not just the file you touched
npm run build

npm test              # vitest
npm run test:e2e      # playwright, needs a running daemon
```

**Rust loop — runs on a Linux host**

```bash
make build                          # debug build, all crates
cargo test -p machina-controller    # one crate at a time is faster
make lint                           # clippy -D warnings
```

**Ship it and check it actually works**

```bash
./scripts/deploy-remote.sh <user> <host> --remote-check   # compiles only, no install — fastest sanity check
./scripts/deploy-remote.sh <user> <host> --quick           # incremental build + restart, minutes
./scripts/deploy-remote.sh <user> <host>                   # full rsync + build + install, from scratch
```

> **A build that passes is not a verified change.** For a UI change, open it in a real browser and
> click the actual path — a stale route, a proxy that isn't wired up, or a blank panel won't show
> up in a green build. This project tests that way deliberately.

**A good shape for a first PR.** Look for a page that's linked from a nav menu but whose route
doesn't actually resolve — a small, self-contained bug that requires actually reading the routing
code across two or three files rather than guessing. Fixing one takes you through the full
commit → push → deploy → verify-in-browser loop once, for real.

## Week 2+ — Own something

1. **See the whole app surface once.** Run the live regression sweep against a real host — it
   exercises pages, power ops, fleet activity, and network/HA CRUD in one pass, not just the
   corner you're about to work in. See [`scripts/regression/README.md`](../scripts/regression/README.md).
2. **Add a Platform page, exactly by the book.** `CLAUDE.md` documents the three-step pattern:
   page component, then a route in `App.tsx`, then a nav entry and breadcrumb label in
   `routes.ts`. Following it once, precisely, teaches you more of the frontend's shape than
   reading it does.
3. **Pick a home base.** Web pages, the controller API, the daemon, or the AI engine under
   `controller/src/engine/ai/` are each a different day-to-day rhythm. Pick one to go deep in
   first rather than staying shallow across all four.

## Field notes — learned the hard way

- Never run `cargo fmt` or `make fmt` across the whole repo — it rewrites roughly 130 unrelated
  files in one pass. Hand-format only the lines you actually touched.
- `~/.cargo`, `~/.rustup`, and any local `target/` are disposable on your own machine — you're not
  building Rust there anyway, so don't invest in keeping them tidy.
- `cargo clippy --workspace -D warnings` currently fails on roughly 75 pre-existing lints inside
  the `core` crate that predate anything you'll be doing. They never reach the agent, daemon,
  controller, or related crates — don't go chasing them.
- Only commit when you're asked to, only push when you're asked to, and confirm before deploying
  to a shared or customer host. None of that is a suggestion.
- "Launchpad" is an overloaded name in the web app — it can mean an app-grid hub component
  (`ApplicationLaunchpad`, `PlatformZyraHubLaunchpad`, `/platform/launchpad`) or a specific
  integration feature.
  Check what a component actually imports before assuming which one you're touching.

## Where to get help

| Source | What's there |
|---|---|
| `CLAUDE.md` | Architecture, every build/test/deploy command, the exact patterns for new pages and API handlers — the actual source of truth, not this doc |
| [`docs/CUSTOMER_SITE_READINESS.md`](CUSTOMER_SITE_READINESS.md) | The pilot and production go-live checklist |
| [`scripts/regression/README.md`](../scripts/regression/README.md) | What the live regression sweep covers and how to run it |
| [`scripts/regression/RESULTS.md`](../scripts/regression/RESULTS.md) | Output of the most recent regression run |
| `git log` / `git blame` | More current than any teammate's memory, this doc included |

---

*Living document — open a PR when something here goes stale.*
