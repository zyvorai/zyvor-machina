-- Load balancer health checks: per-LB check config, per-member health state. Unhealthy members are left out of the rule set.
ALTER TABLE load_balancers ADD COLUMN hc_protocol TEXT NOT NULL DEFAULT 'none';
ALTER TABLE load_balancers ADD COLUMN hc_port INTEGER;
ALTER TABLE load_balancers ADD COLUMN hc_path TEXT NOT NULL DEFAULT '/';
ALTER TABLE load_balancers ADD COLUMN hc_interval_secs INTEGER NOT NULL DEFAULT 10;
ALTER TABLE load_balancers ADD COLUMN hc_timeout_secs INTEGER NOT NULL DEFAULT 3;
ALTER TABLE load_balancers ADD COLUMN hc_healthy_threshold INTEGER NOT NULL DEFAULT 2;
ALTER TABLE load_balancers ADD COLUMN hc_unhealthy_threshold INTEGER NOT NULL DEFAULT 3;
ALTER TABLE load_balancers ADD COLUMN hc_last_run TEXT;
ALTER TABLE lb_members ADD COLUMN health TEXT NOT NULL DEFAULT 'unknown';
ALTER TABLE lb_members ADD COLUMN health_ok INTEGER NOT NULL DEFAULT 0;
ALTER TABLE lb_members ADD COLUMN health_fail INTEGER NOT NULL DEFAULT 0;
ALTER TABLE lb_members ADD COLUMN health_detail TEXT NOT NULL DEFAULT '';
