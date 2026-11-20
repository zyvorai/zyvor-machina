-- Day-2: notification channels. Threshold alerts (and other events) previously only landed
-- in the in-app notification_outbox / signed webhooks. Channels let operators route them to
-- Slack (incoming webhook) or email (SMTP), with a per-channel event filter.
CREATE TABLE IF NOT EXISTS notification_channels (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,                 -- 'slack' | 'email' | 'webhook'
    target TEXT NOT NULL,               -- slack/webhook URL, or recipient email address
    events TEXT NOT NULL DEFAULT '["alert.*"]',  -- JSON array of event-kind filters ('*' = all)
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_notification_channels_enabled ON notification_channels(enabled);

-- Per-channel delivery attempts (mirrors webhook_deliveries: retry with backoff).
CREATE TABLE IF NOT EXISTS channel_deliveries (
    id TEXT NOT NULL PRIMARY KEY,
    channel_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    target TEXT NOT NULL,
    subject TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT '',
    event_kind TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',   -- pending | delivered | failed
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 6,
    next_retry_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_error TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_channel_deliveries_pending ON channel_deliveries(status, next_retry_at);
