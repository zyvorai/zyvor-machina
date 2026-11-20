-- Zeus Firewall Phase 15 (AI-162–166)

CREATE TABLE IF NOT EXISTS firewall_approvals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    target_kind TEXT NOT NULL DEFAULT 'host',
    target_id UUID NOT NULL,
    profile TEXT,
    plan_json JSONB NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'pending',
    requested_by TEXT NOT NULL,
    reviewed_by TEXT,
    review_note TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    reviewed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_fw_approvals_status ON firewall_approvals(status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_fw_approvals_target ON firewall_approvals(target_kind, target_id);

CREATE TABLE IF NOT EXISTS firewall_policy_sync_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    direction TEXT NOT NULL,
    policy_count INT NOT NULL DEFAULT 0,
    actor TEXT,
    detail_json JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
