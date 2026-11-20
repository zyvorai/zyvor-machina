-- Platform batch 11: inventory sync interval

ALTER TABLE clusters ADD COLUMN IF NOT EXISTS inventory_sync_interval_secs INT NOT NULL DEFAULT 30;
