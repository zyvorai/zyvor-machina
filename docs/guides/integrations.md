# Integrations (KubeVirt, k8s, automation)

Machina surfaces optional backends through a single status API and Settings UI.

## Status API

`GET /api/v1/integrations/status` returns:

- **kubevirt** — exec enabled, default namespace/storage class, route hints
- **automation** — worker interval, `last_tick_unix`, alert rule counts, unacked alerts
- **k8s** — kubeconfig auto-selection, inventory history flag, route hints
- **run_as_user** — impersonation mode and whether it is active

**Vessel** (local containers) has a dedicated status endpoint:
`GET /api/v1/vessel/status` — engine type, version, socket, `connected`, and
`capabilities` (including Podman pods). Lifecycle and inventory live under
`/api/v1/vessel/containers` and `/api/v1/vessel/pods`; stats/logs stream on
`/ws/v1/vessel/containers/{id}/stats|logs`.

Deep links: run-as-user `GET /api/v1/auth/run-as-user`.

## Kubernetes metrics

`GET /api/v1/k8s/metrics?context=` runs `kubectl top nodes` and `kubectl top pods -A` (requires **metrics-server**). The K8s Overview page shows a live utilization panel.

## Metrics ingest (beyond JSON remote_write)

| Endpoint | Body | Purpose |
|----------|------|---------|
| `POST /api/v1/metrics/ingest/batch` | `{ "points": [ MetricsHistoryPoint, … ] }` | Append up to 500 history samples |
| `POST /api/v1/metrics/ingest/prometheus` | Prometheus text exposition | Parse `machina_host_*_percent` gauges into one history point |
| `POST /api/v1/metrics/ingest/remote-write` | Snappy protobuf (v1 `WriteRequest` or v2 `io.prometheus.write.v2.Request`) | Host triplet gauges → history ring only (not a TSDB) |
| `GET /api/v1/metrics/traces` | — | Recent HTTP spans (W3C trace IDs) for debugging |

## Prometheus

On scrape, the daemon exports:

- `machina_automation_last_tick_unix` — last successful automation worker tick
- `machina_run_as_user_active` — `1` when `[auth.run_as_user]` impersonation is active
- `machina_k8s_metrics_available` / `machina_k8s_last_probe_unix` — last `kubectl top` probe

See `contrib/grafana/machina-overview.json` (v4+) and `contrib/prometheus/alerts.yaml`.

## CLI

```bash
./machinactl integrations
```

## Run-as-user

Install and config: `docs/oidc-run-as-user.md`, `contrib/run-as-user/README.md`.

Build the setuid helper: `cargo build -p machina-run-as-user-helper --release`.
