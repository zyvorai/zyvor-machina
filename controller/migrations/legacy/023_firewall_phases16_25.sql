-- Zeus Firewall phases 16-25 (AI-172–371)

ALTER TABLE clusters ADD COLUMN IF NOT EXISTS firewall_approval_sla_hours INT NOT NULL DEFAULT 72;

CREATE TABLE IF NOT EXISTS firewall_k8s_apply_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    namespace TEXT NOT NULL,
    profile TEXT NOT NULL,
    backend TEXT NOT NULL,
    actor TEXT,
    detail_json JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS firewall_cloud_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    provider TEXT NOT NULL,
    summary TEXT NOT NULL,
    inventory_json JSONB NOT NULL DEFAULT '{}',
    captured_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_fw_cloud_provider ON firewall_cloud_snapshots(provider, captured_at DESC);

CREATE TABLE IF NOT EXISTS firewall_connectivity_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    target_id UUID NOT NULL,
    profile TEXT NOT NULL,
    matrix_json JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS firewall_policy_reconcile_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    policies_synced INT NOT NULL DEFAULT 0,
    detail_json JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
