# VM Performance Autopilot

VM Performance Autopilot combines existing Machina inventory and native VM-intel telemetry into one deterministic optimization plan.

## Signals

- host CPU pressure
- VM runqueue p99
- sched_ext maximum queue delay
- sched_ext latency-target violations
- block-I/O p99
- reclaim p99
- page-fault p99
- vCPU migration count
- destination CPU/memory headroom

## Recommendations

Depending on evidence, the engine can recommend:

- latency-aware sched_ext class
- vCPU/NUMA locality stabilization
- storage rebalance
- memory/NUMA rebalance
- noisy-neighbour remediation
- predictive live migration to a better host

Every mutating recommendation has `requires_approval=true`.

Migration recommendations also carry `require_migration_oracle=true`, so the move is not treated as safe merely because a cooler destination exists.

## API

`GET /api/v1/ai/performance-autopilot?vm_id=<uuid>`

Optional: `limit=<n>`

## Philosophy

This engine does not use an LLM to choose infrastructure actions. Scores are deterministic and evidence is included in the response.
