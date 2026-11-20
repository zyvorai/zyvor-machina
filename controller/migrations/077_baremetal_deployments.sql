-- Rendered installer configuration for a bare-metal server that is being deployed
-- (autoinstall user-data or kickstart, containing a one-time enrollment token).
-- The row is deleted when the host joins, the deploy fails, or the server is released.
CREATE TABLE baremetal_deployments (
    server_id TEXT PRIMARY KEY NOT NULL REFERENCES baremetal_servers(id) ON DELETE CASCADE,
    os TEXT NOT NULL,
    token_hash TEXT NOT NULL DEFAULT '',
    user_data TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
