-- Zeus Machine Firewall (AI-142+)

CREATE TABLE IF NOT EXISTS firewall_profiles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    spec_json JSONB NOT NULL DEFAULT '{}',
    builtin BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS firewall_posture_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    target_kind TEXT NOT NULL,
    target_id UUID NOT NULL,
    checksum TEXT NOT NULL,
    posture_json JSONB NOT NULL DEFAULT '{}',
    captured_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_fw_posture_target ON firewall_posture_snapshots(target_kind, target_id, captured_at DESC);

CREATE TABLE IF NOT EXISTS firewall_checkpoints (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    target_kind TEXT NOT NULL,
    target_id UUID NOT NULL,
    label TEXT NOT NULL DEFAULT 'rollback',
    adapter_state JSONB NOT NULL DEFAULT '{}',
    created_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS firewall_temporary_rules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    target_kind TEXT NOT NULL,
    target_id UUID NOT NULL,
    source_cidr TEXT NOT NULL,
    dest_port INT NOT NULL,
    protocol TEXT NOT NULL DEFAULT 'tcp',
    reason TEXT NOT NULL,
    owner TEXT,
    approval_id UUID,
    expires_at TIMESTAMPTZ NOT NULL,
    applied BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_fw_temp_expiry ON firewall_temporary_rules(expires_at) WHERE applied = true;

CREATE TABLE IF NOT EXISTS firewall_timeline (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    target_kind TEXT NOT NULL,
    target_id UUID NOT NULL,
    kind TEXT NOT NULL,
    summary TEXT NOT NULL,
    detail_json JSONB NOT NULL DEFAULT '{}',
    actor TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_fw_timeline_target ON firewall_timeline(target_kind, target_id, created_at DESC);

CREATE TABLE IF NOT EXISTS firewall_policies (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    spec_yaml TEXT NOT NULL,
    workspace_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO firewall_profiles (name, display_name, spec_json, builtin) VALUES
    ('Public', 'Public', '{"default_inbound":"deny"}', true),
    ('Private', 'Private', '{"default_inbound":"allow"}', true),
    ('ProductionServer', 'Production Server', '{"default_inbound":"deny"}', true),
    ('DatabaseServer', 'Database Server', '{"default_inbound":"deny"}', true),
    ('WebServer', 'Web Server', '{"default_inbound":"deny"}', true),
    ('KubernetesNode', 'Kubernetes Node', '{"default_inbound":"deny"}', true),
    ('StorageNode', 'Storage Node', '{"default_inbound":"deny"}', true),
    ('ManagementNode', 'Management Node', '{"default_inbound":"deny"}', true),
    ('DevelopmentVm', 'Development VM', '{"default_inbound":"allow"}', true),
    ('LockedDown', 'Locked Down', '{"default_inbound":"deny","default_outbound":"deny"}', true),
    ('EmergencyIsolation', 'Emergency Isolation', '{"default_inbound":"deny","default_outbound":"deny","stealth":"emergency"}', true)
ON CONFLICT (name) DO NOTHING;
