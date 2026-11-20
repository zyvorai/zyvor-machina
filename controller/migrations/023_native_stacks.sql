-- Declarative multi-resource stacks — Phase D of the next-gen roadmap
-- (/Users/ssahani/.claude/plans/lazy-munching-quilt.md). A stack describes a set of
-- resources (security groups, volumes, VMs) as one template; api::stacks creates them
-- in dependency order and tracks what it created in resources_json so DELETE can tear
-- the whole thing down as a unit. Not a new orchestration engine — it's a thin
-- composition layer over the existing native create_vm/create_volume/
-- create_security_group handlers.
CREATE TABLE IF NOT EXISTS stacks (
    id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    template_json TEXT NOT NULL,
    resources_json TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL DEFAULT 'creating',
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
