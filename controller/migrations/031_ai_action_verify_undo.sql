-- AI actions can now be verified after they run and, for reversible ones, undone.
--   before_state  JSON snapshot of the machine taken just before execution
--   verify_result JSON {status: ok|pending|failed|unknown, detail, checked_at}
--   undone_at     set when an operator undid the action
ALTER TABLE ai_actions ADD COLUMN before_state TEXT NOT NULL DEFAULT '{}';
ALTER TABLE ai_actions ADD COLUMN verify_result TEXT NOT NULL DEFAULT '{}';
ALTER TABLE ai_actions ADD COLUMN undone_at TEXT;
