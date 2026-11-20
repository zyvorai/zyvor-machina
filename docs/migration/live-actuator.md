# Live Migration Actuator

The adaptive engine ([adaptive-engine.md](adaptive-engine.md)) only decides. The actuator applies a decision to a migration that is already running.

## What it does

- `GetMigrationStatus` (agent RPC): job statistics from `virsh domjobinfo --rawstats`.
- `ControlMigration` (agent RPC): bandwidth (`migrate-setspeed`), maximum downtime (`migrate-setmaxdowntime`), switch to post-copy (`migrate-start-postcopy`), abort (`domjobabort`), and a vCPU quota (`schedinfo`) that is only ever lowered to 70-100% of a CPU per vCPU, with 100% restoring full speed.
- `POST /api/v1/migrations/adaptive/step`: the controller takes a decision from the engine and sends it to the host that owns the VM. The agent address is read from that VM's own host row, never from the request.

## What it does not do

- **Compression is not applied during a job.** This transport has no safe runtime toggle, so a decision to enable compression is reported, not applied.
- It does not start migrations; the migration itself is still `MigrateVm`.

## Status

Compiled and unit-tested. It has not been run against a real migration, so treat the controls as untested until a drill on a disposable guest has been recorded in `docs/claims.md` (C29).
