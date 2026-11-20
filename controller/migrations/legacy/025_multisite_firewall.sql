-- Zeus Firewall Phase 24 — multi-site federation (AI-332–351)

CREATE TABLE IF NOT EXISTS firewall_sites (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    region TEXT NOT NULL DEFAULT 'local',
    role TEXT NOT NULL DEFAULT 'primary',
    gitops_namespace TEXT NOT NULL DEFAULT 'default',
    lockdown_enabled BOOLEAN NOT NULL DEFAULT false,
    geo_fence TEXT,
    dr_pair TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS firewall_site_policies (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    site_id UUID NOT NULL REFERENCES firewall_sites(id) ON DELETE CASCADE,
    policy_name TEXT NOT NULL,
    profile TEXT NOT NULL DEFAULT 'ProductionServer',
    spec_yaml TEXT NOT NULL DEFAULT '',
    geo_fence TEXT,
    dr_pair TEXT,
    stretch_deny BOOLEAN NOT NULL DEFAULT false,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (site_id, policy_name)
);

CREATE TABLE IF NOT EXISTS firewall_site_drift (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    site_id UUID NOT NULL REFERENCES firewall_sites(id) ON DELETE CASCADE,
    peer_site_id UUID REFERENCES firewall_sites(id) ON DELETE SET NULL,
    drift_json JSONB NOT NULL DEFAULT '{}',
    captured_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS firewall_site_timeline (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    site_id UUID NOT NULL REFERENCES firewall_sites(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    detail_json JSONB NOT NULL DEFAULT '{}',
    actor TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_fw_site_policies_site ON firewall_site_policies(site_id);
CREATE INDEX IF NOT EXISTS idx_fw_site_drift_site ON firewall_site_drift(site_id, captured_at DESC);

INSERT INTO firewall_sites (name, region, role, gitops_namespace, dr_pair)
VALUES ('primary-local', 'local', 'primary', 'site-primary', 'dr-replica')
ON CONFLICT (name) DO NOTHING;

INSERT INTO firewall_sites (name, region, role, gitops_namespace, dr_pair, lockdown_enabled)
VALUES ('dr-replica', 'dr', 'replica', 'site-dr', 'primary-local', false)
ON CONFLICT (name) DO NOTHING;
