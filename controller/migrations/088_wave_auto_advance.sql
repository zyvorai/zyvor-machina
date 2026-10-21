-- Migration waves: an operator can start a wave so the controller advances it on its own (poll
-- the Transiva jobs, start the next planned items) instead of waiting for an advance call.
ALTER TABLE migration_waves ADD COLUMN auto_advance INTEGER NOT NULL DEFAULT 0;
ALTER TABLE migration_waves ADD COLUMN max_parallel INTEGER NOT NULL DEFAULT 3;
