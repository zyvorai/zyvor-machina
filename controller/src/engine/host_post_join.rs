// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! What a host still needs after it joined: the networks and storage pools named on its
//! enrollment token. They are applied once the host has passed validation (the agent is up
//! and reachable by then), by queuing the same `network.provision` and
//! `storage.pool.provision` tasks the API uses. Unknown names are reported in the join log,
//! not retried: the operator named something that does not exist.

use uuid::Uuid;

use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

/// A name list as stored on the token: trimmed, de-duplicated, bounded, no control characters.
pub fn clean_names(v: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for n in v {
        let n = n.trim();
        if n.is_empty() {
            continue;
        }
        if n.len() > 64 || n.chars().any(|c| c.is_control()) {
            return Err(format!("invalid name '{n}'"));
        }
        if !out.iter().any(|x| x == n) {
            out.push(n.to_string());
        }
    }
    if out.len() > 32 {
        return Err("at most 32 names".into());
    }
    Ok(out)
}

/// Queue provisioning for the host's pending networks and pools; returns how many tasks.
pub async fn apply(state: &AppState, host_id: Uuid) -> anyhow::Result<usize> {
    let row: Option<(String, String)> =
        crate::db::query_as("SELECT networks, storage_pools FROM host_post_join WHERE host_id = ?")
            .bind(host_id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((nets, pools)) = row else {
        return Ok(0);
    };
    // Consume first: a second validation must not queue the work twice.
    crate::db::query("DELETE FROM host_post_join WHERE host_id = ?")
        .bind(host_id)
        .execute(&state.pool)
        .await?;
    let nets: Vec<String> = serde_json::from_str(&nets).unwrap_or_default();
    let pools: Vec<String> = serde_json::from_str(&pools).unwrap_or_default();
    let mut queued = 0;
    for name in nets {
        let id: Option<Uuid> = crate::db::query_scalar("SELECT id FROM networks WHERE name = ?")
            .bind(&name)
            .fetch_optional(&state.pool)
            .await?;
        let Some(id) = id else {
            note(
                state,
                host_id,
                "error",
                format!("network '{name}' named on the token does not exist"),
            )
            .await;
            continue;
        };
        match enqueue_task(
            state,
            "network.provision",
            serde_json::json!({ "network_id": id.to_string(), "host_id": host_id.to_string() }),
            Some("network"),
            Some(id),
            Some(host_id),
        )
        .await
        {
            Ok(_) => {
                queued += 1;
                note(
                    state,
                    host_id,
                    "ok",
                    format!("provisioning network '{name}' on this host"),
                )
                .await;
            }
            Err(e) => {
                note(
                    state,
                    host_id,
                    "error",
                    format!("network '{name}': {}", e.message),
                )
                .await
            }
        }
    }
    for name in pools {
        let id: Option<Uuid> =
            crate::db::query_scalar("SELECT id FROM storage_pools WHERE name = ?")
                .bind(&name)
                .fetch_optional(&state.pool)
                .await?;
        let Some(id) = id else {
            note(
                state,
                host_id,
                "error",
                format!("storage pool '{name}' named on the token does not exist"),
            )
            .await;
            continue;
        };
        match enqueue_task(
            state,
            "storage.pool.provision",
            serde_json::json!({ "pool_id": id.to_string(), "host_id": host_id.to_string() }),
            Some("storage_pool"),
            Some(id),
            Some(host_id),
        )
        .await
        {
            Ok(_) => {
                queued += 1;
                note(
                    state,
                    host_id,
                    "ok",
                    format!("provisioning storage pool '{name}' on this host"),
                )
                .await;
            }
            Err(e) => {
                note(
                    state,
                    host_id,
                    "error",
                    format!("storage pool '{name}': {}", e.message),
                )
                .await
            }
        }
    }
    Ok(queued)
}

async fn note(state: &AppState, host_id: Uuid, level: &str, msg: String) {
    crate::api::join_events::record_for_host(&state.pool, host_id, level, "provision", &msg).await;
}

#[cfg(test)]
mod tests {
    use super::clean_names;

    #[test]
    fn names_are_trimmed_deduplicated_and_bounded() {
        let v = vec![" br0 ".into(), "br0".into(), "".into(), "pool-a".into()];
        assert_eq!(clean_names(&v).unwrap(), vec!["br0", "pool-a"]);
        assert!(clean_names(&["a\nb".to_string()]).is_err());
        assert!(clean_names(&["x".repeat(65)]).is_err());
        let many: Vec<String> = (0..40).map(|i| format!("n{i}")).collect();
        assert!(clean_names(&many).is_err());
    }
}
