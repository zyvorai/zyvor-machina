-- Time series behind real forecasting: one row per subject+metric every few minutes, pruned after 14 days.
-- subject: a VM id, or 'pool:<storage pool id>'. metric: mem_ratio | cpu_percent | pool_used_ratio.
CREATE TABLE IF NOT EXISTS metric_samples (
    subject TEXT NOT NULL,
    metric TEXT NOT NULL,
    ts INTEGER NOT NULL,
    value REAL NOT NULL,
    PRIMARY KEY (subject, metric, ts)
);
CREATE INDEX IF NOT EXISTS idx_metric_samples_ts ON metric_samples(ts);
