-- Desktop pools (simple VDI): a stopped template VM is cloned into numbered desktops that users
-- check out one at a time. A non-persistent pool deletes and re-clones a desktop when its user
-- releases it. Member state: needs_clone | cloning | ready | assigned | recycling | error.
CREATE TABLE desktop_pools (
    id UUID PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    template_vm_id UUID NOT NULL,
    prefix TEXT NOT NULL,
    size BIGINT NOT NULL,
    clone_mode TEXT NOT NULL DEFAULT 'linked',
    persistent BOOLEAN NOT NULL DEFAULT FALSE,
    created_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);
CREATE TABLE desktop_pool_members (
    id UUID PRIMARY KEY NOT NULL,
    pool_id UUID NOT NULL REFERENCES desktop_pools(id) ON DELETE CASCADE,
    vm_name TEXT NOT NULL,
    assigned_user TEXT NOT NULL DEFAULT '',
    assigned_at TEXT NOT NULL DEFAULT '',
    state TEXT NOT NULL DEFAULT 'needs_clone',
    message TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE (pool_id, vm_name)
);
-- One desktop per user per pool.
CREATE UNIQUE INDEX idx_desktop_pool_members_user ON desktop_pool_members(pool_id, assigned_user)
    WHERE assigned_user <> '';
