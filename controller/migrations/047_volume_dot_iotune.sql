-- EC2 block-device behaviour: delete a volume with the instance it is attached to, and per-volume I/O limits.
ALTER TABLE volumes ADD COLUMN delete_on_termination INTEGER NOT NULL DEFAULT 0;
ALTER TABLE volumes ADD COLUMN read_iops INTEGER;
ALTER TABLE volumes ADD COLUMN write_iops INTEGER;
ALTER TABLE volumes ADD COLUMN read_bps INTEGER;
ALTER TABLE volumes ADD COLUMN write_bps INTEGER;
