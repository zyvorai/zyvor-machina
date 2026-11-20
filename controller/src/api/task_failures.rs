// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Failed tasks, made readable: the last N hours grouped by operation and cause, each with a
//! plain-language hint, plus "acknowledge" so a counter shows what is new instead of history.

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::{Extension, Json};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct WindowQuery {
    #[serde(default = "default_hours")]
    pub hours: i64,
}
fn default_hours() -> i64 {
    24
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct FailedRow {
    id: String,
    operation: String,
    message: Option<String>,
    created_at: String,
}

#[derive(Debug, Serialize)]
pub struct FailureGroup {
    pub operation: String,
    pub cause: String,
    pub count: usize,
    pub last_at: String,
    pub sample_task_id: String,
    pub hint: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct FailureSummary {
    pub window_hours: i64,
    /// Failed in the window and not acknowledged.
    pub unacknowledged: usize,
    pub groups: Vec<FailureGroup>,
}

/// The message with ids, numbers and addresses removed, so the same cause groups together.
pub fn normalize(message: &str) -> String {
    let first = message.lines().next().unwrap_or("").trim();
    let mut out = String::with_capacity(first.len());
    let mut chars = first.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            while chars
                .peek()
                .is_some_and(|n| n.is_ascii_alphanumeric() || *n == '-')
            {
                chars.next();
            }
            if !out.ends_with('#') {
                out.push('#');
            }
        } else {
            out.push(c);
        }
    }
    out.chars().take(140).collect()
}

/// What to do about a failure, from its message.
pub fn hint_for(message: &str) -> Option<&'static str> {
    let m = message.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| m.contains(n));
    if has(&[
        "valid authentication",
        "unauthenticated",
        "invalid or missing agent token",
    ]) {
        Some("The agent rejected the controller's token. If this host re-joined, restart the controller; otherwise check MACHINA_AGENT_TOKEN in /etc/default/machina-platform on both machines.")
    } else if has(&[
        "connection refused",
        "transport error",
        "tcp connect error",
        "dns error",
        "no route to host",
        "connect error",
    ]) {
        Some("The agent is not reachable. Check that machina-agent is running on the host and that port 50051 is open from the controller (machina-preflight --role agent --controller URL).")
    } else if has(&["failed to connect socket", "libvirt", "virt"])
        && has(&["connect", "socket", "daemon", "not running"])
    {
        Some("libvirt is not answering on the host. Start it: sudo systemctl enable --now libvirtd (or virtqemud.socket).")
    } else if has(&["no space left", "disk full", "enospc"]) {
        Some("A disk is full on the host. Free space under /var/lib/libvirt and /var/lib/machina, then retry.")
    } else if has(&["timed out", "deadline exceeded", "timeout"]) {
        Some("The host did not answer in time. Check its load and network, then retry the task.")
    } else if has(&["qemu"]) && has(&["not found", "no such file", "not installed"]) {
        Some("QEMU is missing on the host: run machina-preflight --fix there.")
    } else if has(&["permission denied"]) {
        Some("A permission was denied on the host. The agent needs root and access to /dev/kvm and libvirt.")
    } else {
        None
    }
}

pub fn group(rows: &[(String, String, Option<String>, String)]) -> Vec<FailureGroup> {
    let mut map: HashMap<(String, String), FailureGroup> = HashMap::new();
    for (id, op, msg, at) in rows {
        let msg = msg.as_deref().unwrap_or("(no message)");
        let key = (op.clone(), normalize(msg));
        let e = map.entry(key.clone()).or_insert_with(|| FailureGroup {
            operation: op.clone(),
            cause: key.1.clone(),
            count: 0,
            last_at: at.clone(),
            sample_task_id: id.clone(),
            hint: hint_for(msg),
        });
        e.count += 1;
        if *at > e.last_at {
            e.last_at = at.clone();
            e.sample_task_id = id.clone();
        }
    }
    let mut out: Vec<FailureGroup> = map.into_values().collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(b.last_at.cmp(&a.last_at)));
    out
}

pub async fn summary(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<WindowQuery>,
) -> Result<Json<FailureSummary>, ApiError> {
    require_operator(&actor)?;
    let hours = q.hours.clamp(1, 24 * 30);
    let since = (Utc::now() - Duration::hours(hours))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let rows: Vec<FailedRow> = crate::db::query_as(
        "SELECT CAST(id AS TEXT) AS id, operation, message, CAST(created_at AS TEXT) AS created_at
         FROM tasks WHERE status = 'failed' AND acknowledged_at IS NULL AND created_at >= ?
         ORDER BY created_at DESC LIMIT 5000",
    )
    .bind(since)
    .fetch_all(&state.pool)
    .await?;
    let tuples: Vec<_> = rows
        .iter()
        .map(|r| {
            (
                r.id.clone(),
                r.operation.clone(),
                r.message.clone(),
                r.created_at.clone(),
            )
        })
        .collect();
    Ok(Json(FailureSummary {
        window_hours: hours,
        unacknowledged: rows.len(),
        groups: group(&tuples),
    }))
}

#[derive(Debug, Deserialize)]
pub struct AckBody {
    /// Only this operation; all failed tasks when absent.
    #[serde(default)]
    pub operation: Option<String>,
}

pub async fn acknowledge(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<AckBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let res = match body.operation.as_deref().filter(|o| !o.is_empty()) {
        Some(op) => {
            crate::db::query(
                "UPDATE tasks SET acknowledged_at = ? WHERE status = 'failed' AND acknowledged_at IS NULL AND operation = ?",
            )
            .bind(&now)
            .bind(op)
            .execute(&state.pool)
            .await?
        }
        None => {
            crate::db::query(
                "UPDATE tasks SET acknowledged_at = ? WHERE status = 'failed' AND acknowledged_at IS NULL",
            )
            .bind(&now)
            .execute(&state.pool)
            .await?
        }
    };
    Ok(Json(
        serde_json::json!({ "acknowledged": res.rows_affected() }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_cause_groups_together_whatever_the_ids_and_numbers() {
        let a = normalize("task failed: host 10.1.2.3 port 50051 refused (id 8f3a-22)");
        let b = normalize("task failed: host 10.9.9.9 port 50052 refused (id 1c4d-77)");
        assert_eq!(a, b);
    }

    #[test]
    fn hints_name_the_likely_fix() {
        let auth =
            hint_for("code: 'The request does not have valid authentication credentials'").unwrap();
        assert!(auth.contains("MACHINA_AGENT_TOKEN"));
        assert!(
            hint_for("transport error: Connection refused (os error 111)")
                .unwrap()
                .contains("50051")
        );
        assert!(hint_for("write failed: No space left on device")
            .unwrap()
            .contains("disk"));
        assert!(hint_for("something nobody has seen before").is_none());
    }

    #[test]
    fn groups_are_counted_and_the_biggest_comes_first() {
        let r = |id: &str, op: &str, m: &str, at: &str| {
            (
                id.to_string(),
                op.to_string(),
                Some(m.to_string()),
                at.to_string(),
            )
        };
        let g = group(&[
            r(
                "1",
                "host.sync",
                "transport error: Connection refused",
                "2026-10-07 10:00:00",
            ),
            r(
                "2",
                "host.sync",
                "transport error: Connection refused",
                "2026-10-07 10:05:00",
            ),
            r(
                "3",
                "vm.start",
                "libvirt connect failed: socket",
                "2026-10-07 10:06:00",
            ),
        ]);
        assert_eq!(g.len(), 2);
        assert_eq!(
            (
                g[0].operation.as_str(),
                g[0].count,
                g[0].sample_task_id.as_str()
            ),
            ("host.sync", 2, "2")
        );
        assert!(g[0].hint.is_some());
    }

    #[tokio::test]
    async fn acknowledging_removes_failures_from_the_open_count() {
        let pool = crate::engine::test_support::test_pool().await;
        for (op, st) in [
            ("host.sync", "failed"),
            ("host.sync", "failed"),
            ("vm.start", "failed"),
            ("vm.stop", "completed"),
        ] {
            crate::db::query(
                "INSERT INTO tasks (id, operation, status, message) VALUES (?, ?, ?, 'boom')",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(op)
            .bind(st)
            .execute(&pool)
            .await
            .unwrap();
        }
        let open = |pool: crate::db::DbPool| async move {
            crate::db::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM tasks WHERE status = 'failed' AND acknowledged_at IS NULL",
            )
            .fetch_one(&pool)
            .await
            .unwrap()
        };
        assert_eq!(open(pool.clone()).await, 3);
        crate::db::query("UPDATE tasks SET acknowledged_at = '2026-10-07 10:00:00' WHERE status = 'failed' AND operation = 'host.sync'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(open(pool).await, 1);
    }
}
