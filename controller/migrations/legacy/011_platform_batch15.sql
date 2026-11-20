-- Batch 15: policy rules, project quotas, backup targets

CREATE TABLE IF NOT EXISTS policy_rules (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    rule_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS project_quotas (
    project TEXT PRIMARY KEY,
    max_vms INT NOT NULL DEFAULT 0,
    max_vcpu INT NOT NULL DEFAULT 0,
    max_memory_mib BIGINT NOT NULL DEFAULT 0,
    max_storage_gib BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS backup_targets (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL DEFAULT 'local',
    config_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO policy_rules (id, name, rule_json) VALUES
    ('00000000-0000-4000-8000-000000000001', 'production-ha-required',
     '{"when":{"tags_contains":"production"},"require":{"ha_enabled":true}}'::jsonb)
ON CONFLICT (name) DO NOTHING;
