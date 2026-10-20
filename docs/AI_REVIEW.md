# AI code review (machina)

_Generated 2026-10-20 by claude-opus-5-5 from a snapshot of tracked sources; verify before acting._

# Code review: machina

## Summary
Machina is a source-available private-cloud stack for KVM. It is built from Rust services: `machina-daemon` (:5092), `machina-controller` (:5093, SQLite or PostgreSQL), a per-host `machina-agent` over gRPC/mTLS, and the `machina-bpfd` eBPF datapath. It also ships a React web UI, Go/Python/TypeScript SDKs, a Terraform provider, an EC2-compatible API and deploy automation (Ansible, cloud-init, Terraform, Compose). Only part of the source tree is included here (mainly migrations, docs, small Rust/TS modules and scripts), so several findings below are marked as uncertain.

## Bugs and correctness risks

**Controller workers and handlers**
- **The Zeus firewall worker is not leader-gated.** `controller/src/engine/zeus_firewall/worker.rs` runs these every 300 s without checking leadership:
  - `expire_temporary_rules`
  - `expire_stale_approvals`
  - `reconcile_gitops_policies`

  `controller/src/engine/soc/worker.rs` does check `state.leader.is_leader()`. Migrations 017 and 019 show the project already hit double-processing bugs in multi-controller setups. Running GitOps reconcile on every controller is the same class of bug.
- **The placement GET handler writes and skips auth.** In `controller/src/api/placement.rs`, `list_recommendations` has no `AuthUser` / `require_operator` check, unlike `refresh_recommendations` and the other handlers shown. It also calls `persist_recommendations` on every read and discards the error (`let _ =`). Whether an outer auth middleware covers this route can't be confirmed from the files shown, but at minimum the role check is missing.

**Migrations**
- **SQLite function in a PostgreSQL migration.** `controller/migrations_pg/073_local_first.sql` calls `json_extract(config_json, '$.dce_endpoint')`. That is a SQLite function, not a PostgreSQL built-in. Unless the code shown doesn't include a compat function, this migration fails on PostgreSQL.
- **Possible id type mismatch in SQLite.** Migration 046 states ids are stored as 16-byte BLOBs (sqlx `Uuid`). However, 075/077/081/082 declare `server_id TEXT … REFERENCES baremetal_servers(id)` and `host_id TEXT REFERENCES hosts(id)`. If the parent ids are BLOBs, foreign-key matching and joins may silently fail. Please verify.
- **No schema-vs-code test.** `060_lb_member_health_changed_at.sql` records that migration 058 shipped without a column the code writes ("every probe result failed to save"). Nothing in the visible CI catches this class of bug.

**Agent**
- **Agent identity may change on restart.** `agent/src/state.rs` generates a fresh `Uuid::new_v4()` host_id on every start. If enrolment doesn't override and persist it, a restarted agent gets a new identity. Unverified.

**Scripts and web**
- **x86-only CI smoke.** `scripts/ci-kvm-lifecycle.sh` hardcodes `arch='x86_64'` and `/usr/bin/qemu-system-x86_64`, while arm64 packages are advertised.
- **Edge case in gateway guess.** `web/src/utils/predictedRoute.ts` returns `network+1` for /31 and /32 prefixes, which is not a meaningful gateway.
- **Unguarded `localStorage.setItem`.** It is not wrapped in try/catch in:
  - `finderInspectorWidth.ts`
  - `recentVMs.ts`
  - `platformDockPins.ts`

  Sibling utilities do wrap it.

## Security concerns

**Secret exposure**
- **Admin password exposed in process args.** `deploy/ansible/roles/agent/files/mint-join-command.sh` passes `MACHINA_ADMIN_PASSWORD` on `python3`'s argv. It then passes the resulting JSON to `curl -d "…"`, so the password appears in `/proc/*/cmdline` on the controller. `scripts/e2e-test-remote.sh` explicitly avoids this pattern. Use stdin or `--data @-` instead.
- **Committed lab credentials and public IPs.** These appear in:
  - `marketing/videos/README.md`: `80.79.5.173`, `sus`/`max`
  - `scripts/demo-videos/README.md`: `212.8.248.187`
  - `scripts/regression/README.md`
  - `scripts/e2e-*-remote.sh`: `212.8.252.194`, `185.165.240.5`

  Rotate these and remove them from history.
- **Secrets stored in plaintext by default.** EC2 secrets (migration 050), BMC passwords (074) and LLM keys are only encrypted when `MACHINA_API_KEY_MASTER_KEY` is set. Consider refusing to store them, or warning loudly, when it is unset. Per-host `agent_token` (061) storage format is not visible.

**Plaintext transport**
- The handbook says the controller binds `0.0.0.0:5093` over plain HTTP.
- `deploy/ansible/roles/controller/defaults/main.yml` sets `machina_public_url: http://…:5093`.
- The TypeScript SDK and Terraform HTTP examples use basic auth over `http://`.
- The README and Go SDK say the controller is loopback-only.

These conflicting defaults risk credentials crossing the network in cleartext.

**Root-equivalent setuid helper.** `contrib/run-as-user` installs a mode-4755 binary that can exec `useradd`, `usermod` and `chpasswd`. That is effectively root for any caller unless the caller is restricted. `main.rs` is not shown; confirm it checks the caller's uid or group.

**Internal path leak.** `controller/migrations/023_native_stacks.sql` comments reference a local developer path (`/Users/ssahani/.claude/plans/…`).

**Minor**
- `contrib/ingest/test-remote-write.sh` uses `curl -k`.
- `scripts/qualification/*.sh` write to fixed `/tmp` paths.
- The Terraform provider knowingly ships a reachable gRPC vulnerability (documented in its README).

## Test and CI gaps
- **No automated SQLite/PostgreSQL parity check.** Migration 073 shows SQLite-only SQL in the PG tree, and the 058/060 issue shows schema drift from code. Nothing visible applies both migration trees in CI and checks them against the code.
- **Low web coverage bar.** `web/vitest.config.ts` sets thresholds of 20% lines and 25% functions.
- **E2E tests are mostly mocked.** The `web/e2e/*` specs use `mockPlatformApi`. Live suites depend on specific lab hosts and `ignoreHTTPSErrors`.
- **EC2 Terraform path never run.** Its README says "Nothing here has been run against a real endpoint yet."
- **Inconsistent Actions versions.** `.github/workflows/pages.yml` pins `actions/checkout@v7`, `setup-node@v7`, and `upload-pages-artifact`/`deploy-pages@v5`, while `blackbox.yml` uses `checkout@v4`. Verify the pages versions exist; otherwise that workflow fails.
- **SPDX check may not be enforced.** CONTRIBUTING requires `add-spdx.py --check`, but these files have no header:
  - `scripts/qualification/*.sh`
  - `scripts/qualification/scale.py`
  - `scripts/compliance/generate-evidence.py`

  Either they are excluded from the check or it isn't enforced. Separately, `distro/rename.sh` is empty apart from its header.
- **`ci.yml` not available for review.** It is referenced in the README badge but not included here, so Rust test coverage in CI can't be assessed.

## Documentation gaps

**Contradictions**
- **Licensing:** `docs/legal/README.md` says all code is proprietary and GuestKit is LGPL. `PRODUCT-MATRIX.md` says GuestKit is proprietary, with no LGPL. README and `LICENSING-MODEL.md` say Machina is source-available.
- **Repo URL:**
  - `SECURITY.md` uses `zyvorai/machina`.
  - The handbook quickstart clones `zyvorai/machina.git`.
  - `LICENSING-MODEL.md` links `github.com/zyvorai/machina`.
  - README and CI badges use `zyvorai/zyvor-machina`.
- **Controller bind address and transport:** handbook versus README versus Go SDK README (see Security).
- **Terraform naming:** `terraform/machina/README.md` lists a `machina_host` data source, but the native provider exposes `machina_hosts`.

**Stale or incorrect details**
- `docs/client-presentations/README.md` says "All decks (01–10)" although deck 11 exists. `generate-feature-pdfs.sh` says "07–10" in its comment but passes 11.
- `docs/guides/README.md` lists `iotune_tests` twice for volumes.
- `marketing/videos/README.md` describes `edit-demos.sh` as the ffmpeg editor; it is a wrapper around `edit-demos.py`.

**Thin or limiting docs**
- Customer page guides are templated boilerplate, e.g. steps like "**Empty:** —".
- Social card builds require macOS `sips`.

## Top 5 recommended actions (ordered)
1. **Remove secrets and stop leaking new ones.** Remove committed lab IPs and credentials and rotate them. Fix `mint-join-command.sh` so the admin password never appears in argv.
2. **Leader-gate the Zeus firewall worker.** Do the same audit for any other background worker that doesn't check `is_leader()`.
3. **Fix PostgreSQL migration 073 and add parity CI.** Replace `json_extract` with PostgreSQL syntax. Add a CI job that applies both `migrations/` and `migrations_pg/` and runs controller tests on each.
4. **Settle one secure transport default for :5093.** Choose either loopback-only or TLS-by-default. Update the Ansible defaults, SDK READMEs and Terraform examples to match. Warn or refuse when the master key is unset and secrets would be stored in plaintext.
5. **Fix `api/placement.rs` and the setuid helper.** Add the operator check to the placement list handler and stop writing on GET. Review `run-as-user-helper` caller restrictions. Then resolve the licensing and repo-URL contradictions in the docs.
