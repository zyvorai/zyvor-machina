-- Platform batch 8: OIDC flow, leader election, IPMI fence, webhook retries

ALTER TABLE clusters ADD COLUMN IF NOT EXISTS oidc_client_secret TEXT NOT NULL DEFAULT '';
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS oidc_redirect_uri TEXT NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS oidc_states (
    state TEXT PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE hosts ADD COLUMN IF NOT EXISTS fence_method TEXT NOT NULL DEFAULT 'shell';
ALTER TABLE hosts ADD COLUMN IF NOT EXISTS ipmi_address TEXT NOT NULL DEFAULT '';
ALTER TABLE hosts ADD COLUMN IF NOT EXISTS ipmi_username TEXT NOT NULL DEFAULT '';
ALTER TABLE hosts ADD COLUMN IF NOT EXISTS ipmi_password TEXT NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS controller_leadership (
    id INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    holder_id TEXT NOT NULL DEFAULT '',
    lease_until TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO controller_leadership (id, holder_id, lease_until)
VALUES (1, '', NOW())
ON CONFLICT (id) DO NOTHING;

CREATE TABLE IF NOT EXISTS webhook_deliveries (
    id UUID PRIMARY KEY,
    webhook_id UUID,
    url TEXT NOT NULL,
    secret TEXT NOT NULL DEFAULT '',
    body JSONB NOT NULL,
    event_kind TEXT NOT NULL DEFAULT '',
    attempts INT NOT NULL DEFAULT 0,
    max_attempts INT NOT NULL DEFAULT 5,
    next_retry_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    status TEXT NOT NULL DEFAULT 'pending',
    last_error TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_webhook_deliveries_pending
    ON webhook_deliveries(next_retry_at) WHERE status = 'pending';
CREATE INDEX IF NOT EXISTS idx_oidc_states_created ON oidc_states(created_at);
