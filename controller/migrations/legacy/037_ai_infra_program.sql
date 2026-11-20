-- AI Infrastructure Program: structured incidents + graph memory (AI-138)

CREATE TABLE IF NOT EXISTS ai_incidents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    severity TEXT NOT NULL DEFAULT 'medium',
    status TEXT NOT NULL DEFAULT 'open',
    affected_resources JSONB NOT NULL DEFAULT '[]',
    root_cause TEXT,
    evidence_json JSONB NOT NULL DEFAULT '{}',
    window_start TIMESTAMPTZ,
    window_end TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ai_incidents_status ON ai_incidents(status);
CREATE INDEX IF NOT EXISTS idx_ai_incidents_created ON ai_incidents(created_at DESC);
