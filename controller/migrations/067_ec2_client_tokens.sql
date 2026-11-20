-- EC2 `ClientToken` idempotency: the first answer to (user, action, token) is kept so a retry gets the same answer.
CREATE TABLE ec2_client_tokens (
    username TEXT NOT NULL,
    action TEXT NOT NULL,
    token TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    response TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (username, action, token)
);
CREATE INDEX idx_ec2_client_tokens_created ON ec2_client_tokens (created_at);
