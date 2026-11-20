-- Zeus AI redesign: multi-provider, routing, agents, prompts, memory, actions, marketplace

ALTER TABLE clusters ADD COLUMN IF NOT EXISTS zeus_multi_provider BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS zeus_agents_enabled BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS zeus_ambient_ux BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS zeus_memory_enabled BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS zeus_memory_team_scope BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS zeus_memory_project_scope BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS zeus_memory_retention_days INT NOT NULL DEFAULT 90;
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS zeus_air_gap_llm BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE IF NOT EXISTS ai_providers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'openai',
    base_url TEXT NOT NULL DEFAULT '',
    org_id TEXT NOT NULL DEFAULT '',
    deployment_name TEXT NOT NULL DEFAULT '',
    api_key_encrypted TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS ai_models (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    provider_id UUID NOT NULL REFERENCES ai_providers(id) ON DELETE CASCADE,
    model_id TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    capabilities_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    context_window INT NOT NULL DEFAULT 128000,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    UNIQUE (provider_id, model_id)
);

CREATE TABLE IF NOT EXISTS ai_user_preferences (
    user_id TEXT PRIMARY KEY,
    default_provider_id UUID REFERENCES ai_providers(id) ON DELETE SET NULL,
    default_model_id UUID REFERENCES ai_models(id) ON DELETE SET NULL,
    default_agent TEXT NOT NULL DEFAULT 'auto',
    memory_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS ai_routing_rules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_class TEXT NOT NULL UNIQUE,
    provider_id UUID REFERENCES ai_providers(id) ON DELETE SET NULL,
    model_id UUID REFERENCES ai_models(id) ON DELETE SET NULL,
    priority INT NOT NULL DEFAULT 0,
    enabled BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE TABLE IF NOT EXISTS ai_prompts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    scope TEXT NOT NULL DEFAULT 'personal',
    owner_id TEXT NOT NULL DEFAULT '',
    team_id TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    tags JSONB NOT NULL DEFAULT '[]'::jsonb,
    agent_id TEXT NOT NULL DEFAULT 'auto',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS ai_memory_entries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    scope TEXT NOT NULL DEFAULT 'user',
    subject_kind TEXT NOT NULL DEFAULT 'conversation',
    subject_id TEXT NOT NULL DEFAULT '',
    owner_id TEXT NOT NULL DEFAULT '',
    project_id TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL,
    metadata_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS ai_conversations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id TEXT NOT NULL,
    project_id TEXT NOT NULL DEFAULT '',
    agent_id TEXT NOT NULL DEFAULT 'auto',
    summary TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS ai_actions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source TEXT NOT NULL DEFAULT 'zeus',
    action_type TEXT NOT NULL,
    label TEXT NOT NULL,
    review TEXT NOT NULL DEFAULT '',
    risk TEXT NOT NULL DEFAULT 'Review required',
    object_ref JSONB NOT NULL DEFAULT '{}'::jsonb,
    status TEXT NOT NULL DEFAULT 'pending',
    requested_by TEXT NOT NULL DEFAULT '',
    approved_by TEXT,
    executed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS ai_agent_plugins (
    slug TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    agent_id TEXT NOT NULL,
    config_schema_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    installed BOOLEAN NOT NULL DEFAULT FALSE,
    published_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ai_prompts_owner ON ai_prompts(owner_id);
CREATE INDEX IF NOT EXISTS idx_ai_memory_owner ON ai_memory_entries(owner_id);
CREATE INDEX IF NOT EXISTS idx_ai_actions_status ON ai_actions(status);

-- Seed default routing rules (provider/model resolved at runtime from default provider)
INSERT INTO ai_routing_rules (task_class, priority, enabled) VALUES
    ('infrastructure', 10, TRUE),
    ('code_generation', 20, TRUE),
    ('security_analysis', 30, TRUE),
    ('research', 40, TRUE),
    ('long_context', 50, TRUE),
    ('fast_local', 60, TRUE)
ON CONFLICT (task_class) DO NOTHING;

-- Seed agent marketplace catalog
INSERT INTO ai_agent_plugins (slug, name, description, agent_id, installed) VALUES
    ('aws-expert', 'AWS Expert', 'Cloud architecture and AWS service guidance', 'architect', FALSE),
    ('azure-expert', 'Azure Expert', 'Azure landing zones and NSG guidance', 'architect', FALSE),
    ('gcp-expert', 'GCP Expert', 'GCP networking and GKE guidance', 'architect', FALSE),
    ('linux-expert', 'Linux Expert', 'Host tuning and systemd diagnostics', 'sre', FALSE),
    ('kubernetes-expert', 'Kubernetes Expert', 'Cluster ops and workload placement', 'kubernetes', FALSE),
    ('terraform-expert', 'Terraform Expert', 'IaC generation and module guidance', 'architect', FALSE),
    ('finops-expert', 'FinOps Expert', 'Cost optimization and chargeback', 'cost', FALSE),
    ('security-expert', 'Security Expert', 'Threat hunting and compliance', 'security', FALSE)
ON CONFLICT (slug) DO NOTHING;

-- Migrate legacy clusters BYOK into default provider when present
INSERT INTO ai_providers (name, kind, api_key_encrypted, is_default, enabled)
SELECT 'Default provider', COALESCE(NULLIF(ai_provider, ''), 'openai'), COALESCE(ai_api_key, ''), TRUE, ai_enabled
FROM clusters
WHERE COALESCE(ai_api_key, '') <> ''
  AND NOT EXISTS (SELECT 1 FROM ai_providers WHERE is_default = TRUE)
ORDER BY created_at
LIMIT 1;

INSERT INTO ai_models (provider_id, model_id, display_name, enabled)
SELECT p.id, COALESCE(NULLIF(c.ai_model, ''), 'gpt-4o-mini'), COALESCE(NULLIF(c.ai_model, ''), 'gpt-4o-mini'), TRUE
FROM ai_providers p
CROSS JOIN (SELECT ai_model FROM clusters ORDER BY created_at LIMIT 1) c
WHERE p.is_default = TRUE
  AND NOT EXISTS (SELECT 1 FROM ai_models m WHERE m.provider_id = p.id);
