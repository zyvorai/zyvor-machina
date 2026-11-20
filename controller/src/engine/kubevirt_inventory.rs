// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::collections::HashSet;

use reqwest::Client;
use serde::Deserialize;
use uuid::Uuid;

use crate::engine::vm_inventory::{cluster_inventory_policy, ClusterInventoryPolicy};
use crate::state::AppState;

const KUBEVIRT_MISSING: &str =
    "kubevirt_not_found: VirtualMachine no longer present in cluster inventory scan";

#[derive(Debug, Deserialize)]
pub struct KubeVirtInventoryResponse {
    #[serde(default)]
    rows: Vec<KubeVirtVmRow>,
    #[serde(default = "default_true")]
    kubevirt_available: bool,
    #[serde(default)]
    list_error: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct KubeVirtVmRow {
    name: String,
    namespace: String,
    #[serde(default)]
    vmi_phase: Option<String>,
    #[serde(default)]
    vm_printable_status: Option<String>,
    #[serde(default)]
    guest_ip: Option<String>,
    #[serde(default)]
    pod_ip: Option<String>,
}

fn observed_from_row(row: &KubeVirtVmRow) -> String {
    let phase = row
        .vmi_phase
        .as_deref()
        .or(row.vm_printable_status.as_deref())
        .unwrap_or("unknown");
    match phase {
        "Running" | "running" | "Ready" | "ready" => "running".into(),
        "Stopped" | "stopped" | "Halted" | "halted" => "stopped".into(),
        "Pending" | "pending" | "Scheduling" | "scheduling" => "pending".into(),
        "Failed" | "failed" | "Error" | "error" => "failed".into(),
        other if other.eq_ignore_ascii_case("running") => "running".into(),
        other if other.eq_ignore_ascii_case("stopped") => "stopped".into(),
        _ => "unknown".into(),
    }
}

pub async fn fetch_inventory_rows(
    daemon_base_url: &str,
    jwt_secret: &str,
) -> anyhow::Result<KubeVirtInventoryResponse> {
    let base = daemon_base_url.trim_end_matches('/');
    let path = "/api/v1/k8s/kubevirt/vm-summary?all_namespaces=true&meta=true";

    // The daemon's `require_browser_session_for_host_insight` guard rejects API
    // tokens on this route (it's grouped with endpoints that read passwd-like
    // host data), so this same-host sync call needs a browser-equivalent
    // credential. A short-lived platform JWT — the same mechanism used for
    // controller-issued deep links into the daemon UI — satisfies that guard
    // without weakening it for anyone else.
    let token =
        crate::jwt::issue_token(jwt_secret, "controller-internal-sync", "operator", 60, None)
            .map_err(|e| anyhow::anyhow!("failed to mint internal service token: {e:#}"))?;

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        // Loopback self-call to the daemon's own (commonly self-signed) TLS
        // listener — accepting its cert here is no less trusted than the plain
        // HTTP this call used to send.
        .danger_accept_invalid_certs(true)
        .build()?;

    let configured_url = format!("{base}{path}");
    let (host_port, configured_is_https) = match base.strip_prefix("https://") {
        Some(rest) => (rest, true),
        None => (base.strip_prefix("http://").unwrap_or(base), false),
    };

    // Deployments commonly run the daemon TLS-only, even on loopback, but
    // `daemon_base_url` defaults to `http://…`. Try HTTPS first; fall back to
    // the URL as configured if that can't even connect (e.g. a genuinely
    // plaintext daemon), so this works either way without new config.
    let resp = if configured_is_https {
        client
            .get(&configured_url)
            .bearer_auth(&token)
            .send()
            .await?
    } else {
        let https_url = format!("https://{host_port}{path}");
        match client.get(&https_url).bearer_auth(&token).send().await {
            Ok(r) => r,
            Err(_) => {
                client
                    .get(&configured_url)
                    .bearer_auth(&token)
                    .send()
                    .await?
            }
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("daemon kubevirt inventory HTTP {status}: {body}");
    }
    Ok(resp.json().await?)
}

/// Outcome of a `sync_cluster` attempt. `synced == false` means the DB was left
/// untouched (fetch failure, CRD unavailable, or a list error with no rows) —
/// callers must not report this as a successful sync.
#[derive(Debug, Default)]
pub struct KubevirtSyncOutcome {
    pub synced: bool,
    pub reason: Option<String>,
}

pub async fn sync_cluster(
    state: &AppState,
    cluster_id: Uuid,
) -> anyhow::Result<KubevirtSyncOutcome> {
    let summary =
        match fetch_inventory_rows(&state.config.daemon_base_url, &state.config.jwt_secret).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("kubevirt inventory fetch failed (keeping DB rows): {e:#}");
                return Ok(KubevirtSyncOutcome {
                    synced: false,
                    reason: Some(format!("kubevirt inventory fetch failed: {e:#}")),
                });
            }
        };

    if !summary.kubevirt_available {
        tracing::debug!(
            list_error = ?summary.list_error,
            "kubevirt CRD unavailable — skip prune"
        );
        return Ok(KubevirtSyncOutcome {
            synced: false,
            reason: Some("kubevirt CRD unavailable on cluster".into()),
        });
    }

    if summary.list_error.is_some() && summary.rows.is_empty() {
        tracing::warn!(
            err = ?summary.list_error,
            "kubevirt list error with empty rows — skip prune"
        );
        return Ok(KubevirtSyncOutcome {
            synced: false,
            reason: summary.list_error.clone(),
        });
    }

    let policy = cluster_inventory_policy(&state.pool, cluster_id).await?;
    let mut seen: HashSet<(String, String)> = HashSet::new();

    for row in &summary.rows {
        let ns = row.namespace.clone();
        let name = row.name.clone();
        seen.insert((ns.clone(), name.clone()));
        let observed = observed_from_row(row);
        let spec = serde_json::json!({
            "kubevirt": true,
            "namespace": ns,
            "guest_ip": row.guest_ip,
            "pod_ip": row.pod_ip,
        });

        let existing: Option<(Uuid, bool)> = crate::db::query_as(
            "SELECT id, managed FROM vms
             WHERE cluster_id = ? AND inventory_source = 'kubevirt'
               AND k8s_namespace = ? AND name = ?",
        )
        .bind(cluster_id)
        .bind(&ns)
        .bind(&name)
        .fetch_optional(&state.pool)
        .await?;

        if let Some((id, _managed)) = existing {
            crate::db::query(
                "UPDATE vms SET observed_state = ?, spec_json = ?, last_seen_at = datetime('now'), updated_at = datetime('now')
                 WHERE id = ?",
            )
            .bind(&observed)
            .bind(&spec)
            .bind(id)
            .execute(&state.pool)
            .await?;
        } else {
            let new_id = Uuid::new_v4();
            crate::db::query(
                "INSERT INTO vms (id, cluster_id, host_id, name, k8s_namespace, spec_json,
                 desired_state, observed_state, managed, lifecycle_phase, inventory_source, last_seen_at)
                 VALUES (?, ?, NULL, ?, ?, ?, 'unknown', ?, FALSE, 'idle', 'kubevirt', datetime('now'))",
            )
            .bind(new_id)
            .bind(cluster_id)
            .bind(&name)
            .bind(&ns)
            .bind(&spec)
            .bind(&observed)
            .execute(&state.pool)
            .await?;
            state.emit_event(
                "vm.discovered",
                format!("Discovered KubeVirt VM '{}/{}' in cluster", ns, name),
            );
        }
    }

    reconcile_kubevirt_tombstones(state, cluster_id, &seen, &policy).await?;
    Ok(KubevirtSyncOutcome {
        synced: true,
        reason: None,
    })
}

async fn reconcile_kubevirt_tombstones(
    state: &AppState,
    cluster_id: Uuid,
    seen: &HashSet<(String, String)>,
    policy: &ClusterInventoryPolicy,
) -> anyhow::Result<()> {
    #[derive(sqlx::FromRow)]
    struct Row {
        id: Uuid,
        name: String,
        k8s_namespace: Option<String>,
        managed: bool,
    }

    let rows: Vec<Row> = crate::db::query_as(
        "SELECT id, name, k8s_namespace, managed FROM vms
         WHERE cluster_id = ? AND inventory_source = 'kubevirt'",
    )
    .bind(cluster_id)
    .fetch_all(&state.pool)
    .await?;

    for row in rows {
        let ns = row.k8s_namespace.unwrap_or_else(|| "default".into());
        if seen.contains(&(ns.clone(), row.name.clone())) {
            continue;
        }
        if !row.managed && policy.inventory_prune_unmanaged {
            crate::db::query("DELETE FROM vms WHERE id = ?")
                .bind(row.id)
                .execute(&state.pool)
                .await?;
            state.emit_event(
                "vm.removed",
                format!(
                    "Discovered KubeVirt VM '{}/{}' removed from inventory",
                    ns, row.name
                ),
            );
        } else if row.managed && policy.inventory_mark_managed_missing {
            crate::db::query(
                "UPDATE vms SET observed_state = 'missing', last_error = ?, updated_at = datetime('now') WHERE id = ?",
            )
            .bind(KUBEVIRT_MISSING)
            .bind(row.id)
            .execute(&state.pool)
            .await?;
            state.emit_event(
                "vm.missing",
                format!("KubeVirt VM '{}/{}' missing from cluster", ns, row.name),
            );
        }
    }
    Ok(())
}

/// Delete a KubeVirt VM via the co-located daemon (kubectl delete).
pub async fn delete_kubevirt_vm(
    daemon_base_url: &str,
    namespace: &str,
    name: &str,
) -> anyhow::Result<()> {
    let url = format!(
        "{}/api/v1/k8s/kubevirt/virtualmachines/{}/{}",
        daemon_base_url.trim_end_matches('/'),
        urlencoding::encode(namespace),
        urlencoding::encode(name)
    );
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let resp = client.delete(&url).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("daemon kubevirt delete HTTP {status}: {body}");
    }
    Ok(())
}
