-- Who holds an allocated bare-metal server, and since when.
ALTER TABLE baremetal_servers ADD COLUMN allocated_to TEXT NOT NULL DEFAULT '';
ALTER TABLE baremetal_servers ADD COLUMN allocated_at TEXT NOT NULL DEFAULT '';
