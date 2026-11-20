-- Batch 12: VM lifecycle phases, host join validation, reconcile metadata

ALTER TABLE vms ADD COLUMN IF NOT EXISTS lifecycle_phase TEXT NOT NULL DEFAULT 'idle';
ALTER TABLE vms ADD COLUMN IF NOT EXISTS last_error TEXT NOT NULL DEFAULT '';
ALTER TABLE vms ADD COLUMN IF NOT EXISTS managed BOOLEAN NOT NULL DEFAULT TRUE;

ALTER TABLE hosts ADD COLUMN IF NOT EXISTS validation_status TEXT NOT NULL DEFAULT 'pending';
ALTER TABLE hosts ADD COLUMN IF NOT EXISTS validation_report JSONB NOT NULL DEFAULT '[]'::jsonb;

CREATE INDEX IF NOT EXISTS idx_vms_lifecycle_phase ON vms(lifecycle_phase);
CREATE INDEX IF NOT EXISTS idx_hosts_validation_status ON hosts(validation_status);
