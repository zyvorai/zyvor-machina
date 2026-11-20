# Machina Black Box — native VM flight recorder

Machina Black Box is an in-memory, per-VM incident recorder inside `machina-bpfd`.
It reuses records bpfd already emits; it does **not** install new BPF programs and
never changes enforcement, leases, policy or VM state.

## Capture model

- rolling pre-buffer: last **120 seconds** per VM
- hard bound: **8,192 events per VM**
- manual trigger: default **15 seconds** of post-trigger capture
- maximum post-trigger capture: **300 seconds**
- automatic trigger: high/critical `anomaly`, `alert` and `guard` events
- after `freeze_at`, the incident becomes immutable until a later trigger replaces it or it is cleared

The recorder sees the common bpfd publish path, so it captures network, DNS, L7,
process, anomaly, VMM guard and VM-edge flow/alert events even when there are no
SSE subscribers.

## Host API

```text
GET    /api/v1/bpf/blackbox
GET    /api/v1/bpf/blackbox/{vm}
POST   /api/v1/bpf/blackbox/{vm}/trigger
DELETE /api/v1/bpf/blackbox/{vm}
```

Manual trigger body:

```json
{
  "post_secs": 30,
  "reason": "database latency spike"
}
```

`POST` and `DELETE` require the admin role. `GET` is read-only.

Example:

```bash
curl -s http://127.0.0.1:5092/api/v1/bpf/blackbox/payment-db | jq
curl -s -X POST http://127.0.0.1:5092/api/v1/bpf/blackbox/payment-db/trigger \
  -H 'content-type: application/json' \
  -d '{"post_secs":30,"reason":"latency spike"}' | jq
```

## Safety

The recorder is deliberately best-effort. Serialization failures are ignored and
poisoned recorder locks are recovered. Black Box cannot cause a datapath operation
to fail.
