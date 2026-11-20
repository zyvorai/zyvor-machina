-- Autopilot capacity: a member leaving its group's load balancer is drained
-- before it stops or sleeps; metric history is rolled up hourly and kept 35
-- days so forecasts can see daily and weekly patterns.
ALTER TABLE cloud_group_members ADD COLUMN draining_since TEXT;

-- One row per subject+metric+hour (epoch seconds of the hour start).
CREATE TABLE IF NOT EXISTS metric_hourly (
    subject TEXT NOT NULL,
    metric TEXT NOT NULL,
    hour INTEGER NOT NULL,
    avg REAL NOT NULL,
    max REAL NOT NULL,
    n INTEGER NOT NULL,
    PRIMARY KEY (subject, metric, hour)
);
CREATE INDEX IF NOT EXISTS idx_metric_hourly_hour ON metric_hourly(hour);
