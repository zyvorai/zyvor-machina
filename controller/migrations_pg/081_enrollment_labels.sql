-- Labels an enrollment token carries: where the host sits and what to set up on it once it
-- has joined and passed validation (existing networks and storage pools, by name).
ALTER TABLE enrollment_tokens ADD COLUMN site TEXT NOT NULL DEFAULT '';
ALTER TABLE enrollment_tokens ADD COLUMN rack TEXT NOT NULL DEFAULT '';
ALTER TABLE enrollment_tokens ADD COLUMN tags TEXT NOT NULL DEFAULT '[]';
ALTER TABLE enrollment_tokens ADD COLUMN networks TEXT NOT NULL DEFAULT '[]';
ALTER TABLE enrollment_tokens ADD COLUMN storage_pools TEXT NOT NULL DEFAULT '[]';
CREATE TABLE host_post_join (
    host_id UUID PRIMARY KEY NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    networks TEXT NOT NULL DEFAULT '[]',
    storage_pools TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);
