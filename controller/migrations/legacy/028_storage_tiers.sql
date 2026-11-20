-- Storage tiers + backup SLA stubs (Horizon phase 6 / AI-412–431)

CREATE TABLE IF NOT EXISTS storage_tiers (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    tier_class TEXT NOT NULL DEFAULT 'silver',
    iops_tier TEXT NOT NULL DEFAULT 'standard',
    replication TEXT NOT NULL DEFAULT 'local',
    snapshot_retention_days INT NOT NULL DEFAULT 7,
    backup_rpo_hours INT NOT NULL DEFAULT 24,
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE storage_pools ADD COLUMN IF NOT EXISTS tier_id UUID REFERENCES storage_tiers(id) ON DELETE SET NULL;

CREATE TABLE IF NOT EXISTS storage_backup_sla (
    id UUID PRIMARY KEY,
    pool_id UUID NOT NULL REFERENCES storage_pools(id) ON DELETE CASCADE,
    rpo_hours INT NOT NULL DEFAULT 24,
    rto_hours INT NOT NULL DEFAULT 4,
    retention_days INT NOT NULL DEFAULT 30,
    last_backup_at TIMESTAMPTZ,
    compliance_grade TEXT NOT NULL DEFAULT 'B',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (pool_id)
);

CREATE INDEX IF NOT EXISTS idx_storage_pools_tier ON storage_pools(tier_id);

INSERT INTO storage_tiers (id, name, tier_class, iops_tier, replication, snapshot_retention_days, backup_rpo_hours, description)
VALUES
    ('d1000000-0000-4000-8000-000000000001', 'gold-performance', 'gold', 'nvme', 'sync-mirror', 30, 4, 'Low-latency NVMe tier with synchronous mirror stub'),
    ('d1000000-0000-4000-8000-000000000002', 'silver-standard', 'silver', 'standard', 'local', 14, 24, 'Default production datastore tier'),
    ('d1000000-0000-4000-8000-000000000003', 'bronze-archive', 'bronze', 'hdd', 'local', 7, 72, 'Capacity-optimized cold tier')
ON CONFLICT (name) DO NOTHING;
