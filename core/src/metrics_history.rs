// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Host + VM metrics time-series snapshots (JSON Lines under `/var/lib/machina`).

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::state::VmMetrics;
use crate::LibvirtError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsHistoryPoint {
    pub timestamp_ms: i64,
    pub host_cpu_percent: f64,
    pub host_memory_percent: f64,
    pub host_disk_percent: f64,
    pub load_1: f64,
    pub vms_running: u32,
    pub vm_count: u32,
    pub vm_metrics: Vec<VmMetrics>,
}

pub fn metrics_history_jsonl_path() -> PathBuf {
    PathBuf::from("/var/lib/machina/metrics-history.jsonl")
}

fn io_err(msg: impl Into<String>) -> LibvirtError {
    LibvirtError::Operation(msg.into())
}

fn trim_jsonl_to_budget(path: &Path, target_max_bytes: usize) -> Result<(), LibvirtError> {
    let data =
        fs::read_to_string(path).map_err(|e| io_err(format!("read {}: {e}", path.display())))?;
    if data.len() <= target_max_bytes {
        return Ok(());
    }
    let lines: Vec<&str> = data.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut kept: Vec<&str> = Vec::new();
    let mut size = 0usize;
    for line in lines.iter().rev() {
        let need = line.len() + 1;
        if !kept.is_empty() && size + need > target_max_bytes {
            break;
        }
        kept.push(*line);
        size += need;
    }
    kept.reverse();
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    fs::write(path, out).map_err(|e| io_err(format!("rewrite {}: {e}", path.display())))?;
    tracing::info!(
        "trimmed metrics history {} to ~{} bytes ({} points)",
        path.display(),
        size,
        kept.len()
    );
    Ok(())
}

/// Append one snapshot; trims oldest lines when the file exceeds `max_file_mb` (0 = no trim).
pub fn append_metrics_history_point(
    point: &MetricsHistoryPoint,
    max_file_mb: u64,
) -> Result<(), LibvirtError> {
    let path = metrics_history_jsonl_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut line = serde_json::to_string(point)
        .map_err(|e| LibvirtError::Internal(format!("metrics history JSON: {e}")))?;
    line.push('\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| io_err(format!("open metrics history: {e}")))?;
    file.write_all(line.as_bytes())
        .map_err(|e| io_err(format!("write metrics history: {e}")))?;

    if max_file_mb > 0 {
        if let Ok(meta) = fs::metadata(&path) {
            let max_bytes = max_file_mb.saturating_mul(1024 * 1024);
            if meta.len() > max_bytes {
                let target = ((max_bytes as usize).saturating_mul(85) / 100).max(4096);
                trim_jsonl_to_budget(&path, target)?;
            }
        }
    }
    Ok(())
}

/// Load the most recent `limit` snapshots from disk (newest last).
pub fn load_metrics_history_points(limit: usize) -> Vec<MetricsHistoryPoint> {
    let path = metrics_history_jsonl_path();
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let lim = limit.max(1);
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .rev()
        .take(lim)
        .filter_map(|line| serde_json::from_str::<MetricsHistoryPoint>(line).ok())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}
