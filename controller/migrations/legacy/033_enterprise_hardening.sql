-- Enterprise hardening — Vault sync, MFA enrollments, FIPS matrix, tenant isolation (Phase 32 / AI-512–531)

CREATE TABLE IF NOT EXISTS vault_sync_runs (
    id UUID PRIMARY KEY,
    provider_id UUID NOT NULL REFERENCES vault_providers(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    message TEXT NOT NULL DEFAULT '',
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_vault_sync_runs_provider ON vault_sync_runs(provider_id, recorded_at DESC);

CREATE TABLE IF NOT EXISTS mfa_enrollments (
    id UUID PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    method TEXT NOT NULL DEFAULT 'webauthn',
    enrolled_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS fips_crypto_profiles (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    tls_min_version TEXT NOT NULL DEFAULT '1.2',
    fips_mode TEXT NOT NULL DEFAULT 'disabled',
    cipher_suites TEXT NOT NULL DEFAULT 'system-default',
    notes TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS tenant_isolation_policies (
    id UUID PRIMARY KEY,
    project_name TEXT NOT NULL UNIQUE,
    network_isolation TEXT NOT NULL DEFAULT 'shared',
    max_vms INT NOT NULL DEFAULT 0,
    max_storage_gib INT NOT NULL DEFAULT 0,
    enforce_quotas BOOLEAN NOT NULL DEFAULT false,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO fips_crypto_profiles (id, name, tls_min_version, fips_mode, cipher_suites, notes)
VALUES
    ('f1000000-0000-4000-8000-000000000001', 'platform-default', '1.2', 'disabled', 'TLS_AES_128_GCM_SHA256,TLS_AES_256_GCM_SHA384', 'Controller TLS via system OpenSSL — FIPS module not selected'),
    ('f1000000-0000-4000-8000-000000000002', 'fips-ready', '1.2', 'required', 'TLS_AES_256_GCM_SHA384', 'Target profile for FIPS 140-3 validated module rollout')
ON CONFLICT (name) DO NOTHING;

INSERT INTO tenant_isolation_policies (id, project_name, network_isolation, max_vms, max_storage_gib, enforce_quotas)
VALUES
    ('10000000-0000-4000-8000-000000000001', 'default', 'shared', 0, 0, false),
    ('10000000-0000-4000-8000-000000000002', 'production', 'segmented', 50, 10240, true)
ON CONFLICT (project_name) DO NOTHING;

INSERT INTO mfa_enrollments (id, username, method)
SELECT '20000000-0000-4000-8000-000000000001', 'admin', 'webauthn'
WHERE EXISTS (SELECT 1 FROM users WHERE username = 'admin')
ON CONFLICT (username) DO NOTHING;
