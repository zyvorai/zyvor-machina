-- Operations runbooks + compliance showback (Horizon phase 29 / AI-452–471)

CREATE TABLE IF NOT EXISTS ops_runbook_catalog (
    id UUID PRIMARY KEY,
    incident TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'incident',
    severity TEXT NOT NULL DEFAULT 'medium',
    auto_trigger TEXT,
    enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS ops_runbook_executions (
    id UUID PRIMARY KEY,
    incident TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'completed',
    steps_json JSONB NOT NULL DEFAULT '[]',
    actor TEXT,
    summary TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ops_runbook_exec_created ON ops_runbook_executions(created_at DESC);

CREATE TABLE IF NOT EXISTS ops_showback_snapshots (
    id UUID PRIMARY KEY,
    project_name TEXT NOT NULL,
    cost_usd NUMERIC NOT NULL DEFAULT 0,
    compliance_grade TEXT NOT NULL DEFAULT 'B',
    vm_count INT NOT NULL DEFAULT 0,
    captured_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ops_showback_project ON ops_showback_snapshots(project_name, captured_at DESC);

INSERT INTO ops_runbook_catalog (id, incident, title, category, severity, auto_trigger)
VALUES
    ('f1000000-0000-4000-8000-000000000001', 'host_offline', 'Host offline recovery', 'incident', 'high', 'host.state=offline'),
    ('f1000000-0000-4000-8000-000000000002', 'backup_failed', 'Backup failure triage', 'incident', 'medium', 'task.failed:backup'),
    ('f1000000-0000-4000-8000-000000000003', 'migration_failed', 'Migration failure triage', 'incident', 'medium', 'task.failed:migrate'),
    ('f1000000-0000-4000-8000-000000000004', 'firewall_drift', 'Firewall drift remediation', 'compliance', 'high', 'zeus.drift_detected'),
    ('f1000000-0000-4000-8000-000000000005', 'storage_full', 'Storage pool capacity', 'capacity', 'critical', 'storage.used_pct>85')
ON CONFLICT (incident) DO NOTHING;
