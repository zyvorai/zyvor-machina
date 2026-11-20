-- Real project entities with membership, replacing the free-text-only view in
-- api/projects.rs (previously just `SELECT DISTINCT project FROM vms`). No `domains`
-- layer — Machina doesn't need Keystone's service-provider-scale multi-org isolation;
-- a flat project list is enough (see the "next-gen, simpler" roadmap plan).
--
-- Backward-compat: `vms.project` / `project_quotas.project` stay free-text and
-- untouched — these tables are joined in by NAME (see db::ensure_native_projects),
-- not by rewriting every existing project filter across the codebase.
CREATE TABLE IF NOT EXISTS projects (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS project_role_assignments (
    id TEXT NOT NULL PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(user_id, project_id, role)
);
CREATE INDEX IF NOT EXISTS idx_project_role_assignments_project ON project_role_assignments(project_id);
CREATE INDEX IF NOT EXISTS idx_project_role_assignments_user ON project_role_assignments(user_id);
