-- Chaos game days: an experiment is a list of fault steps against target
-- VMs with health probes; each run keeps its own report.
CREATE TABLE IF NOT EXISTS chaos_experiments (
 id TEXT PRIMARY KEY NOT NULL,
 name TEXT NOT NULL UNIQUE,
 description TEXT NOT NULL DEFAULT '',
 spec_json TEXT NOT NULL,
 created_by TEXT NOT NULL DEFAULT '',
 created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
 updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS chaos_runs (
 id TEXT PRIMARY KEY NOT NULL,
 experiment_id TEXT NOT NULL REFERENCES chaos_experiments(id) ON DELETE CASCADE,
 status TEXT NOT NULL DEFAULT 'running' CHECK(status IN ('running','passed','failed','aborted','interrupted')),
 started_by TEXT NOT NULL DEFAULT '',
 started_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
 finished_at TEXT,
 abort_reason TEXT NOT NULL DEFAULT '',
 report_json TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS chaos_runs_experiment ON chaos_runs(experiment_id, started_at);
-- At most one run in flight per experiment.
CREATE UNIQUE INDEX IF NOT EXISTS chaos_runs_one_live ON chaos_runs(experiment_id) WHERE status = 'running';
