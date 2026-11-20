-- EC2 AMI-style visibility: public (default, today's behaviour), private (owner project + shared projects), and
-- explicit per-project shares. Applies to image listing; see docs/cloud-ec2-semantics.md for what is not enforced.
ALTER TABLE templates ADD COLUMN visibility TEXT NOT NULL DEFAULT 'public';
CREATE TABLE IF NOT EXISTS image_shares (
    template_id BLOB NOT NULL REFERENCES templates(id) ON DELETE CASCADE,
    project TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (template_id, project)
);
