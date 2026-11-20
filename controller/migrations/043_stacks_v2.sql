-- Stack v2: instance groups and policies are reconciled against what exists.
-- drift_json: the last reconcile's findings. auto_heal: recreate missing VMs.
-- previous_template_json: the template before the last update, for undo.
ALTER TABLE stacks ADD COLUMN drift_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE stacks ADD COLUMN checked_at TEXT;
ALTER TABLE stacks ADD COLUMN auto_heal INTEGER NOT NULL DEFAULT 1;
ALTER TABLE stacks ADD COLUMN updated_at TEXT;
ALTER TABLE stacks ADD COLUMN previous_template_json TEXT;
