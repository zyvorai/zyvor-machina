-- OS images the bare-metal deploy writes to disk. Imported from a well-known source
-- or a URL, always verified against a checksum before it is marked ready.
CREATE TABLE baremetal_images (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    os TEXT NOT NULL DEFAULT '',
    arch TEXT NOT NULL DEFAULT 'amd64',
    format TEXT NOT NULL DEFAULT 'qcow2',
    size_bytes INTEGER NOT NULL DEFAULT 0,
    checksum TEXT NOT NULL DEFAULT '',
    checksum_algo TEXT NOT NULL DEFAULT 'sha256',
    source_url TEXT NOT NULL DEFAULT '',
    path TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    error TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
