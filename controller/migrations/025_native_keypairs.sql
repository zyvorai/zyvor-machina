-- Native SSH keypair catalog — part of Fleet Cloud's native compute layer.
-- A named public key an operator picks at instance-create
-- time via cloud-init (Machina has no private-key generation/storage — only the
-- public key is ever stored, matching how cloud-init ssh_authorized_keys works).
CREATE TABLE IF NOT EXISTS keypairs (
    id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    name TEXT NOT NULL UNIQUE,
    public_key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
