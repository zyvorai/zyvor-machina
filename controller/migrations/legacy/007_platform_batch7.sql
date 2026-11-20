-- Platform batch 7: tags, OIDC, CPU compat, restore tracking, indexes

ALTER TABLE vms ADD COLUMN IF NOT EXISTS tags TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE hosts ADD COLUMN IF NOT EXISTS tags TEXT[] NOT NULL DEFAULT '{}';

ALTER TABLE clusters ADD COLUMN IF NOT EXISTS oidc_enabled BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS oidc_issuer TEXT NOT NULL DEFAULT '';
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS oidc_client_id TEXT NOT NULL DEFAULT '';
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS cpu_compat_matrix JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE backup_records ADD COLUMN IF NOT EXISTS restore_status TEXT NOT NULL DEFAULT '';

ALTER TABLE notification_outbox ADD COLUMN IF NOT EXISTS delivered_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_tasks_operation ON tasks(operation);
CREATE INDEX IF NOT EXISTS idx_audit_actor ON audit_logs(actor);
CREATE INDEX IF NOT EXISTS idx_events_kind ON events(kind);
CREATE INDEX IF NOT EXISTS idx_vms_tags ON vms USING GIN(tags);
CREATE INDEX IF NOT EXISTS idx_hosts_tags ON hosts USING GIN(tags);
