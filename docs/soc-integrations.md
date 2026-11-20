# SOC integrations (SIEM export)

Machina's Security Operations Center normalizes audit, firewall, eBPF, and platform events into ECS-shaped records, runs detection rules, and forwards to external SIEMs.

## Splunk HEC (primary)

1. Create an HTTP Event Collector token in Splunk (`Settings → Data inputs → HTTP Event Collector`).
2. In Machina: **Platform → SOC → Integrations**
   - **HEC URL**: `https://splunk-host:8088` (or full collector path)
   - **Token**: HEC token
   - **Index**: e.g. `machina`
   - Enable forwarder and **Save**, then **Test connection**.

### Sourcetypes

| Sourcetype | Content |
|------------|---------|
| `machina:soc:ecs` | Normalized SOC events |
| `machina:soc:alert` | SOC detection alerts |
| `machina:soc:health` | Connectivity test events |

### Example SPL

```spl
index=machina sourcetype="machina:soc:alert"
| stats count by machina.severity, message

index=machina sourcetype="machina:soc:ecs" event.dataset="machina.bpf"
| where event.severity>=70
| table _time, message, machina.source, host
```

## Elastic Security

Configure via API:

`PATCH /api/v1/soc/integrations/elastic_bulk`

```json
{
  "enabled": true,
  "config_json": {
    "url": "https://elastic:9200",
    "api_key": "<api-key>",
    "index": "logs-machina.soc"
  }
}
```

Test: `POST /api/v1/soc/integrations/elastic_bulk/test`

## JSON webhook (local collector)

Replaces the removed Microsoft Sentinel (Azure Monitor DCR) exporter. Machina POSTs batches of
ECS events as NDJSON to any endpoint you run: Vector, Fluent Bit, a Loki push proxy, Wazuh,
Graylog, or your own script. Nothing leaves the site unless you point it elsewhere.

`PATCH /api/v1/soc/integrations/webhook_json`

```json
{
  "enabled": true,
  "config_json": {
    "url": "http://10.0.0.5:8686/events",
    "bearer_token": "<optional>",
    "insecure_tls": false
  }
}
```

Test: `POST /api/v1/soc/integrations/webhook_json/test`. An existing, configured `sentinel_dcr`
row is no longer forwarded (a warning is logged); switch it to `webhook_json` or `elastic_bulk`.

## IBM QRadar

`PATCH /api/v1/soc/integrations/qradar_rest`

```json
{
  "enabled": true,
  "config_json": {
    "url": "https://qradar",
    "api_token": "<sec-token>",
    "log_source_id": 123
  }
}
```

## Replay

`POST /api/v1/soc/forward/replay?hours=24` re-exports events not yet marked exported (admin only).

## ECS fields (subset)

| Field | Description |
|-------|-------------|
| `@timestamp` | Event time |
| `event.dataset` | `machina.firewall`, `machina.audit`, `machina.bpf`, `machina.platform` |
| `event.severity` | Numeric 10–90 |
| `event.action` | Source-specific action |
| `message` | Human summary |
| `machina.severity` | `low` / `medium` / `high` / `critical` |
| `machina.source` | Ingest source key |

## SOAR playbooks

Built-in playbook `notify_on_critical` runs on high/critical alerts (webhook step). Configure the webhook URL under **Platform → SOC → Playbooks → Global webhook**, set `MACHINA_SOC_WEBHOOK_URL` on the controller host, or edit `soc_playbooks` in the database.

## Playbooks & alert triage (v2)

- **Alerts tab:** select an alert to open the detail panel (linked events, MITRE tags from ECS, assignee, Ack/Close).
- **Playbooks tab:** create/edit playbooks with webhook or notify steps; minimum severity trigger; global webhook URL for `url_from_setting` steps.
