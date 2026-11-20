-- Golden template metadata: workload profile, approval, optional git/daemon linkage
ALTER TABLE templates ADD COLUMN IF NOT EXISTS workload TEXT NOT NULL DEFAULT '';
ALTER TABLE templates ADD COLUMN IF NOT EXISTS approval_status TEXT NOT NULL DEFAULT 'approved';
ALTER TABLE templates ADD COLUMN IF NOT EXISTS git_ref TEXT NOT NULL DEFAULT '';
ALTER TABLE templates ADD COLUMN IF NOT EXISTS daemon_json_path TEXT NOT NULL DEFAULT '';
ALTER TABLE templates ADD COLUMN IF NOT EXISTS project TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS idx_templates_workload ON templates (workload);
CREATE INDEX IF NOT EXISTS idx_templates_approval ON templates (approval_status);
