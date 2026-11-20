-- Per-task retry accounting: number of times this task has been attempted.
-- Used to bound automatic retries of transient (agent-unreachable) failures.
ALTER TABLE tasks ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;

-- Owning controller instance that claimed the task (running state). Lets the
-- startup reaper reap only its OWN orphaned tasks instead of blindly failing
-- every 'running' row — which, in a multi-controller deployment, would mark a
-- peer controller's in-flight task as failed.
ALTER TABLE tasks ADD COLUMN claimed_by TEXT;
