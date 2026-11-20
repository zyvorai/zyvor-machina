-- Migration waves: an operator can start a wave so the controller advances it on its own (poll
-- the Transiva jobs, start the next planned items) instead of waiting for an advance call.
ALTER TABLE migration_waves ADD COLUMN auto_advance BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE migration_waves ADD COLUMN max_parallel BIGINT NOT NULL DEFAULT 3;
