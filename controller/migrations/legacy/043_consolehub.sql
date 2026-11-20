-- Zeus ConsoleHub: sessions, host Guacamole metadata, JIT access, marketplace console plugins

ALTER TABLE hosts
    ADD COLUMN IF NOT EXISTS guacamole_base_url TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS guacamole_json_secret_hex TEXT NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS console_sessions (
    id UUID PRIMARY KEY,
    vm_id UUID NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    host_id UUID REFERENCES hosts(id) ON DELETE SET NULL,
    actor TEXT NOT NULL DEFAULT '',
    actor_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    protocol TEXT NOT NULL,
    backend TEXT NOT NULL DEFAULT 'native',
    guac_token TEXT,
    agent_proxy_base TEXT NOT NULL DEFAULT '',
    emergency_url TEXT,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ended_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ NOT NULL,
    audit_id UUID,
    recording_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    recording_path TEXT,
    spectator_token TEXT,
    metadata_json JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_console_sessions_vm ON console_sessions(vm_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_console_sessions_actor ON console_sessions(actor, started_at DESC);

CREATE TABLE IF NOT EXISTS console_access_requests (
    id UUID PRIMARY KEY,
    vm_id UUID NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    requester TEXT NOT NULL,
    requester_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    protocol TEXT NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    approved_by TEXT,
    approved_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_console_access_requests_status ON console_access_requests(status, created_at DESC);

INSERT INTO platform_plugins (id, slug, name, category, description, version, author, featured, installed)
VALUES
    ('c2000000-0000-4000-8000-000000000010', 'kasm-workspaces', 'Kasm Workspaces', 'console', 'Disposable browser and isolated desktop labs (Marketplace workload — not core ConsoleHub).', '1.0.0', 'Kasm', FALSE, FALSE),
    ('c2000000-0000-4000-8000-000000000011', 'rustdesk', 'RustDesk', 'console', 'TeamViewer-style remote support sessions via Marketplace plugin.', '1.0.0', 'RustDesk', FALSE, FALSE),
    ('c2000000-0000-4000-8000-000000000012', 'meshcentral', 'MeshCentral', 'console', 'Remote management and support gateway as optional Marketplace plugin.', '1.0.0', 'MeshCentral', FALSE, FALSE)
ON CONFLICT (slug) DO NOTHING;
