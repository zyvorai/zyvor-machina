# Machina metrics JSON ingest

`machina-metrics-ingest.py` is a minimal receiver for `[metrics_history].remote_write_url`.

The daemon also supports:

- `POST /api/v1/metrics/ingest/batch` — JSON array of `MetricsHistoryPoint`
- `POST /api/v1/metrics/ingest/prometheus` — Prometheus text with `machina_host_*_percent` gauges
- `POST /api/v1/metrics/ingest/remote-write` — Snappy protobuf (remote_write **1.0** or **2.0** via `Content-Type` proto parameter)

Test remote_write:

```bash
chmod +x contrib/ingest/test-remote-write.sh
MACHINA_TOKEN=mach_… ./contrib/ingest/test-remote-write.sh
```

```bash
python3 contrib/ingest/machina-metrics-ingest.py --port 9099
```

```toml
[metrics_history]
remote_write_url = "http://127.0.0.1:9099/ingest"
```

Each POST appends one JSON line (`MetricsHistoryPoint`). For production, use your own service or Grafana Alloy — see `contrib/alloy/README.md` for Prometheus/Mimir scrape.

**Scope:** `remote-write` and `ingest/prometheus` on the daemon only store `machina_host_*_percent` into the history ring, not arbitrary time series. See `docs/guides/observability.md`.
