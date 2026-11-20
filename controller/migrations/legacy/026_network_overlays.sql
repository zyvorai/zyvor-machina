-- Phase 26: NSX-class overlays + micro-segmentation (AI-392–411)

CREATE TABLE IF NOT EXISTS network_segments (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    tier TEXT NOT NULL DEFAULT 'tier1',
    cidr TEXT NOT NULL,
    east_west_default TEXT NOT NULL DEFAULT 'allow',
    firewall_profile TEXT,
    gitops_namespace TEXT NOT NULL DEFAULT 'network-segments',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS network_ipam_pools (
    id UUID PRIMARY KEY,
    segment_id UUID NOT NULL REFERENCES network_segments(id) ON DELETE CASCADE,
    cidr TEXT NOT NULL,
    gateway TEXT,
    dns_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    next_offset INT NOT NULL DEFAULT 2,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE networks ADD COLUMN IF NOT EXISTS segment_id UUID REFERENCES network_segments(id) ON DELETE SET NULL;

ALTER TABLE network_reservations ADD COLUMN IF NOT EXISTS pool_id UUID REFERENCES network_ipam_pools(id) ON DELETE SET NULL;
ALTER TABLE network_reservations ADD COLUMN IF NOT EXISTS hostname TEXT;

CREATE INDEX IF NOT EXISTS idx_networks_segment ON networks(segment_id);
CREATE INDEX IF NOT EXISTS idx_ipam_pools_segment ON network_ipam_pools(segment_id);

INSERT INTO network_segments (id, name, tier, cidr, east_west_default, firewall_profile, gitops_namespace)
VALUES
    ('a1000000-0000-4000-8000-000000000001', 'prod-tier1', 'tier1', '10.10.0.0/16', 'allow', 'ProductionServer', 'prod-segments'),
    ('a1000000-0000-4000-8000-000000000002', 'dmz-tier0', 'tier0', '172.16.0.0/24', 'deny', 'WebServer', 'dmz-segments')
ON CONFLICT (name) DO NOTHING;

INSERT INTO network_ipam_pools (id, segment_id, cidr, gateway, dns_json, next_offset)
VALUES
    ('b1000000-0000-4000-8000-000000000001', 'a1000000-0000-4000-8000-000000000001', '10.10.0.0/16', '10.10.0.1', '["10.10.0.1"]'::jsonb, 10),
    ('b1000000-0000-4000-8000-000000000002', 'a1000000-0000-4000-8000-000000000002', '172.16.0.0/24', '172.16.0.1', '["172.16.0.1"]'::jsonb, 10)
ON CONFLICT (id) DO NOTHING;
