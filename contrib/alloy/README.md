# Alloy examples

## `machina-metrics-forwarder.alloy`

Scrapes `GET /api/v1/prometheus` with a Bearer token and forwards to a **Prometheus-compatible** `remote_write` endpoint (Mimir, Cortex, Grafana Cloud).

Machina’s built-in `[metrics_history].remote_write_url` sends **JSON** (`MetricsHistoryPoint`), not Prometheus protobuf. Use one of:

| Goal | Approach |
|------|----------|
| JSON samples to custom ingest | `[metrics_history] remote_write_url` in `/etc/machina/config.toml` |
| Native Prometheus remote_write into Machina history | `POST /api/v1/metrics/ingest/remote-write` or `contrib/alloy/machina-remote-write-receiver.alloy` |
| Full Prometheus metrics in Mimir | `machina-metrics-forwarder.alloy` + `PROM_REMOTE_WRITE_URL` |
| Both | Daemon JSON POST + Alloy scrape → Mimir or loopback remote_write |

Environment:

```bash
export MACHINA_HOST=127.0.0.1:5092
export MACHINA_SCHEME=https
export MACHINA_TOKEN='mach_…'
export PROM_REMOTE_WRITE_URL='https://mimir.example/api/v1/push'
alloy run contrib/alloy/machina-metrics-forwarder.alloy
```
