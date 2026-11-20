-- Project-scoped API keys: a JSON array of project names. Empty (the default) keeps the old behaviour (global role only).
ALTER TABLE api_keys ADD COLUMN projects TEXT NOT NULL DEFAULT '[]';
