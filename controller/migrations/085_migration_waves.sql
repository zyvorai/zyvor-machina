-- Migration waves: a planned batch of VMs to move in with Transiva/h2kvm. Each item keeps its
-- Transiva migration config encrypted (`enc:` prefix when MACHINA_API_KEY_MASTER_KEY is set; the
-- config can name source credentials) and the Transiva job it started.
-- Wave status: planned | running | done | attention | cancelled.
-- Item status: planned | submitted | running | completed | failed | cancelled.
-- Cutover (a manual checklist, set after a successful run): none | ready | cut_over.
CREATE TABLE migration_waves (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 16),
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    planned_at TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'planned',
    created_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE migration_wave_items (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 16),
    wave_id TEXT NOT NULL REFERENCES migration_waves(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    name TEXT NOT NULL,
    config_enc TEXT NOT NULL DEFAULT '',
    transiva_job_id TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'planned',
    phase TEXT NOT NULL DEFAULT '',
    progress_percent INTEGER NOT NULL DEFAULT 0,
    message TEXT NOT NULL DEFAULT '',
    cutover TEXT NOT NULL DEFAULT 'none',
    cutover_note TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (wave_id, name)
);
CREATE INDEX idx_migration_wave_items_wave ON migration_wave_items(wave_id, position);
