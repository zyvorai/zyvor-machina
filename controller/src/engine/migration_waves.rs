// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Migration waves: the pure rules behind `api/migration_waves.rs`. A wave is an ordered list of
//! VMs to move in with Transiva/h2kvm. Each `advance` call asks Transiva how the started jobs are
//! doing, then starts the next planned items while fewer than `max_parallel` are in flight. The
//! functions here decide; the API does the HTTP and the database writes.

use serde_json::Value;
use uuid::Uuid;

pub const MAX_ITEMS: usize = 200;
pub const MAX_PARALLEL_LIMIT: u32 = 20;
pub const DEFAULT_PARALLEL: u32 = 3;
pub const MAX_CONFIG_BYTES: usize = 64 * 1024;

/// One item as the planner sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemState {
    pub id: Uuid,
    pub position: i64,
    pub status: String,
}

fn in_flight(status: &str) -> bool {
    matches!(status, "submitted" | "running")
}

/// Items to start now: planned ones in `position` order, enough to fill `max_parallel` slots.
pub fn next_to_start(items: &[ItemState], max_parallel: u32) -> Vec<Uuid> {
    let active = items.iter().filter(|i| in_flight(&i.status)).count();
    let free = (max_parallel as usize).saturating_sub(active);
    let mut planned: Vec<&ItemState> = items.iter().filter(|i| i.status == "planned").collect();
    planned.sort_by_key(|i| i.position);
    planned.into_iter().take(free).map(|i| i.id).collect()
}

/// Map a Transiva job status to an item status. Unknown values keep the current status.
pub fn item_status_from_job(job_status: &str, current: &str) -> String {
    match job_status {
        "pending" => "submitted",
        "running" => "running",
        "completed" => "completed",
        "failed" => "failed",
        "cancelled" => "cancelled",
        _ => current,
    }
    .to_string()
}

/// `job_id` from a Transiva submit response (`{"job_id":"abc","message":"..."}`).
pub fn job_id_from_submit(resp: &Value) -> Option<String> {
    resp.get("job_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 128
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .map(str::to_string)
}

/// Job fields we keep from `GET /api/v1/jobs/{id}`: status, phase, percent, error.
#[derive(Debug, Clone, PartialEq)]
pub struct JobView {
    pub status: String,
    pub phase: String,
    pub percent: i64,
    pub error: String,
}

pub fn job_view(job: &Value) -> Option<JobView> {
    let status = job.get("status")?.as_str()?.to_string();
    let progress = job.get("progress");
    Some(JobView {
        status,
        phase: progress
            .and_then(|p| p.get("phase"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        percent: progress
            .and_then(|p| p.get("percent_complete"))
            .and_then(Value::as_f64)
            .map(|f| f.clamp(0.0, 100.0).round() as i64)
            .unwrap_or(0),
        error: job
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .take(500)
            .collect(),
    })
}

/// Overall wave status from its items: `cancelled` stays cancelled; otherwise any in-flight item
/// is `running`; all completed is `done`; a failure with nothing in flight is `attention`.
pub fn wave_status(current: &str, items: &[ItemState]) -> &'static str {
    if current == "cancelled" {
        return "cancelled";
    }
    if items.is_empty() {
        return "planned";
    }
    if items.iter().any(|i| in_flight(&i.status)) {
        return "running";
    }
    if items.iter().all(|i| i.status == "completed") {
        return "done";
    }
    if items.iter().any(|i| i.status == "failed") {
        return "attention";
    }
    if items
        .iter()
        .any(|i| i.status == "completed" || i.status == "cancelled")
    {
        // Some finished and the rest are still planned or cancelled: waiting for the next advance.
        return if items.iter().any(|i| i.status == "planned") {
            "running"
        } else {
            "attention"
        };
    }
    "planned"
}

/// `ready` and `cut_over` only make sense for an item that migrated successfully.
pub fn cutover_allowed(item_status: &str, target: &str) -> Result<(), String> {
    match target {
        "none" => Ok(()),
        "ready" | "cut_over" if item_status == "completed" => Ok(()),
        "ready" | "cut_over" => Err(format!(
            "cutover '{target}' needs a completed migration (this item is {item_status})"
        )),
        _ => Err("cutover must be none, ready or cut_over".into()),
    }
}

pub fn validate_item_name(n: &str) -> Result<(), String> {
    let n = n.trim();
    if n.is_empty() || n.len() > 128 || n.chars().any(char::is_control) {
        return Err("item name must be 1-128 printable characters".into());
    }
    Ok(())
}

pub fn validate_config(c: &Value) -> Result<String, String> {
    if !c.is_object() {
        return Err("item config must be a JSON object".into());
    }
    let s = c.to_string();
    if s.len() > MAX_CONFIG_BYTES {
        return Err("item config is larger than 64 KiB".into());
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(position: i64, status: &str) -> ItemState {
        ItemState {
            id: Uuid::new_v4(),
            position,
            status: status.into(),
        }
    }

    #[test]
    fn starts_planned_items_in_order_up_to_the_free_slots() {
        let items = vec![
            item(2, "planned"),
            item(0, "running"),
            item(1, "planned"),
            item(3, "planned"),
        ];
        // 3 slots, 1 active: two planned items start, lowest positions first.
        let started = next_to_start(&items, 3);
        assert_eq!(started, vec![items[2].id, items[0].id]);
        // Already full: nothing starts.
        assert!(next_to_start(&items, 1).is_empty());
        // Failed and completed items do not count as active.
        let done = vec![item(0, "failed"), item(1, "completed"), item(2, "planned")];
        assert_eq!(next_to_start(&done, 1), vec![done[2].id]);
    }

    #[test]
    fn job_statuses_map_and_unknown_is_kept() {
        assert_eq!(item_status_from_job("pending", "planned"), "submitted");
        assert_eq!(item_status_from_job("running", "submitted"), "running");
        assert_eq!(item_status_from_job("completed", "running"), "completed");
        assert_eq!(item_status_from_job("failed", "running"), "failed");
        assert_eq!(item_status_from_job("cancelled", "running"), "cancelled");
        assert_eq!(item_status_from_job("weird", "running"), "running");
    }

    #[test]
    fn parses_submit_and_job_responses() {
        assert_eq!(
            job_id_from_submit(&json!({"job_id": "abc12345", "message": "migration submitted"}))
                .as_deref(),
            Some("abc12345")
        );
        assert_eq!(job_id_from_submit(&json!({"job_id": ""})), None);
        // The id becomes part of a URL path, so anything else is refused.
        assert_eq!(job_id_from_submit(&json!({"job_id": "../admin"})), None);
        assert_eq!(job_id_from_submit(&json!({"job_id": "a b"})), None);
        assert_eq!(job_id_from_submit(&json!({"jobs": []})), None);
        let v = job_view(&json!({
            "id": "abc", "status": "running",
            "progress": {"phase": "convert", "percent_complete": 42.4},
            "error": ""
        }))
        .unwrap();
        assert_eq!(
            v,
            JobView {
                status: "running".into(),
                phase: "convert".into(),
                percent: 42,
                error: String::new()
            }
        );
        assert!(job_view(&json!({"id": "x"})).is_none());
        assert_eq!(
            job_view(&json!({"status": "failed", "error": "disk full"}))
                .unwrap()
                .error,
            "disk full"
        );
        assert_eq!(
            job_view(&json!({"status": "running", "progress": {"percent_complete": 250.0}}))
                .unwrap()
                .percent,
            100
        );
    }

    #[test]
    fn wave_status_rules() {
        assert_eq!(wave_status("planned", &[]), "planned");
        assert_eq!(wave_status("planned", &[item(0, "planned")]), "planned");
        assert_eq!(
            wave_status("planned", &[item(0, "running"), item(1, "failed")]),
            "running"
        );
        assert_eq!(
            wave_status("running", &[item(0, "completed"), item(1, "completed")]),
            "done"
        );
        assert_eq!(
            wave_status("running", &[item(0, "completed"), item(1, "failed")]),
            "attention"
        );
        assert_eq!(
            wave_status("running", &[item(0, "completed"), item(1, "planned")]),
            "running"
        );
        assert_eq!(wave_status("cancelled", &[item(0, "running")]), "cancelled");
    }

    #[test]
    fn cutover_needs_a_completed_item() {
        assert!(cutover_allowed("completed", "ready").is_ok());
        assert!(cutover_allowed("completed", "cut_over").is_ok());
        assert!(cutover_allowed("running", "ready").is_err());
        assert!(cutover_allowed("failed", "none").is_ok());
        assert!(cutover_allowed("completed", "bogus").is_err());
    }

    #[test]
    fn validates_names_and_configs() {
        assert!(validate_item_name("web-01").is_ok());
        assert!(validate_item_name("  ").is_err());
        assert!(validate_item_name("a\nb").is_err());
        assert!(validate_config(&json!({"command": "vsphere"})).is_ok());
        assert!(validate_config(&json!(["x"])).is_err());
        assert!(validate_config(&json!({"blob": "x".repeat(70_000)})).is_err());
    }
}
