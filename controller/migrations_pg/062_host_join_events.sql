-- What happened while a host joined: one row per step, shown as a live log in the web UI.
CREATE TABLE host_join_events (
    id TEXT PRIMARY KEY,
    token TEXT NOT NULL,
    host_id TEXT,
    level TEXT NOT NULL,
    step TEXT NOT NULL,
    message TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_host_join_events_token ON host_join_events (token, created_at);
CREATE INDEX idx_host_join_events_host ON host_join_events (host_id, created_at);
