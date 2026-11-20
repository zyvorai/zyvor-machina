---
sidebar_position: 2
title: Observability
description: Prometheus metrics, OTLP export, Linux pressure signals and fleet-wide scrapes.
---

# Observability

## Endpoints

| Endpoint | Purpose |
| --- | --- |
| `GET /api/v1/prometheus` | Prometheus text scrape (authenticated) |
| `GET /api/v1/metrics` | Live VM metrics from libvirt |
| `GET /api/v1/metrics/history` | Ring buffer with optional JSONL persistence |
| `GET /api/v1/host/linux-observability` | PSI, diskstats, SMART, thermal, cgroups |
| `GET /api/v1/vms/{name}/guest-health` | Guest agent, metrics and detected issues |
| `GET /api/v1/fleet/prometheus` | One scrape for the local host and all peers |
| `GET /api/v1/bpf/flows`, `/bpf/dns`, `/bpf/l7`, `/bpf/health` | Kernel-level flows, DNS, L7 and TCP health from `machina-bpfd` |
| `GET /api/v1/bpf/stream` | Server-sent events of all eBPF telemetry |

## eBPF telemetry

`machina-bpfd` adds what metrics alone can't show: per-flow traffic with VM/pod attribution, DNS, L7 requests
(HTTP, TLS SNI, gRPC, Redis, PostgreSQL, MySQL, Kafka), TCP connect latency and retransmits, JA3/JA4 fingerprints,
an audit of who changed routes and links, and per-VM KVM exit, run-queue and I/O latency histograms. The controller
feeds it into the SOC, the network canvas and Zyra AI root-cause analysis. See
[Native eBPF](../networking/ebpf-overview.md).

## Prometheus

Scrape with a read-only API token:

```yaml
scrape_configs:
  - job_name: machina
    metrics_path: /api/v1/prometheus
    static_configs:
      - targets: ['hypervisor.example:5092']
    authorization:
      type: Bearer
      credentials: '<mach_… token>'
```

Starter Grafana dashboard: `contrib/grafana/machina-overview.json`. Example alert rules:
`contrib/prometheus/alerts.yaml`.

## OTLP

```toml
[observability.otlp]
enabled = true
endpoint = "http://127.0.0.1:4318"
interval_secs = 60
export_metrics = true
export_logs = true
export_traces = true
```

Works with the OpenTelemetry Collector and Grafana Alloy. API responses carry a W3C `traceparent` header.

## Metrics history

```toml
[metrics_history]
enabled = true
interval_secs = 30
max_points = 120
persist = true          # /var/lib/machina/metrics-history.jsonl
```

Settings → Observability in the UI edits these values and reloads the workers without a restart.
