-- Platform plugin marketplace (Horizon slice — inventory + install stubs)

CREATE TABLE IF NOT EXISTS platform_plugins (
    id UUID PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'integration',
    description TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '1.0.0',
    author TEXT NOT NULL DEFAULT 'Zyvor',
    featured BOOLEAN NOT NULL DEFAULT FALSE,
    installed BOOLEAN NOT NULL DEFAULT FALSE,
    config_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_platform_plugins_category ON platform_plugins(category, featured);

INSERT INTO platform_plugins (id, slug, name, category, description, version, author, featured, installed)
VALUES
    ('c2000000-0000-4000-8000-000000000001', 'guestkit', 'GuestKit', 'automation', 'Guest health checks, job runner, and in-VM automation bridge.', '1.0.0', 'Zyvor', TRUE, FALSE),
    ('c2000000-0000-4000-8000-000000000002', 'packetwolf', 'PacketWolf', 'observability', 'Flow capture, anomaly hints, and firewall activity correlation.', '1.0.0', 'Zyvor', TRUE, FALSE),
    ('c2000000-0000-4000-8000-000000000003', 'hypersdk', 'HyperSDK', 'migration', 'P2V migration assistant and Windows VM discovery.', '1.0.0', 'Zyvor', TRUE, FALSE),
    ('c2000000-0000-4000-8000-000000000004', 'zeus-firewall', 'Zeus Firewall', 'security', 'Fleet machine shield, profiles, and connectivity simulation.', '1.0.0', 'Zyvor', TRUE, TRUE),
    ('c2000000-0000-4000-8000-000000000005', 'kubevirt-bridge', 'KubeVirt Bridge', 'kubernetes', 'Export libvirt VMs and qcow2 bundles for Kubernetes.', '1.0.0', 'Zyvor', FALSE, FALSE),
    ('c2000000-0000-4000-8000-000000000006', 'network-overlay', 'Network Overlay', 'networking', 'NSX-class segments, IPAM pools, and micro-segmentation stubs.', '1.0.0', 'Zyvor', FALSE, TRUE)
ON CONFLICT (slug) DO NOTHING;
