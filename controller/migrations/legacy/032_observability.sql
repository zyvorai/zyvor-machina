-- Observability — SLO policies + API trace spans (Horizon phase 31 / AI-492–511)

CREATE TABLE IF NOT EXISTS slo_policies (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    target TEXT NOT NULL,
    objective_pct NUMERIC NOT NULL DEFAULT 99.9,
    window_hours INT NOT NULL DEFAULT 720,
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS api_trace_spans (
    id UUID PRIMARY KEY,
    method TEXT NOT NULL,
    path TEXT NOT NULL,
    status_code INT NOT NULL,
    duration_ms INT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_api_trace_recorded ON api_trace_spans(recorded_at DESC);
CREATE INDEX IF NOT EXISTS idx_api_trace_path ON api_trace_spans(path, recorded_at DESC);

INSERT INTO slo_policies (id, name, target, objective_pct, window_hours, description)
VALUES
    ('a1000000-0000-4000-8000-000000000001', 'api-availability', 'controller /api/v1/*', 99.5, 720, 'HTTP 2xx/3xx rate for platform API'),
    ('a1000000-0000-4000-8000-000000000002', 'task-success', 'platform tasks', 98.0, 168, 'Completed vs failed task ratio'),
    ('a1000000-0000-4000-8000-000000000003', 'host-availability', 'online hosts', 99.0, 720, 'Hosts reporting online vs registered')
ON CONFLICT (name) DO NOTHING;

ALTER TABLE ops_runbook_catalog ADD COLUMN IF NOT EXISTS last_triggered_at TIMESTAMPTZ;
