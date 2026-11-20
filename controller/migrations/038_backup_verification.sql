-- Scheduled backup verification: when a stored backup was last re-checked and what the check found.
ALTER TABLE backup_records ADD COLUMN verified_at TEXT;
ALTER TABLE backup_records ADD COLUMN verify_status TEXT NOT NULL DEFAULT '';
ALTER TABLE backup_records ADD COLUMN verify_message TEXT;
