-- Security Operations Center (SOC) — events, alerts, detection, SIEM integrations, playbooks

CREATE TABLE IF NOT EXISTS soc_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    occurred_at TIMESTAMPTZ NOT NULL,
    source TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'security',
    severity TEXT NOT NULL DEFAULT 'info',
    host_id UUID,
    vm_id UUID,
    actor TEXT,
    summary TEXT NOT NULL,
    ecs_json JSONB NOT NULL DEFAULT '{}',
    raw_ref JSONB NOT NULL DEFAULT '{}',
    dedupe_key TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_soc_events_dedupe
    ON soc_events(dedupe_key) WHERE dedupe_key IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_soc_events_occurred ON soc_events(occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_soc_events_severity ON soc_events(severity, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_soc_events_source ON soc_events(source, occurred_at DESC);

CREATE TABLE IF NOT EXISTS soc_detection_rules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    severity TEXT NOT NULL DEFAULT 'medium',
    query_json JSONB NOT NULL DEFAULT '{}',
    throttle_minutes INT NOT NULL DEFAULT 60,
    builtin BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS soc_alerts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    rule_id UUID REFERENCES soc_detection_rules(id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    severity TEXT NOT NULL DEFAULT 'medium',
    status TEXT NOT NULL DEFAULT 'open',
    assigned_to TEXT,
    dedupe_key TEXT NOT NULL,
    first_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    event_count INT NOT NULL DEFAULT 1,
    event_ids JSONB NOT NULL DEFAULT '[]',
    detail_json JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_soc_alerts_dedupe_open
    ON soc_alerts(dedupe_key) WHERE status IN ('open', 'acknowledged');
CREATE INDEX IF NOT EXISTS idx_soc_alerts_status ON soc_alerts(status, last_seen DESC);

CREATE TABLE IF NOT EXISTS soc_integrations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    integration_type TEXT NOT NULL,
    name TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    config_json JSONB NOT NULL DEFAULT '{}',
    last_success_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(integration_type, name)
);

CREATE TABLE IF NOT EXISTS soc_forwarder_cursors (
    integration_id UUID NOT NULL REFERENCES soc_integrations(id) ON DELETE CASCADE,
    cursor_kind TEXT NOT NULL DEFAULT 'events',
    last_occurred_at TIMESTAMPTZ,
    last_event_id UUID,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (integration_id, cursor_kind)
);

CREATE TABLE IF NOT EXISTS soc_event_exports (
    integration_id UUID NOT NULL REFERENCES soc_integrations(id) ON DELETE CASCADE,
    resource_type TEXT NOT NULL DEFAULT 'event',
    resource_id UUID NOT NULL,
    exported_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (integration_id, resource_type, resource_id)
);

CREATE TABLE IF NOT EXISTS soc_ingest_watermarks (
    source TEXT PRIMARY KEY,
    last_at TIMESTAMPTZ NOT NULL DEFAULT '1970-01-01T00:00:00Z'
);

CREATE TABLE IF NOT EXISTS soc_saved_hunts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    query_text TEXT NOT NULL,
    schedule_cron TEXT,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    last_run_at TIMESTAMPTZ,
    created_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS soc_playbooks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    trigger_json JSONB NOT NULL DEFAULT '{}',
    steps_json JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS soc_playbook_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    playbook_id UUID NOT NULL REFERENCES soc_playbooks(id) ON DELETE CASCADE,
    alert_id UUID REFERENCES soc_alerts(id) ON DELETE SET NULL,
    status TEXT NOT NULL DEFAULT 'running',
    step_results JSONB NOT NULL DEFAULT '[]',
    error TEXT,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_soc_playbook_runs_alert ON soc_playbook_runs(alert_id, started_at DESC);

INSERT INTO soc_ingest_watermarks (source, last_at) VALUES
    ('firewall_timeline', '1970-01-01T00:00:00Z'),
    ('audit_logs', '1970-01-01T00:00:00Z'),
    ('packetwolf', '1970-01-01T00:00:00Z'),
    ('platform_events', '1970-01-01T00:00:00Z')
ON CONFLICT (source) DO NOTHING;

INSERT INTO soc_detection_rules (id, name, description, enabled, severity, query_json, throttle_minutes, builtin)
VALUES
    ('a1000001-0001-4001-8001-000000000001', 'critical_anomaly',
     'PacketWolf critical or high severity anomaly', TRUE, 'high',
     '{"type":"match","match":{"source":"packetwolf","severity":["critical","high"]}}'::jsonb, 30, TRUE),
    ('a1000001-0001-4001-8001-000000000002', 'firewall_deny_spike',
     'Three or more firewall deny events in 15 minutes', TRUE, 'medium',
     '{"type":"threshold","match":{"source":"firewall","category":"firewall"},"window_minutes":15,"min_count":3}'::jsonb, 60, TRUE),
    ('a1000001-0001-4001-8001-000000000003', 'brute_force_ssh',
     'Repeated failed SSH or auth audit events', TRUE, 'high',
     '{"type":"threshold","match":{"source":"audit","ecs.event.action":["auth.failure","login.failed"]},"window_minutes":10,"min_count":5}'::jsonb, 120, TRUE),
    ('a1000001-0001-4001-8001-000000000004', 'new_admin_api_key',
     'New API key created by admin actor', TRUE, 'medium',
     '{"type":"match","match":{"source":"audit","ecs.event.action":["api_key.create","api_keys.create"]}}'::jsonb, 60, TRUE)
ON CONFLICT (name) DO NOTHING;

INSERT INTO soc_integrations (id, integration_type, name, enabled, config_json)
VALUES
    ('b2000002-0002-4002-8002-000000000001', 'splunk_hec', 'default', FALSE,
     '{"url":"","token":"","index":"machina","sourcetype_events":"machina:soc:ecs","sourcetype_alerts":"machina:soc:alert","host":""}'::jsonb),
    ('b2000002-0002-4002-8002-000000000002', 'elastic_bulk', 'default', FALSE,
     '{"url":"","api_key":"","index":"logs-machina.soc","pipeline":""}'::jsonb),
    ('b2000002-0002-4002-8002-000000000003', 'sentinel_dcr', 'default', FALSE,
     '{"dce_endpoint":"","dcr_immutable_id":"","stream_name":"","tenant_id":"","client_id":"","client_secret":""}'::jsonb),
    ('b2000002-0002-4002-8002-000000000004', 'qradar_rest', 'default', FALSE,
     '{"url":"","api_token":"","log_source_id":""}'::jsonb)
ON CONFLICT (integration_type, name) DO NOTHING;

INSERT INTO soc_playbooks (id, name, description, enabled, trigger_json, steps_json)
VALUES
    ('c3000003-0003-4003-8003-000000000001', 'notify_on_critical',
     'Webhook notify when critical SOC alert opens', TRUE,
     '{"min_severity":"high","rule_names":[]}'::jsonb,
     '[{"type":"webhook","url_from_setting":"soc_webhook_url","body":{"alert_id":"{{alert_id}}","title":"{{title}}","severity":"{{severity}}"}}]'::jsonb)
ON CONFLICT (name) DO NOTHING;
