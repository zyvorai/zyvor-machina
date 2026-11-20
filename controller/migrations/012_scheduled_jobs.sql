-- Day-2: general-purpose recurring-jobs framework. Recurrence was previously hardcoded
-- per domain (snapshots via interval_minutes; maintenance one-shot run_at). This gives a
-- single interval-based scheduler that enqueues a whitelisted, self-contained task
-- operation on a cadence — the foundation for recurring host refresh/report/patch ops.
-- (Record-dependent ops like vm.snapshot/vm.backup keep their bespoke schedulers, which
-- pre-insert their tracking rows; this framework covers ops that need no pre-setup.)
CREATE TABLE IF NOT EXISTS scheduled_jobs (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    operation TEXT NOT NULL,           -- whitelisted task op (see engine/scheduled_jobs_runner.rs)
    payload TEXT NOT NULL DEFAULT '{}',
    target_host_id TEXT,               -- optional; NULL = all hosts for host-scoped ops
    interval_minutes INTEGER NOT NULL DEFAULT 60,
    enabled INTEGER NOT NULL DEFAULT 1,
    last_run_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_enabled ON scheduled_jobs(enabled);
