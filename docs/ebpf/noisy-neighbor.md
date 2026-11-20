# Machina Noisy Neighbor Assassin

This feature builds a directed VM interference graph: `aggressor -> victim`.

Evidence:
- pCPU residency overlap
- victim runqueue/scx delay
- aggressor sched_ext runtime share
- victim block p99 + aggressor block-sample share
- dominant network share + victim drops
- corroborating vCPU migrations and sched_ext latency violations

Safety: observe/recommend-only. No migration, throttling, affinity or QoS is changed by this feature.

API:
`GET /api/v1/ai/interference/noisy-neighbors`

Optional query parameters:
`host_id`, `victim_vm`, `limit`.
