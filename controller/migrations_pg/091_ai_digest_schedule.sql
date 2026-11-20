-- Scheduled Zyra fleet digest. One row (id = 1). Off by default: nothing is sent until an admin turns it on.
-- weekday is ISO (1 = Monday .. 7 = Sunday) and only matters for the weekly frequency; hour_utc is 0..23.
CREATE TABLE ai_digest_schedule (
    id BIGINT PRIMARY KEY NOT NULL CHECK (id = 1),
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    frequency TEXT NOT NULL DEFAULT 'daily',
    hour_utc BIGINT NOT NULL DEFAULT 8,
    weekday BIGINT NOT NULL DEFAULT 1,
    last_sent_at TEXT,
    updated_by TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);
INSERT INTO ai_digest_schedule (id) VALUES (1);
