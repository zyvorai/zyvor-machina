# Machina live regression (CDP + API)

Page-by-page and API heartbeat sweeps against a deployed Machina host. The maintained
entrypoints are `page-sweep.js` and `api-sweep.js`.

## Setup

```bash
cd scripts/regression
npm install
```

Chrome with remote debugging (macOS example):

```bash
./chrome-launch.sh &
# or:
# Google Chrome --user-data-dir=/tmp/machina-chrome-regression \
#   --remote-debugging-port=9222 --ignore-certificate-errors about:blank
```

## Run

```bash
export MACHINA_BASE_URL=https://HOST:5092
export MACHINA_USER=sus
export MACHINA_PASS=max
# TLS is verified. For a self-signed daemon cert, trust it explicitly:
export MACHINA_CA_FILE=/path/to/cert.pem   # copy of the host's /etc/machina/ssl/cert.pem
# or, on a throwaway lab host only, skip verification:
# export MACHINA_INSECURE_TLS=1

# API heartbeat
npm run api -- --loops 1

# Interactive ops (power, screenshot, volumes, clone guard, reboot)
npm run ops

# Disk/NIC/rename/linked-clone lifecycle
npm run lifecycle

# Platform console/precheck/host-sync/pause + KubeVirt console guard
npm run platform

# Networks/node/metrics/platform inventory/AI/Zeus firewall
npm run infra

# Devices/services/catalog/batch power/Fleet Cloud+K8s status
npm run fleet

# Native Fleet Cloud CRUD (flavors/keypairs/SGs/LBs/port-forwards)
# Prefer live platform IDs:
#   MACHINA_PLATFORM_VM_ID=… MACHINA_PLATFORM_HOST_ID=… MACHINA_VM_NAME=…
npm run fleetcloud
# or: make regression-fleetcloud

# Browse disks / fleet activity / reports / observability / Atlas
npm run mission

# Jobs/audit/guest-health/network CRUD/HA/CD-ROM guards
npm run catalog

# FluxVM: serial + agent console, snapshots, backup/restore (QEMU + stopped
# Firecracker), hot-add, NICs (bridge + netns across a restart), daemon live migration, controller inventory/migrate/HA re-create (one host).
# Needs [fluxvm] on the daemon and MACHINA_FLUXVM_URL on machina-agent; copies a
# shared disk on the host (locally on loopback, else ssh FLUXVM_SSH). Overrides:
# FLUXVM_IMAGE, FLUXVM_QEMU_KERNEL, FLUXVM_QEMU_INITRD, FLUXVM_FC_KERNEL, FLUXVM_BRIDGE (virbr0),
# FLUXVM_SHARED_DIR, FLUXVM_SKIP_PLATFORM=1
npm run fluxvm
# or: make regression-fluxvm

# Hardware inventory/compat/SOC/K8s/send-key
npm run hardware

# Host stats/PCI/USB/secrets CRUD/rightsizing/Zeus firewall
npm run host

# Health/session + storage/network live inventory + discover
npm run storage

# Zeus firewall deep + API keys/webhooks/nwfilter/cordon
npm run zeus

# Audit/templates/compliance/simulate/terminal session
npm run audit

# Volume CRUD + VM console/observability
npm run volume

# Volume resize/clone + classic disk attach/detach
npm run disk

# Classic+platform pause/resume tasks + NIC inventory
npm run power

# CDP UI (classic Pause/Resume, platform tabs, finder, cinema) — needs CDP
./chrome-launch.sh &
npm run ui
npm run ui-settings
npm run ui-wizards
npm run ui-security
npm run ui-k8s-os
npm run ui-mission
npm run ui-catalog
npm run ui-hardware
npm run ui-host
npm run ui-storage
npm run ui-zeus
npm run ui-audit
npm run ui-volume
npm run ui-power

# Full page sweep
npm run pages -- --loops 1

# Continuous page sweep
npm run pages -- --forever
```

From repo root:

```bash
make regression-api MACHINA_BASE_URL=https://HOST:5092
make regression-pages MACHINA_BASE_URL=https://HOST:5092
```

## Outputs

`ops-fluxvm.js` takes `FLUXVM_ONLY=b` (or `a,c`) to run a subset of its three VMs, e.g. to retest on a busy host.

Written under `scripts/regression/results/` (gitignored; `MACHINA_REGRESSION_OUT` moves it — use that, or run from a
copy, on a host where something else `rsync --delete`s the tree):

- `page-sweep.jsonl` / `page-sweep.log`
- `api-sweep.jsonl` / `api-sweep.log`
- per-suite `ops-*.jsonl` / `ui-*.jsonl` when those runners are used

Live baselines and pass counts: [`RESULTS.md`](./RESULTS.md).

**Tip:** against a remote host, prefer an SSH local-forward to `127.0.0.1` (PAM rate-limits on the public `:5092` login path):

```bash
ssh -f -N -L 15092:127.0.0.1:5092 sus@HOST
export MACHINA_BASE_URL=https://127.0.0.1:15092
```

## Fixtures

- `fixtures/pages.json` — 130 App routes (classic + platform + Fleet Cloud + K8s)
- `fixtures/known-softs.md` — intermittent short-body hydrates (not hard fails)

Override routes: `MACHINA_PAGES_JSON=/path/to.json` or
`MACHINA_EXTRA_PAGES=/platform/foo,/platform/bar`.

## Pass / fail

| Kind | Meaning |
|------|---------|
| PASS | Body length ≥ 100 and no crash/404 copy |
| SOFT | Short/empty body after wait (hydrate race) |
| FAIL | Crash UI, route not found, or CDP/API exception |

API sweep fails a check on HTTP ≥ 400 or HTML proxy error bodies.
Every 5th API loop also exercises pause/resume on `MACHINA_VM_NAME`
(default `chrome-e2e-vm`).
