-- Off-site backup copies now carry a SHA-256 checksum manifest stored next to the data. Record
-- the manifest's own digest and whether the copy has one (copies made earlier are size-verified only).
ALTER TABLE backup_replicas ADD COLUMN manifest_sha256 TEXT NOT NULL DEFAULT '';
ALTER TABLE backup_replicas ADD COLUMN checksummed INTEGER NOT NULL DEFAULT 0;
