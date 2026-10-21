-- Fleet-managed k3s clusters: a cluster is one server node plus agent nodes, each on an
-- enrolled host. The join token and admin kubeconfig are stored encrypted (`enc:` prefix when
-- MACHINA_API_KEY_MASTER_KEY is set). Status: provisioning | ready | upgrading | error | deleting.
CREATE TABLE k8s_clusters (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 16),
    name TEXT NOT NULL UNIQUE,
    version TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'provisioning',
    message TEXT NOT NULL DEFAULT '',
    api_url TEXT NOT NULL DEFAULT '',
    token_enc TEXT NOT NULL DEFAULT '',
    kubeconfig_enc TEXT NOT NULL DEFAULT '',
    created_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
-- Node status: pending | ready | error | removing. Role: server | agent.
CREATE TABLE k8s_cluster_nodes (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 16),
    cluster_id TEXT NOT NULL REFERENCES k8s_clusters(id) ON DELETE CASCADE,
    host_id TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    node_name TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    message TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (cluster_id, host_id)
);
CREATE INDEX idx_k8s_cluster_nodes_host ON k8s_cluster_nodes(host_id);
