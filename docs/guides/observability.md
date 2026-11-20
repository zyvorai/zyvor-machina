# Machina observability guide

Machina exposes host, VM, guest, fleet, and daemon telemetry for Linux KVM hypervisors.

## Quick endpoints

| Endpoint | Purpose |
|----------|---------|
| `GET /api/v1/prometheus` | Prometheus text scrape (requires auth) |
| `GET /api/v1/metrics` | Live VM metrics (libvirt) |
| `GET /api/v1/metrics/history` | Ring buffer + optional JSONL persistence |
| `GET /api/v1/host/linux-observability` | PSI, diskstats, SMART, thermal, cgroups, bpftool summary |
| `GET /api/v1/host/linux-audit` | Recent auditd / `ausearch` events |
| `GET /api/v1/vms/{name}/guest-health` | Guest agent + metrics + issues |
| `GET /api/v1/fleet/metrics` | Multi-node host CPU/memory + capacity score |
| `GET /api/v1/fleet/alerts` | Aggregated automation alerts across fleet peers |
| `GET /api/v1/fleet/prometheus-targets` | Starter Prometheus `scrape_configs` for fleet |
| `GET /api/v1/fleet/prometheus` | Single scrape: local + all peers (adds `machina_peer` label) |

## Prometheus

Scrape with a read-only API token:

```yaml
scrape_configs:
  - job_name: machina
    scheme: http
    metrics_path: /api/v1/prometheus
    static_configs:
      - targets: ['hypervisor.example:5092']
    authorization:
      type: Bearer
      credentials: '<mach_… token>'
```

Import `contrib/grafana/machina-overview.json` for starter dashboards.

## Metrics history (disk)

```toml
[metrics_history]
enabled = true
interval_secs = 30
max_points = 120
persist = true
max_file_mb = 32
# Optional: POST each sample as Machina JSON (NOT Prometheus remote_write protobuf)
remote_write_url = "https://metrics.example/ingest/machina"
remote_write_authorization = "Bearer …"
```

Each JSON POST body is one [`MetricsHistoryPoint`](../../core/src/metrics_history.rs) object (`timestamp_ms`, host percentages, `vm_metrics`, …).

**Native Prometheus remote_write (Snappy protobuf):**

```http
POST /api/v1/metrics/ingest/remote-write
Content-Type: application/x-protobuf
Authorization: Bearer <api-token>
```

Accepts **remote_write 1.0** (`prometheus.WriteRequest`) and **2.0** when senders set:

`Content-Type: application/x-protobuf; proto=io.prometheus.write.v2.Request`

Maps `machina_host_cpu_percent`, `machina_host_memory_percent`, and `machina_host_disk_percent` into the metrics history ring. Response JSON includes `protocol` (`v1` or `v2`). See `contrib/alloy/machina-remote-write-receiver.alloy` and `contrib/ingest/test-remote-write.sh`.

**Ingest scope (intentional):** Machina is not a Prometheus-compatible time-series database. Batch, Prometheus text, and remote_write endpoints decode payloads but only promote the three `machina_host_*_percent` gauges into [`MetricsHistoryPoint`](../../core/src/metrics_history.rs). Other sample names are counted in the JSON response but not persisted or re-exported on `GET /prometheus`. For full metrics storage, scrape `GET /api/v1/prometheus` (or `GET /api/v1/fleet/prometheus`) into Mimir/Cortex/Grafana Cloud via Alloy — see `contrib/alloy/README.md`.

File: `/var/lib/machina/metrics-history.jsonl`

## OTLP export

```toml
[observability.otlp]
enabled = true
endpoint = "http://127.0.0.1:4318"
interval_secs = 60
export_metrics = true
export_logs = true
export_traces = true
```

Compatible with OpenTelemetry Collector and Grafana Alloy (`/v1/metrics`, `/v1/logs`, `/v1/traces`). API responses include a W3C `traceparent` header; OTLP trace export uses random trace/span IDs per request.

Example alert rules: `contrib/prometheus/alerts.yaml`.

## Audit log shipping

```toml
[audit]
max_file_mb = 64
rotate_keep = 5
syslog_enabled = true
http_webhook_url = "https://your-ingest.example/audit"
webhook_authorization = "Bearer …"
sign_lines = true   # prefix each line with sha256:<hex> for tamper detection
```

## Linux auditd & eBPF

- **auditd:** `[observability.linux_audit]` + `GET /host/linux-audit`; set `health_avc_threshold` for `/health/problems`.
- **eBPF:** Machina ships its own eBPF programs through `machina-bpfd` (service LB, VM edge, Shield, TCP health, TLS/JA4, net-change audit, sampled L7, VM runtime intelligence and more). Status and kernel capabilities are at `GET /api/v1/bpf/status`; the `bpftool` summary is still included in `linux-observability` and Prometheus (`machina_bpf_*`). See [../ebpf/README.md](../ebpf/README.md), and [../ebpf/observability.md](../ebpf/observability.md) for the telemetry features.

## Automation

Background worker evaluates alert rules, runs schedules, and fires webhooks on VM lifecycle events. Configure rules/channels in the UI **Settings → Automation** or via `/api/v1/automation/*`.

Prometheus gauges: `machina_alerts_unacknowledged`, `machina_alert_rules_enabled`, `machina_automation_last_tick_unix`, `machina_run_as_user_active`.

## Fleet

Enable `[fleet]` peers in config, then use `/fleet/status`, `/fleet/metrics`, `/fleet/alerts`, `/fleet/placement`, `/fleet/prometheus-targets`, and `/fleet/prometheus` for multi-hypervisor views.

For a single Prometheus job instead of per-peer scrapes:

```yaml
scrape_configs:
  - job_name: machina-fleet
    metrics_path: /api/v1/fleet/prometheus
    static_configs:
      - targets: ['controller.example:5092']
    authorization:
      type: Bearer
      credentials: '<mach_… token>'
```

Peer series are labeled `machina_peer="<peer-name>"`; the local node uses `machina_peer="local"`.

`POST /api/v1/fleet/placement` ranks local + peer nodes by capacity headroom for a requested VM size (`vcpus`, `memory_mb`).

## Settings UI (admin)

**Settings → Observability** edits OTLP export, metrics JSON `remote_write_url`, and `audit.sign_lines` in `/etc/machina/config.toml` (workers reload without restart).

`POST /api/v1/fleet/create-vm` with `auto_place` and a create payload proxies VM creation to the best fleet peer (`peer: "local"` → use `POST /api/v1/vms` locally).

Example JSON ingest receiver: `contrib/ingest/machina-metrics-ingest.py`.

`GET /api/v1/audit/verify` checks `sha256:` prefixes on `/var/lib/machina/audit.log`. CLI: `./machinactl audit verify`.

## Alloy / Mimir

See `contrib/alloy/README.md` for scraping Prometheus and forwarding to a **native** `remote_write` endpoint (separate from Machina JSON ingest).
