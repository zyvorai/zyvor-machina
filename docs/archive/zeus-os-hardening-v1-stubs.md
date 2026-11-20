# Zeus OS v1 stub hardening — operator, multisite, LLDP topology

> **Historical snapshot (archived).** Kept for reference only; it describes an earlier state of the product and is not maintained. Current docs start at [docs/README.md](../README.md).

> Hardens Phase 24–26 stubs with real apply paths and shared LLDP cache (no new phase number).

## AI operator (Phase 25)

[`controller/src/engine/zeus_firewall/operator.rs`](../../controller/src/engine/zeus_firewall/operator.rs):

- `POST /api/v1/zeus-firewall/operator/execute` — dry-run calls `apply_profile(dry_run=true)`; live apply calls agent-backed `apply_profile`
- Approval-gated hosts enqueue `firewall_approvals` instead of silent stub timeline
- `POST /api/v1/zeus-firewall/operator/execute-batch` — fleet dry-run/apply for auto-eligible previews

UI: [`PlatformFirewallOverview.tsx`](../../web/src/pages/platform/security/PlatformFirewallOverview.tsx) — AI operator panel with dry-run + apply buttons.

## Multi-site sync (Phase 24)

[`controller/src/engine/zeus_firewall/multisite.rs`](../../controller/src/engine/zeus_firewall/multisite.rs):

- Sync copies `profile` + `spec_yaml` between site policy tables
- `apply_profiles: true` applies first synced profile to all online hosts via Zeus apply path
- `include_lockdown: true` applies `MetalLockdown` (configurable) to online hosts
- Default site policies seeded when empty

UI: Multi-site card — **Sync primary → DR** with apply/lockdown toggles.

## LLDP topology (Phase 26)

Migration `030_lldp_cache.sql` (since folded into `controller/migrations/000_sqlite_schema.sql`) + [`network_overlay.rs`](../../controller/src/engine/network_overlay.rs):

- `host_lldp_cache` table refreshed from online agents (5m TTL)
- `GET /api/v1/topology` and digital twin graph merge deduped switch nodes + uplink edges
- `GET /api/v1/hosts/{id}/lldp` writes through to cache
- Digital twin supports `target_kind: switch` isolate impact simulation

UI: [`PlatformTopology.tsx`](../../web/src/pages/platform/PlatformTopology.tsx) derives LLDP strip from topology graph (no N+1 host fetches).

## E2E

Section **V1 STUB HARDENING** in [`scripts/lib/e2e-platform-smoke.sh`](../../scripts/lib/e2e-platform-smoke.sh).

## Out of scope

Cross-controller federation control plane, unattended fleet-wide apply without approval gates, live stretch-cluster routing.
