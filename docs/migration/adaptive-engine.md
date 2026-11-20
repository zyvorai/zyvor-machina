# Adaptive Migration Engine

The adaptive engine is the deterministic control brain for live migration.

Every telemetry sample is classified into one next action:

- hold
- increase bandwidth
- enable compression
- throttle guest vCPU (bounded)
- raise downtime target (bounded)
- switch to post-copy (only when explicitly allowed)
- abort
- ready for stop-and-copy

## Guardrails

- packet loss >= 5% -> abort
- time limit -> abort unless already in post-copy
- post-copy is opt-in, never default
- vCPU throttle is bounded (default max 30%)
- bandwidth is bounded (128..4096 MiB/s by default)
- downtime escalation is bounded to 2x target

## API

`POST /api/v1/migrations/adaptive/decision`

The body is a live `MigrationTelemetry` snapshot. The response is the exact next control target.

## Why the engine stops at the decision layer

The current HostAgent API exposes one long-running `MigrateVm` RPC with startup-time bandwidth and post-copy flags, but it does not expose status/control RPCs for an already-running migration. Adding real actuation safely requires a follow-up agent/libvirt RPC that can read job stats and apply bandwidth/downtime/compression/post-copy/throttle controls during the job.
