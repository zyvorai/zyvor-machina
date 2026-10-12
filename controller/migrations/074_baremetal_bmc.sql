-- Bare-metal BMC access: per-server credentials (password stored encrypted by the
-- controller, `enc:` prefix when MACHINA_API_KEY_MASTER_KEY is set) and the outcome
-- of the last live power call. Power control used to be a database-only preview.
ALTER TABLE baremetal_servers ADD COLUMN bmc_username TEXT NOT NULL DEFAULT '';
ALTER TABLE baremetal_servers ADD COLUMN bmc_password_enc TEXT NOT NULL DEFAULT '';
ALTER TABLE baremetal_servers ADD COLUMN last_error TEXT NOT NULL DEFAULT '';
