-- Default alert rules, so a fresh install tells the operator about the problems that matter without setup.
-- Inserted once by the migration: an operator who deletes or disables one keeps it that way. They fire through the
-- normal dispatcher (in-app notification, webhooks, notification channels). Certificate expiry already alerts by
-- default (engine/cert_monitor.rs), so it is not duplicated here.
INSERT OR IGNORE INTO alert_rules (id, name, metric, comparator, threshold, severity, cooldown_minutes, enabled)
VALUES (x'a1e47000000040008000000000000001', 'Host offline', 'host_offline', 'gt', 0, 'critical', 5, 1);
INSERT OR IGNORE INTO alert_rules (id, name, metric, comparator, threshold, severity, cooldown_minutes, enabled)
VALUES (x'a1e47000000040008000000000000002', 'Storage pool above 90 % full', 'storage_pool_percent', 'gt', 90, 'warning', 60, 1);
INSERT OR IGNORE INTO alert_rules (id, name, metric, comparator, threshold, severity, cooldown_minutes, enabled)
VALUES (x'a1e47000000040008000000000000003', 'Backup failed (last 24 h)', 'backup_failed_24h', 'gt', 0, 'warning', 360, 1);
INSERT OR IGNORE INTO alert_rules (id, name, metric, comparator, threshold, severity, cooldown_minutes, enabled)
VALUES (x'a1e47000000040008000000000000004', 'Burst of failed tasks (15 min)', 'failed_task_burst', 'gt', 5, 'warning', 30, 1);
