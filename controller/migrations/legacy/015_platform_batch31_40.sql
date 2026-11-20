-- Batch 31-40: blueprints, finops, delete approval, ISO checksum

CREATE TABLE IF NOT EXISTS blueprints (
    id UUID PRIMARY KEY,
    cluster_id UUID NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    actions JSONB NOT NULL DEFAULT '[]',
    vm_ids UUID[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (cluster_id, name)
);

ALTER TABLE clusters ADD COLUMN IF NOT EXISTS require_vm_delete_approval BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS finops_vcpu_hour_usd DOUBLE PRECISION NOT NULL DEFAULT 0.02;
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS finops_gib_hour_usd DOUBLE PRECISION NOT NULL DEFAULT 0.005;
ALTER TABLE content_images ADD COLUMN IF NOT EXISTS checksum TEXT;

CREATE INDEX IF NOT EXISTS idx_blueprints_cluster ON blueprints(cluster_id);
