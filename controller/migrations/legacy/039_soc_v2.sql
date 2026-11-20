-- SOC v2 — global settings (webhook URL for playbooks)

CREATE TABLE IF NOT EXISTS soc_settings (
    id INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    webhook_url TEXT NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO soc_settings (id, webhook_url) VALUES (1, '')
ON CONFLICT (id) DO NOTHING;
