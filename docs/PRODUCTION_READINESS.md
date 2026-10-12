# Production Deployment & Readiness Runbook

Operator guide for deploying Machina at a customer site. Written from a real deploy +
verification pass — including the failure modes that actually bit us. **Read the "Known
limitations" section before promising a customer this runs unattended.**

---

## 1. Prerequisites (per KVM host)

- Ubuntu 22.04/24.04 (or comparable), root/sudo.
- `libvirtd` + KVM running, a libvirt storage pool with capacity for VM disks, and the
  `default` libvirt network active.
- Build deps (the installer installs these): `libvirt-dev pkg-config protobuf-compiler
  libclang-dev clang libssl-dev qemu-kvm virtinst`.
- `openssl` (used to generate secrets).
- Ports: controller `:5093`, daemon/UI `:5092`, agent gRPC `:50051`, console `:50052`.
- Outbound network if you want alert channels to reach Slack/SMTP.

## 2. Secrets — MANDATORY (the controller refuses to boot without them)

The controller has a boot-guard: it **will not start** on the dev-default JWT secret or the
default `admin` password. As of the deploy-hardening change, `install-platform.sh` now
**auto-generates** strong values on first install and **preserves them on re-deploy**:

- `MACHINA_JWT_SECRET` — signs platform JWTs (bridges controller ↔ daemon).
- `MACHINA_AGENT_TOKEN` — authenticates controller ↔ agent gRPC and the console port. A host added with `machina-agent join` gets its own token instead (kept in `hosts.agent_token`, stored on the host by `join`, dropped when the host is removed); hosts without one use the shared value.
- `MACHINA_ADMIN_PASSWORD` — seeds the bootstrap `admin` user (printed once at install — **save it**).

They live in `/etc/default/machina-platform` (mode 0600). To rotate an **API key**, use
`POST /api/v1/api-keys/{id}/rotate`. (Live JWT-signing-key rotation is not yet automated —
see Known limitations.)

> Historical footgun (now fixed): a `--platform` re-deploy used to overwrite this file and
> wipe the secrets, taking the controller down. If you're on an older build, keep secrets in
> a systemd drop-in (`/etc/machina/secrets.env` + `…​.service.d/10-secrets.conf`) so an
> install can't clobber them.

## 3. Deploy

From a workstation with the repo (and the sibling `guestkit` repo checked out beside it):

```bash
./scripts/deploy-remote.sh <user>@<host> --platform --quick
```

`--platform` installs/updates **controller + agent** (`--quick` alone only updates the
daemon). The build runs **on the host** (rsync → cargo/npm build → install → systemd). First
build is slow; subsequent `--quick` builds are incremental.

## 4. Post-deploy verification (do every deploy)

```bash
# services + health
ssh <user>@<host> 'systemctl is-active machina-controller machina-agent machina-daemon libvirtd'
curl -s -o /dev/null -w '%{http_code}\n' http://<host>:5093/api/v1/health          # 200
# migrations applied (new tables present)
ssh <host> 'sudo sqlite3 /var/lib/machina/controller.db ".tables"'
# controller ↔ agent working (host online, VMs listed)
curl -su admin:<pw> http://<host>:5093/api/v1/vms
# console token gate (agent :50052): no/wrong token -> 401, right token -> 101
# cert status
curl -su admin:<pw> http://<host>:5093/api/v1/cert-status
```

If the controller is in a restart loop, check the journal for
`Refusing to start:` — a secret is missing/default.

## 5. Rollback

Every install keeps the previous binaries. To revert a bad deploy:

```bash
ssh <host> 'sudo cp -f /usr/local/bin/machina-controller.prev /usr/local/bin/machina-controller \
  && sudo cp -f /usr/local/bin/machina-agent.prev /usr/local/bin/machina-agent \
  && sudo systemctl restart machina-controller machina-agent'
```

(The SQLite DB is not rolled back; migrations are additive. Back up
`/var/lib/machina/controller.db` before a risky upgrade.)

## 6. Optional integrations

- **Alert channels** (Slack/email): `POST /api/v1/notification-channels` (`kind`:
  `slack`|`email`|`webhook`). Email needs SMTP env: `MACHINA_SMTP_HOST`, `_PORT`, `_FROM`,
  `_USER`, `_PASS`. Use `/notification-channels/{id}/test` to confirm wiring.
- **Threshold alerts**: `POST /api/v1/alert-rules` (feeds the channels).
- **Backups**: `POST /api/v1/backup-schedules`. **Targets** (`/backup-targets`) for offsite.
- **TLS**: default cert is self-signed (browsers won't trust it) — install a real CA cert at
  `/etc/machina/ssl/cert.pem` (+ key). `cert-status` / the cert monitor warn 30 days before
  expiry via the channels.

## 7. Known limitations (be honest with the customer)

These are real gaps observed in verification — a customer should know them up front:

- **CI branch protection is not enforced automatically.** CI now gates on build + tests, but
  you must enable branch protection on `main` (Settings → Branches) so red CI actually blocks
  merges. Without it, broken code can still land.
- **`fmt`/`clippy` are advisory in CI** (large pre-existing backlog); they don't block yet.
- **Metrics reliability.** `vm_metrics` (esp. guest memory) is only populated when the guest
  agent reports it; on VMs without a fully-reporting agent it can read 0 intermittently. This
  makes **threshold alerts, capacity/rightsizing, and the memory-based watchdog signal**
  unreliable until guest tooling is consistently installed and reporting.
- **Live JWT-signing-key / agent-token rotation is not automated.** API-key rotation exists;
  rotating the JWT key or agent token requires a coordinated manual change + restart.
- **Some UI is API-only**: the health watchdog, notification channels, and cert status have
  endpoints but limited/no UI yet.
- **Snapshots require qcow2 disks** (internal snapshots); raw disks will fail snapshot ops.
- The full e2e suite still has a few failing/peripheral phases; don't treat "e2e green" as a
  release gate until those are triaged.

## 7a. KNOWN CRITICAL correctness gaps — data-path audit 2026-07-11 (read before a customer deploy)

A focused audit of the code paths that decide where VMs run and how their data is protected
found architectural issues that are **not yet fixed** (they need design work, not one-line
patches). The bounded, high-confidence ones from the same audit **were** fixed (commit
`a6c0153f`: restore ownership/completeness guard, restore path allow-list, multi-disk
refuse-instead-of-lose, HA/evac `schedulable` + `desired_state` filters, firewall authz).
What remains:

- **HA split-brain + controller-side fencing — FIXED (`1425e033`), needs live validation
  on real BMC hardware.** Fencing now originates from the controller (`ipmitool` → the host's
  BMC), so a dead/partitioned host can actually be isolated (previously the fence ran on the
  dead host's own agent and could never reach it). Recovery is now gated on the host being
  *confirmed fenced* for **every** ha-enabled VM (not just `fence_on_failure=TRUE` ones) — if a
  host can't be fenced, its VMs are **not** auto-recovered and an `ha.blocked_unfenced` event is
  raised for manual action. Requirements/caveats: set `fence_method='ipmi'` + `ipmi_address`/
  creds per host and install `ipmitool` on the controller (now in the deps); without a BMC,
  automatic HA recovery will (correctly) not happen on a hard failure. `ha_allow_unfenced_recovery`
  (cluster setting, default off) is the explicit opt-out for **non-shared-storage** clusters.
  Not yet exercised end-to-end against real IPMI hardware — validate before relying on it.
  The IPMI driver itself (power state read-back, idempotent power, non-default port, the
  `cycle` fallback) has since been run through the deployed API against VirtualBMC on a lab
  host ([claims.md](claims.md) C51), and the fencing password is encrypted at rest when
  `MACHINA_API_KEY_MASTER_KEY` is set; neither replaces a test on your BMC.
- **Migration/evacuation source-undefine — FIXED.** Maintenance evacuation and DRS rebalance
  now pass `undefine_source: true`, so the source domain is undefined on successful migration
  (libvirt does this atomically only on success) and can't autostart on the source while running
  on the destination. Manual one-off migrations still default to keeping the source unless asked.
- **Evacuation capacity — FIXED.** Maintenance evacuation now plans a capacity-aware destination
  per VM (reusing HA recovery's memory-headroom picker with per-batch reservations) and refuses
  to drain — rather than overcommit one host — if any VM can't be placed. Previously it targeted
  every VM at the single least-loaded host.
- **`host.maintenance` drain confirmation — FIXED.** The maintenance task now blocks until the
  host has no running VMs left (or fails, telling the operator NOT to power down), bounded by
  `MACHINA_MAINTENANCE_DRAIN_TIMEOUT_SECS` (default 900s). Task success now means the host is
  genuinely drained.
- **Backups of a running VM are crash-consistent only by luck.** `qemu-img convert` copies the
  live qcow2 with no `FSFreeze`/snapshot/pause, and integrity is not verified before the record
  is marked `completed`. Treat backups of busy/DB VMs as potentially inconsistent; prefer
  snapshot-then-backup, and test-restore before relying on any backup.
- **"Incremental" backups build a fragile chain and retention can delete the base**, making the
  retained incremental unrestorable. Use full backups until this is reworked.
- **Deleting an external snapshot of a running VM leaks the overlay qcow2 on disk** (metadata-
  only delete). With scheduled snapshots this slowly consumes the pool; monitor pool usage.
- **Dual-leader window — FIXED.** `is_leader()` now self-expires against the lease deadline and
  steps down a guard window (3s) before any challenger can acquire, so a stalled renewal task
  can no longer leave two controllers both running HA/DRS/reconcile. (A fencing token on writes
  would harden it further, but the timing window is closed.)
- **Anti-affinity + DRS hysteresis — ADDED.** Tag a VM `anti-affinity:<group>` and placement/DRS
  won't co-locate it with a group peer (so a host failure can't take out both replicas), falling
  back to co-location only if no other host exists. DRS now also requires the destination to beat
  the source by a margin before migrating, preventing ping-pong between similarly-loaded hosts.

**Verified live (2026-07-11, host <ephemeral-ip>):** an end-to-end restore drill on a real
managed VM — backup via the platform API, blank the disk, restore — recovered the **exact**
disk content (sha256 match), and a cross-VM restore was correctly rejected (HTTP 404). Two
out-of-the-box blockers were found and fixed in the process: local **backup writes** and
**restore reads** were both rejected because the backup dir was missing from the storage
allow-list (`30e53e7f`), and `deploy-remote.sh` could leave a service running the *previous*
binary after a deploy (stale-binary guard added). So: **local, single-disk, stopped-VM
backup/restore is now proven.**

**Still not proven / not safe:**
- **Crash-consistent backup of a *running/busy* VM** (no FSFreeze/snapshot; see above).
- **Multi-disk VMs** now *refuse* backup/restore (fail-safe) rather than silently losing disks.
- **HA/fencing on real BMC hardware** — the split-brain fix (controller-side fence + fence-gated
  recovery) is in code and unit-tested, but not yet validated end-to-end against real IPMI.

**Bottom line for a customer:** single-host or quiet multi-host operation is in reasonable
shape after the fixes, and local restore is now demonstrated to work. The HA split-brain root
cause (fencing that couldn't reach a dead host + recovery that skipped the fence for most VMs)
is now fixed in code — but before relying on automatic HA failover, **validate fencing against
your real BMC** (set `fence_method='ipmi'` + creds per host) and confirm an `ipmitool power off`
succeeds from the controller. Backup of busy VMs is still not crash-consistent. Always do your
own restore drill.

## 8. Upgrade procedure

1. Back up `/var/lib/machina/controller.db`.
2. `git pull` on the deploy workstation; ensure CI is green for that commit.
3. `./scripts/deploy-remote.sh <user>@<host> --platform --quick`.
4. Run the §4 verification checklist.
5. If anything regresses, roll back per §5.
