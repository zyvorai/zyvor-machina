-- EC2-style foundations: key/value tags on any resource.
-- resource_id is the resource's UUID in simple (32 hex, lowercase) form, so one table serves every resource type.
CREATE TABLE IF NOT EXISTS resource_tags (
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (resource_type, resource_id, key)
);
CREATE INDEX IF NOT EXISTS idx_resource_tags_key ON resource_tags(key, value);

-- Instance type: remember which flavor a machine was launched with / last changed to (EC2's instance type).
ALTER TABLE vms ADD COLUMN flavor_id TEXT;
