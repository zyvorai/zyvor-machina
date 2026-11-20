-- Access keys for the EC2-compatible Query API (SigV4). The secret must be recoverable to verify signatures, so it is
-- stored encrypted with MACHINA_API_KEY_MASTER_KEY when that is set (plaintext otherwise, as for LLM provider keys).
CREATE TABLE IF NOT EXISTS ec2_access_keys (
    access_key_id TEXT NOT NULL PRIMARY KEY,
    secret_enc TEXT NOT NULL,
    username TEXT NOT NULL,
    role TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_used_at TEXT,
    revoked INTEGER NOT NULL DEFAULT 0
);
