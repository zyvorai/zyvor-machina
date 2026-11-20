-- 058 forgot the column the health check writes when a member changes state; every probe result failed to save with 'no such column'.
ALTER TABLE lb_members ADD COLUMN health_changed_at TEXT;
