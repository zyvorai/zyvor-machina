-- Opaque Keystone-shim tokens for the OpenStack-compatible API (X-Auth-Token).
CREATE TABLE IF NOT EXISTS os_tokens (
    id TEXT NOT NULL PRIMARY KEY,
    username TEXT NOT NULL,
    role TEXT NOT NULL,
    project_id TEXT NOT NULL,
    project_name TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_os_tokens_expires ON os_tokens (expires_at);
