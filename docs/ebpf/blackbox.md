# Machina Black Box — VM flight recorder

`machina-blackbox.py` correlates Machina's existing native-eBPF telemetry into a single, time-ordered incident timeline for one VM. It is intentionally **observe-only**: it never changes enforcement mode, policy, leases, networking or VM state.

## What it correlates

The first version uses data Machina already exposes: network events and VM-edge flows, DNS, L7, process/runtime events, anomaly detections, VMM guard events and a point-in-time VM-intel report (run-queue, block, fault and reclaim latency plus vCPU migrations). Individual sources are best-effort: if a feature is disabled or a route is unavailable, the report still renders and lists the missing source.

## Usage

```bash
python3 scripts/bpf/machina-blackbox.py payment-db \
  --base-url http://127.0.0.1:5092 \
  --since 2026-10-07T00:00:00Z \
  --until 2026-10-07T00:05:00Z
```

JSON for automation:

```bash
python3 scripts/bpf/machina-blackbox.py payment-db --format json > incident.json
```

Offline/reproducible analysis:

```bash
python3 scripts/bpf/machina-blackbox.py payment-db --input captured-datasets.json
```

The report identifies the **first severe precursor** as a navigational hint. It deliberately does not call that event the root cause: causal diagnosis belongs to the later kernel causal-graph phase and must use stronger evidence.

## Safety

Black Box sends GET requests only. It does not call `SetMode`, quarantine, QoS, policy, scheduler, chaos, isolation or any other changing API. Existing Machina enforcement leases remain untouched.

## Next phase

Move the normalized event ABI into `machina-bpfd`, retain a bounded per-VM ring, add trigger/freeze semantics around anomaly events, and expose the same timeline through `machinactl blackbox`. That phase can preserve 30–120 seconds around an incident even if the UI was not connected.
