// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Append cluster inventory snapshots to JSON Lines (`/var/lib/machina/k8s-cluster-inventory.jsonl`).

use machina_core::LibvirtError;
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn k8s_cluster_inventory_jsonl_path() -> PathBuf {
    PathBuf::from("/var/lib/machina/k8s-cluster-inventory.jsonl")
}

fn trim_jsonl_file_to_budget(path: &Path, target_max_bytes: usize) -> Result<(), LibvirtError> {
    let data = fs::read_to_string(path)
        .map_err(|e| LibvirtError::Operation(format!("read {}: {e}", path.display())))?;
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
    fs::write(path, out)
        .map_err(|e| LibvirtError::Operation(format!("rewrite {}: {e}", path.display())))?;
    Ok(())
}

/// Append one snapshot line (full JSON object per line). Trims oldest lines when over budget.
pub fn append_k8s_cluster_inventory_line(
    json_line: &str,
    max_file_bytes: u64,
) -> Result<(), LibvirtError> {
    let path = k8s_cluster_inventory_jsonl_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut line = json_line.trim().to_string();
    if !line.ends_with('\n') {
        line.push('\n');
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| LibvirtError::Operation(format!("open k8s inventory history: {e}")))?;
    file.write_all(line.as_bytes())
        .map_err(|e| LibvirtError::Operation(format!("write k8s inventory history: {e}")))?;

    if max_file_bytes > 0 {
        if let Ok(meta) = fs::metadata(&path) {
            let len = meta.len();
            if len > max_file_bytes {
                let target = ((max_file_bytes as usize).saturating_mul(85) / 100).max(4096);
                trim_jsonl_file_to_budget(&path, target)?;
            }
        }
    }
    Ok(())
}

/// Newest-first JSON values (may be large — keep `limit` modest).
pub fn load_k8s_cluster_inventory_history(limit: usize) -> Result<Vec<Value>, LibvirtError> {
    let path = k8s_cluster_inventory_jsonl_path();
    let data = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    let lim = limit.clamp(1, 500);
    let out: Vec<Value> = data
        .lines()
        .filter(|l| !l.trim().is_empty())
        .rev()
        .filter_map(|line| serde_json::from_str(line).ok())
        .take(lim)
        .collect();
    Ok(out)
}
