# Black Box Root Cause Engine

It reads the capture that the native Black Box recorder in `machina-bpfd` froze (see [blackbox-native.md](blackbox-native.md)).

## Endpoint

`GET /api/v1/ai/incidents/blackbox?vm_name=<vm>[&host_id=<host>]`

Without `host_id`, the controller asks every online host for the VM's Black Box and analyzes the freshest incident. It then reads `VmIntelVm`, `NetHealth`, and `ScxStatus` from the same host.

## Ranked hypotheses

1. storage / block-I/O stall
2. vCPU scheduler starvation / noisy neighbour
3. memory pressure / reclaim
4. network path or policy failure
5. security containment / compromise signal
6. guest/application churn

The scorer is deterministic. Severity, distance from the trigger, VM-intel p99 histograms, sched_ext violations and host network context add bounded evidence points. The returned `score` is an evidence score, not a probability. `confidence` measures the strength of the leading hypothesis and its margin over the runner-up.

## Safety

The endpoint is read-only. It does not change enforcement mode, policy, quarantine, sched_ext, VM state or leases. Recommendations are text only.

## Example

```bash
curl -s 'https://controller:5093/api/v1/ai/incidents/blackbox?vm_name=payment-db' | jq
```

Expected shape:

```json
{
  "root_cause": "Most likely: Storage / block-I/O stall.",
  "confidence": 0.7,
  "hypotheses": [
    {"id":"storage_stall","score":72.4,"evidence":["VM-intel block I/O p99 187.000 ms across 1200 samples."]}
  ],
  "deterministic": true
}
```
