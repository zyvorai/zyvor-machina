-- Enterprise security stubs — Vault/MFA inventory, air-gap bundles (Horizon phase 28 / AI-432–451)

CREATE TABLE IF NOT EXISTS vault_providers (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    provider_type TEXT NOT NULL DEFAULT 'hashicorp',
    address TEXT NOT NULL DEFAULT '',
    namespace TEXT NOT NULL DEFAULT 'machina',
    status TEXT NOT NULL DEFAULT 'disconnected',
    last_sync_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS mfa_policies (
    id UUID PRIMARY KEY,
    role_name TEXT NOT NULL UNIQUE,
    method TEXT NOT NULL DEFAULT 'webauthn',
    required BOOLEAN NOT NULL DEFAULT false,
    grace_days INT NOT NULL DEFAULT 7,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS air_gap_bundles (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    checksum TEXT NOT NULL DEFAULT '',
    manifest_json JSONB NOT NULL DEFAULT '{}',
    size_bytes BIGINT NOT NULL DEFAULT 0,
    exported_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_air_gap_bundles_exported ON air_gap_bundles(exported_at DESC);

INSERT INTO vault_providers (id, name, provider_type, address, namespace, status)
VALUES
    ('e1000000-0000-4000-8000-000000000001', 'local-config', 'file', '', 'machina', 'active'),
    ('e1000000-0000-4000-8000-000000000002', 'vault-stub', 'hashicorp', 'https://vault.example:8200', 'machina', 'disconnected')
ON CONFLICT (name) DO NOTHING;

INSERT INTO mfa_policies (id, role_name, method, required, grace_days)
VALUES
    ('e2000000-0000-4000-8000-000000000001', 'admin', 'webauthn', false, 7),
    ('e2000000-0000-4000-8000-000000000002', 'operator', 'totp', false, 14),
    ('e2000000-0000-4000-8000-000000000003', 'viewer', 'totp', false, 30)
ON CONFLICT (role_name) DO NOTHING;
