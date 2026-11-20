// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! History-based rightsizing. A VM's hourly CPU and memory peaks over the last
//! two weeks give a 95th percentile; the suggestion sizes it so that peak sits
//! near 60% CPU and 1.3x memory. Applied through the `vm.resize` approval
//! action, which records the old size so it can be undone.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::db::DbPool;
use uuid::Uuid;

use crate::api::ApiError;
use crate::engine::ai::{forecast, idle};
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

pub const RESIZE_ACTION: &str = "vm.resize";
/// Fewest hourly points (three days) before a VM gets a suggestion.
pub const MIN_HOURS: usize = 72;
const DAYS: i64 = 14;
const CPU_TARGET: f64 = 0.6;
const MEM_HEADROOM: f64 = 1.3;
const MEM_STEP: i64 = 256;
const MEM_FLOOR: i64 = 512;

pub fn percentile(values: &[f64], p: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let rank = ((p / 100.0) * (v.len() - 1) as f64).round() as usize;
    Some(v[rank.min(v.len() - 1)])
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suggestion {
    pub vcpus: u32,
    pub memory_mib: i64,
    pub reasons: Vec<String>,
}

fn round_up(v: f64, step: i64) -> i64 {
    ((v / step as f64).ceil() as i64) * step
}

/// `cpu_p95`: percent of the VM's vCPUs; `mem_p95`: fraction of its memory.
/// None when the VM is about the right size.
pub fn suggest(
    vcpus: u32,
    memory_mib: i64,
    cpu_p95: f64,
    mem_p95: Option<f64>,
) -> Option<Suggestion> {
    let mut reasons = Vec::new();
    let used_cpus = f64::from(vcpus) * cpu_p95 / 100.0;
    let mut new_vcpus =
        ((used_cpus / CPU_TARGET).ceil() as u32).clamp(1, vcpus.saturating_mul(2).max(1));
    if cpu_p95 > 85.0 {
        new_vcpus = new_vcpus.max(vcpus + 1);
    }
    if new_vcpus < vcpus {
        reasons.push(format!("CPU peaks at {cpu_p95:.0}% of {vcpus} vCPUs (p95)"));
    } else if new_vcpus > vcpus {
        reasons.push(format!(
            "CPU runs hot: {cpu_p95:.0}% of {vcpus} vCPUs at p95"
        ));
    }
    let mut new_mem = memory_mib;
    if let Some(m) = mem_p95.filter(|m| m.is_finite() && *m > 0.0) {
        let want = if m > 0.9 {
            round_up(memory_mib as f64 * 1.25, MEM_STEP)
        } else {
            round_up(memory_mib as f64 * m * MEM_HEADROOM, MEM_STEP).max(MEM_FLOOR)
        };
        let delta = (want - memory_mib).abs();
        if delta >= MEM_STEP && delta * 4 >= memory_mib {
            new_mem = want;
            reasons.push(format!(
                "memory peaks at {:.0}% of {} MiB (p95)",
                m * 100.0,
                memory_mib
            ));
        }
    }
    (new_vcpus != vcpus || new_mem != memory_mib).then_some(Suggestion {
        vcpus: new_vcpus,
        memory_mib: new_mem,
        reasons,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    pub vm_id: Uuid,
    pub name: String,
    pub project: String,
    pub vcpus: u32,
    pub memory_mib: i64,
    pub suggested_vcpus: u32,
    pub suggested_memory_mib: i64,
    pub cpu_p95: f64,
    pub mem_p95: Option<f64>,
    pub hours: usize,
    /// Negative is a saving.
    pub monthly_delta_usd: f64,
    pub reasons: Vec<String>,
    pub pending_action: Option<Uuid>,
}

pub async fn rates(pool: &DbPool) -> (f64, f64) {
    crate::db::query_as(
        "SELECT finops_vcpu_hour_usd, finops_gib_hour_usd FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .unwrap_or((0.02, 0.005))
}

pub async fn recommend(pool: &DbPool) -> anyhow::Result<Vec<Recommendation>> {
    let (vcpu_rate, gib_rate) = rates(pool).await;
    let vms: Vec<(Uuid, String, String, i64, i64)> = crate::db::query_as(
        "SELECT id, name, COALESCE(project, 'default'), vcpus, memory_mib FROM vms
         WHERE observed_state = 'running' AND vcpus > 0 AND memory_mib > 0
         ORDER BY name LIMIT 500",
    )
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for (id, name, project, vcpus, memory_mib) in vms {
        // History from before a resize doesn't describe the new size, and a
        // size the guest couldn't take live only shows after its next restart.
        let resized: i64 = crate::db::query_scalar(
            "SELECT COUNT(*) FROM ai_actions WHERE action_type = ? AND status = 'executed'
             AND undone_at IS NULL AND executed_at > datetime('now', ?)
             AND json_extract(object_ref, '$.vm_id') = ?",
        )
        .bind(RESIZE_ACTION)
        .bind(format!("-{DAYS} days"))
        .bind(id.to_string())
        .fetch_one(pool)
        .await?;
        if resized > 0 {
            continue;
        }
        let cpu = forecast::hourly_vm(pool, id, "cpu_percent", DAYS, true).await?;
        if cpu.len() < MIN_HOURS {
            continue;
        }
        let cpu: Vec<f64> = cpu.into_values().collect();
        let mem: Vec<f64> = forecast::hourly_vm(pool, id, "mem_ratio", DAYS, true)
            .await?
            .into_values()
            .collect();
        let Some(cpu_p95) = percentile(&cpu, 95.0) else {
            continue;
        };
        let mem_p95 = (mem.len() >= MIN_HOURS)
            .then(|| percentile(&mem, 95.0))
            .flatten();
        let vcpus = vcpus.max(1) as u32;
        let Some(s) = suggest(vcpus, memory_mib, cpu_p95, mem_p95) else {
            continue;
        };
        let pending: Option<Uuid> = crate::db::query_scalar(
            "SELECT id FROM ai_actions WHERE action_type = ? AND status = 'pending'
             AND json_extract(object_ref, '$.vm_id') = ? LIMIT 1",
        )
        .bind(RESIZE_ACTION)
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
        let before = idle::monthly_cost(i64::from(vcpus), memory_mib, vcpu_rate, gib_rate);
        let after = idle::monthly_cost(i64::from(s.vcpus), s.memory_mib, vcpu_rate, gib_rate);
        out.push(Recommendation {
            vm_id: id,
            name,
            project,
            vcpus,
            memory_mib,
            suggested_vcpus: s.vcpus,
            suggested_memory_mib: s.memory_mib,
            cpu_p95,
            mem_p95,
            hours: cpu.len(),
            monthly_delta_usd: ((after - before) * 100.0).round() / 100.0,
            reasons: s.reasons,
            pending_action: pending,
        });
    }
    out.sort_by(|a, b| a.monthly_delta_usd.total_cmp(&b.monthly_delta_usd));
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResizeRef {
    pub vm_id: Uuid,
    pub name: String,
    pub vcpus: u32,
    pub memory_mib: i64,
    pub from_vcpus: u32,
    pub from_memory_mib: i64,
}

fn bad_ref(e: impl std::fmt::Display) -> ApiError {
    ApiError::bad_request(format!("resize object_ref: {e}"))
}

async fn current(state: &AppState, vm: Uuid) -> Result<(String, Option<Uuid>, i64, i64), ApiError> {
    crate::db::query_as("SELECT name, host_id, vcpus, memory_mib FROM vms WHERE id = ?")
        .bind(vm)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("vm not found"))
}

/// Queues `vm.resize` tasks for whatever differs from the VM's current size.
async fn resize_to(
    state: &AppState,
    vm: Uuid,
    vcpus: u32,
    memory_mib: i64,
) -> Result<Vec<Uuid>, ApiError> {
    let (_, host, cur_vcpus, cur_mem) = current(state, vm).await?;
    let mut tasks = Vec::new();
    if i64::from(vcpus) != cur_vcpus {
        tasks.push(
            enqueue_task(
                state,
                "vm.resize",
                json!({ "vm_id": vm.to_string(), "kind": "vcpus", "count": vcpus }),
                Some("vm"),
                Some(vm),
                host,
            )
            .await?,
        );
    }
    if memory_mib != cur_mem {
        tasks.push(
            enqueue_task(
                state,
                "vm.resize",
                json!({ "vm_id": vm.to_string(), "kind": "memory", "memory_mb": memory_mib }),
                Some("vm"),
                Some(vm),
                host,
            )
            .await?,
        );
    }
    Ok(tasks)
}

pub async fn propose_ref(
    state: &AppState,
    vm: Uuid,
    vcpus: u32,
    memory_mib: i64,
) -> Result<ResizeRef, ApiError> {
    if !(1..=256).contains(&vcpus) || !(256..=4 * 1024 * 1024).contains(&memory_mib) {
        return Err(ApiError::bad_request("vcpus 1-256, memory 256 MiB-4 TiB"));
    }
    let (name, _, cur_vcpus, cur_mem) = current(state, vm).await?;
    if i64::from(vcpus) == cur_vcpus && memory_mib == cur_mem {
        return Err(ApiError::bad_request("the VM already has that size"));
    }
    Ok(ResizeRef {
        vm_id: vm,
        name,
        vcpus,
        memory_mib,
        from_vcpus: cur_vcpus.max(0) as u32,
        from_memory_mib: cur_mem,
    })
}

pub async fn execute(state: &AppState, object_ref: &Value) -> Result<Value, ApiError> {
    let r: ResizeRef = serde_json::from_value(object_ref.clone()).map_err(bad_ref)?;
    let tasks = resize_to(state, r.vm_id, r.vcpus, r.memory_mib).await?;
    Ok(json!({
        "message": format!("Resizing {} to {} vCPUs and {} MiB", r.name, r.vcpus, r.memory_mib),
        "task_ids": tasks,
    }))
}

pub async fn before(state: &AppState, object_ref: &Value) -> Value {
    let Ok(r) = serde_json::from_value::<ResizeRef>(object_ref.clone()) else {
        return Value::Null;
    };
    match current(state, r.vm_id).await {
        Ok((_, _, vcpus, memory_mib)) => json!({
            "vm_id": r.vm_id.to_string(),
            "vcpus": vcpus,
            "memory_mib": memory_mib,
        }),
        Err(_) => Value::Null,
    }
}

pub async fn check(state: &AppState, object_ref: &Value) -> (&'static str, String) {
    let Ok(r) = serde_json::from_value::<ResizeRef>(object_ref.clone()) else {
        return ("unknown", "The resize request could not be read.".into());
    };
    match current(state, r.vm_id).await {
        Ok((_, _, vcpus, mem)) if vcpus == i64::from(r.vcpus) && mem == r.memory_mib => (
            "ok",
            format!("{} has {} vCPUs and {} MiB.", r.name, vcpus, mem),
        ),
        Ok((_, _, vcpus, mem)) => (
            "pending",
            format!(
                "{} runs with {} vCPUs and {} MiB; asked for {} and {}. \
                 A size the guest can't change live applies at its next restart.",
                r.name, vcpus, mem, r.vcpus, r.memory_mib
            ),
        ),
        Err(_) => ("unknown", "The machine was not found.".into()),
    }
}

pub async fn undo(state: &AppState, before: &Value) -> Result<String, ApiError> {
    let vm: Uuid = before
        .get("vm_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| ApiError::bad_request("no recorded size to go back to"))?;
    let vcpus = before.get("vcpus").and_then(Value::as_i64).unwrap_or(0);
    let mem = before
        .get("memory_mib")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if vcpus <= 0 || mem <= 0 {
        return Err(ApiError::bad_request("no recorded size to go back to"));
    }
    resize_to(state, vm, vcpus as u32, mem).await?;
    Ok(format!("Resizing back to {vcpus} vCPUs and {mem} MiB."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_is_nearest_rank() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(&v, 95.0), Some(95.0));
        assert_eq!(percentile(&[], 95.0), None);
    }

    #[test]
    fn oversized_vms_shrink_and_hot_ones_grow() {
        // 8 vCPUs peaking at 15% use 1.2 CPUs: 2 vCPUs keeps them at 60%.
        let s = suggest(8, 16384, 15.0, Some(0.2)).unwrap();
        assert_eq!(s.vcpus, 2);
        // 16 GiB at 20% is 3.2 GiB; x1.3 rounds up to 4352 MiB.
        assert_eq!(s.memory_mib, 4352);
        assert_eq!(s.reasons.len(), 2);
        let hot = suggest(2, 4096, 97.0, Some(0.95)).unwrap();
        assert_eq!(hot.vcpus, 4);
        assert_eq!(hot.memory_mib, 5120);
    }

    #[tokio::test]
    async fn history_drives_a_resize_that_can_be_undone() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        let host = crate::engine::test_support::seed_host(&state.pool, Uuid::from_u128(1)).await;
        let vm = Uuid::from_u128(2);
        crate::db::query(
            "INSERT INTO vms (id, host_id, name, desired_state, observed_state, vcpus, memory_mib)
             VALUES (?, ?, 'fat', 'running', 'running', 8, 16384)",
        )
        .bind(vm)
        .bind(host)
        .execute(&state.pool)
        .await
        .unwrap();
        let now = chrono::Utc::now().timestamp() / 3600 * 3600;
        for h in 1..=80 {
            for (metric, max) in [("cpu_percent", 15.0), ("mem_ratio", 0.2)] {
                crate::db::query("INSERT INTO metric_hourly (subject, metric, hour, avg, max, n) VALUES (?, ?, ?, ?, ?, 12)")
                    .bind(crate::db::subject_id(vm))
                    .bind(metric)
                    .bind(now - h * 3600)
                    .bind(max / 2.0)
                    .bind(max)
                    .execute(&state.pool)
                    .await
                    .unwrap();
            }
        }
        let recs = recommend(&state.pool).await.unwrap();
        assert_eq!(recs.len(), 1);
        let r = &recs[0];
        assert_eq!(
            (r.suggested_vcpus, r.suggested_memory_mib, r.hours),
            (2, 4352, 80)
        );
        assert!(r.monthly_delta_usd < 0.0);

        let rr = propose_ref(&state, vm, 2, 4352).await.unwrap();
        assert_eq!((rr.from_vcpus, rr.from_memory_mib), (8, 16384));
        assert!(
            propose_ref(&state, vm, 8, 16384).await.is_err(),
            "same size"
        );
        let object_ref = serde_json::to_value(&rr).unwrap();
        let snap = before(&state, &object_ref).await;
        assert_eq!(snap["vcpus"], 8);
        let out = execute(&state, &object_ref).await.unwrap();
        assert_eq!(out["task_ids"].as_array().unwrap().len(), 2);
        assert_eq!(check(&state, &object_ref).await.0, "pending");
        crate::db::query("UPDATE vms SET vcpus = 2, memory_mib = 4352 WHERE id = ?")
            .bind(vm)
            .execute(&state.pool)
            .await
            .unwrap();
        assert_eq!(check(&state, &object_ref).await.0, "ok");
        undo(&state, &snap).await.unwrap();
        let back: Vec<(String,)> = crate::db::query_as(
            "SELECT payload FROM tasks WHERE operation = 'vm.resize' ORDER BY created_at DESC, rowid DESC LIMIT 2",
        )
        .fetch_all(&state.pool)
        .await
        .unwrap();
        let joined: String = back.into_iter().map(|b| b.0).collect();
        assert!(
            joined.contains("\"count\":8") && joined.contains("\"memory_mb\":16384"),
            "{joined}"
        );

        crate::db::query(
            "INSERT INTO ai_actions (id, source, action_type, label, review, risk, object_ref, status, requested_by, executed_at)
             VALUES (?, 'rightsizing', ?, 'r', '', '', ?, 'executed', 't', datetime('now'))",
        )
        .bind(Uuid::new_v4())
        .bind(RESIZE_ACTION)
        .bind(&object_ref)
        .execute(&state.pool)
        .await
        .unwrap();
        assert!(
            recommend(&state.pool).await.unwrap().is_empty(),
            "no new suggestion right after a resize"
        );
    }

    #[test]
    fn about_right_is_left_alone() {
        assert_eq!(suggest(2, 4096, 55.0, Some(0.6)), None);
        // Small memory changes (<25%) are not worth a resize.
        assert_eq!(suggest(2, 4096, 55.0, Some(0.68)), None);
        assert_eq!(
            suggest(1, 512, 2.0, Some(0.1)),
            None,
            "floors: 1 vCPU, 512 MiB"
        );
    }
}
