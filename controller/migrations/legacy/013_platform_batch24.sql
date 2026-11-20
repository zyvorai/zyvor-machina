-- Batch 24-29: guest tools, application groups, health metadata

ALTER TABLE vms ADD COLUMN IF NOT EXISTS guest_tools_status TEXT NOT NULL DEFAULT 'unknown';
ALTER TABLE vms ADD COLUMN IF NOT EXISTS guest_ip TEXT;
ALTER TABLE vms ADD COLUMN IF NOT EXISTS guest_hostname TEXT;
ALTER TABLE vms ADD COLUMN IF NOT EXISTS os_family TEXT;

CREATE TABLE IF NOT EXISTS application_groups (
    id UUID PRIMARY KEY,
    cluster_id UUID NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(cluster_id, name)
);

CREATE TABLE IF NOT EXISTS application_group_vms (
    group_id UUID NOT NULL REFERENCES application_groups(id) ON DELETE CASCADE,
    vm_id UUID NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, vm_id)
);

CREATE INDEX IF NOT EXISTS idx_vms_guest_tools ON vms(guest_tools_status);
