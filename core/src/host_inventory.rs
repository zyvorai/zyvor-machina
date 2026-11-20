// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Host hardware inventory from Linux sysfs (CPU topology, NUMA) and SMBIOS/DMI tables under
//! `/sys/class/dmi/id` — same conceptual sources ESXi uses (firmware-exposed identity + ACPI/SMBIOS),
//! without requiring CPUID from userspace.
//!
//! Libvirt [`crate::state::NodeInfo`] is merged for comparison (inventory “audit” notes when counts differ).

use crate::state::NodeInfo;
use crate::LibvirtError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmiInventory {
    /// `/sys/class/dmi/id/product_uuid` when present (stable hardware identity when not suppressed by firmware).
    pub product_uuid: Option<String>,
    pub product_serial: String,
    pub sys_vendor: String,
    pub product_name: String,
    pub board_vendor: String,
    pub board_name: String,
    pub bios_version: String,
    pub bios_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuTopologySysfs {
    pub logical_cpus: u32,
    pub sockets: u32,
    pub socket_package_ids: Vec<i32>,
    /// Distinct physical cores (each `(socket, core_id)` pair counts once; SMT siblings share a core).
    pub physical_cores: u32,
    pub threads_per_core_max: u32,
    /// Core count per socket (`socket_package_ids` order).
    pub cores_per_socket: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumaNodeInventory {
    pub node_id: u32,
    pub cpu_list: String,
    pub memory_total_kb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibvirtCpuCapsule {
    pub cpu_model: String,
    pub cpu_sockets: u32,
    pub cpu_cores: u32,
    pub cpu_threads: u32,
    pub numa_nodes: u32,
    pub memory_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareInventoryReport {
    pub collected_at_rfc3339: String,
    /// Where each section was read from (paths / APIs).
    pub sources: Vec<String>,
    pub dmi: DmiInventory,
    pub cpu_topology: CpuTopologySysfs,
    pub numa_nodes: Vec<NumaNodeInventory>,
    pub cpuinfo_vendor_id: Option<String>,
    pub cpuinfo_model_name: Option<String>,
    pub libvirt: Option<LibvirtCpuCapsule>,
    /// `numa_nodes × sockets × cores × threads` as reported by libvirt’s node info (common interpretation).
    pub libvirt_logical_cpus_derived: Option<u32>,
    pub consistency_notes: Vec<String>,
}

fn io_err(msg: impl Into<String>) -> LibvirtError {
    LibvirtError::Operation(msg.into())
}

fn read_trim(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path.as_ref())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn read_dmi_field(name: &str) -> String {
    read_trim(format!("/sys/class/dmi/id/{name}"))
}

pub fn read_dmi_inventory() -> DmiInventory {
    let uuid_raw = read_dmi_field("product_uuid");
    let product_uuid = if uuid_raw.is_empty()
        || uuid_raw.eq_ignore_ascii_case("Not Applicable")
        || uuid_raw.eq_ignore_ascii_case("None")
    {
        None
    } else {
        Some(uuid_raw)
    };
    DmiInventory {
        product_uuid,
        product_serial: read_dmi_field("product_serial"),
        sys_vendor: read_dmi_field("sys_vendor"),
        product_name: read_dmi_field("product_name"),
        board_vendor: read_dmi_field("board_vendor"),
        board_name: read_dmi_field("board_name"),
        bios_version: read_dmi_field("bios_version"),
        bios_date: read_dmi_field("bios_date"),
    }
}

fn read_i32_file(path: &Path) -> Result<i32, LibvirtError> {
    let s =
        fs::read_to_string(path).map_err(|e| io_err(format!("read {}: {e}", path.display())))?;
    s.trim()
        .parse()
        .map_err(|_| io_err(format!("parse integer {}", path.display())))
}

fn parse_cpu_topology_sysfs() -> Result<CpuTopologySysfs, LibvirtError> {
    let base = Path::new("/sys/devices/system/cpu");
    if !base.exists() {
        return Ok(CpuTopologySysfs {
            logical_cpus: 0,
            sockets: 0,
            socket_package_ids: Vec::new(),
            physical_cores: 0,
            threads_per_core_max: 0,
            cores_per_socket: Vec::new(),
        });
    }

    let mut rows: Vec<(u32, i32, i32)> = Vec::new();
    for entry in fs::read_dir(base).map_err(|e| io_err(format!("read cpu sysfs: {e}")))? {
        let entry = entry.map_err(|e| io_err(format!("cpu dir: {e}")))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(rest) = name.strip_prefix("cpu") else {
            continue;
        };
        if !rest.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let cpu_id: u32 = rest
            .parse()
            .map_err(|_| io_err(format!("bad cpu id in {name}")))?;
        let pkg_path: PathBuf = base.join(&name).join("topology/physical_package_id");
        let core_path: PathBuf = base.join(&name).join("topology/core_id");
        if !pkg_path.exists() || !core_path.exists() {
            continue;
        }
        let pkg = read_i32_file(&pkg_path)?;
        let core = read_i32_file(&core_path)?;
        rows.push((cpu_id, pkg, core));
    }

    rows.sort_by_key(|r| r.0);

    let mut packages: BTreeSet<i32> = BTreeSet::new();
    for (_, p, _) in &rows {
        packages.insert(*p);
    }
    let socket_package_ids: Vec<i32> = packages.iter().copied().collect();
    let sockets = socket_package_ids.len() as u32;

    let mut core_keys: BTreeSet<(i32, i32)> = BTreeSet::new();
    for (_, p, c) in &rows {
        core_keys.insert((*p, *c));
    }
    let physical_cores = core_keys.len() as u32;

    let mut per_core_count: BTreeMap<(i32, i32), u32> = BTreeMap::new();
    for (_, p, c) in &rows {
        *per_core_count.entry((*p, *c)).or_insert(0) += 1;
    }
    let threads_per_core_max = per_core_count.values().copied().max().unwrap_or(1);

    let mut per_socket_cores: BTreeMap<i32, BTreeSet<i32>> = BTreeMap::new();
    for (_, p, c) in &rows {
        per_socket_cores.entry(*p).or_default().insert(*c);
    }
    let cores_per_socket: Vec<u32> = socket_package_ids
        .iter()
        .map(|sid| {
            per_socket_cores
                .get(sid)
                .map(|s| s.len() as u32)
                .unwrap_or(0)
        })
        .collect();

    Ok(CpuTopologySysfs {
        logical_cpus: rows.len() as u32,
        sockets,
        socket_package_ids,
        physical_cores,
        threads_per_core_max,
        cores_per_socket,
    })
}

fn parse_memtotal_kb(path: &Path) -> u64 {
    let text = fs::read_to_string(path).unwrap_or_default();
    for line in text.lines() {
        if line.starts_with("MemTotal:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(kb) = parts[1].parse::<u64>() {
                    return kb;
                }
            }
        }
    }
    0
}

fn parse_numa_nodes_sysfs() -> Vec<NumaNodeInventory> {
    let dir = Path::new("/sys/devices/system/node");
    if !dir.exists() {
        return Vec::new();
    }
    let mut out: Vec<NumaNodeInventory> = Vec::new();
    let Ok(read_dir) = fs::read_dir(dir) else {
        return Vec::new();
    };
    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(idx) = name.strip_prefix("node") else {
            continue;
        };
        if !idx.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let node_id: u32 = idx.parse().unwrap_or(0);
        let node_dir = dir.join(&name);
        let cpu_list = read_trim(node_dir.join("cpulist"));
        let memory_total_kb = parse_memtotal_kb(&node_dir.join("meminfo"));
        out.push(NumaNodeInventory {
            node_id,
            cpu_list,
            memory_total_kb,
        });
    }
    out.sort_by_key(|n| n.node_id);
    out
}

fn parse_proc_cpuinfo_vendor_model() -> (Option<String>, Option<String>) {
    let Ok(text) = fs::read_to_string("/proc/cpuinfo") else {
        return (None, None);
    };
    let mut vendor = None;
    let mut model = None;
    for line in text.lines() {
        if vendor.is_none() && line.starts_with("vendor_id") {
            vendor = line.split(':').nth(1).map(|s| s.trim().to_string());
        }
        if model.is_none() && line.starts_with("model name") {
            model = line.split(':').nth(1).map(|s| s.trim().to_string());
        }
        if vendor.is_some() && model.is_some() {
            break;
        }
    }
    (vendor, model)
}

/// Build a VMware-style inventory report: sysfs + DMI + `/proc/cpuinfo`, optionally reconciled with libvirt node caps.
pub fn gather_hardware_inventory_report(
    libvirt: Option<NodeInfo>,
) -> Result<HardwareInventoryReport, LibvirtError> {
    let dmi = read_dmi_inventory();
    let cpu_topology = parse_cpu_topology_sysfs()?;
    let numa_nodes = parse_numa_nodes_sysfs();
    let (cpuinfo_vendor_id, cpuinfo_model_name) = parse_proc_cpuinfo_vendor_model();

    let mut sources = vec![
        "sysfs:/sys/devices/system/cpu/*/topology".to_string(),
        "sysfs:/sys/devices/system/node/*/meminfo".to_string(),
        "dmi:/sys/class/dmi/id".to_string(),
        "proc:/proc/cpuinfo".to_string(),
    ];

    let lib_caps = libvirt.map(|n| LibvirtCpuCapsule {
        cpu_model: n.cpu_model.clone(),
        cpu_sockets: n.cpu_sockets,
        cpu_threads: n.cpu_threads,
        cpu_cores: n.cpu_cores,
        numa_nodes: n.numa_nodes,
        memory_mb: n.memory_mb,
    });

    let libvirt_logical_cpus_derived = lib_caps.as_ref().map(|c| {
        c.numa_nodes
            .saturating_mul(c.cpu_sockets)
            .saturating_mul(c.cpu_cores)
            .saturating_mul(c.cpu_threads)
    });

    if lib_caps.is_some() {
        sources.push("libvirt:virConnectGetNodeInfo".to_string());
    }

    let mut consistency_notes: Vec<String> = Vec::new();

    if cpu_topology.logical_cpus == 0 && Path::new("/sys/devices/system/cpu").exists() {
        consistency_notes.push(
            "Could not parse CPU topology under /sys/devices/system/cpu (unexpected layout?)."
                .into(),
        );
    }

    if let (Some(lv), sysfs_n) = (libvirt_logical_cpus_derived, cpu_topology.logical_cpus) {
        if lv != sysfs_n && sysfs_n > 0 {
            consistency_notes.push(format!(
                "Logical CPU count differs: sysfs lists {sysfs_n} online CPUs; libvirt node dimensions multiply to {lv} (NUMA×socket×core×thread). Topology accounting differs between/kernel and libvirt."
            ));
        }
    }

    if let Some(ref c) = lib_caps {
        if c.cpu_sockets != cpu_topology.sockets && cpu_topology.sockets > 0 {
            consistency_notes.push(format!(
                "Socket count differs: sysfs distinct physical_package_id values {}, libvirt reports {} sockets per NUMA cell.",
                cpu_topology.sockets, c.cpu_sockets
            ));
        }
        let sysfs_numa = numa_nodes.len() as u32;
        if sysfs_numa > 0 && c.numa_nodes != sysfs_numa {
            consistency_notes.push(format!(
                "NUMA node count differs: sysfs lists {sysfs_numa} nodes, libvirt reports {}.",
                c.numa_nodes
            ));
        }
    }

    Ok(HardwareInventoryReport {
        collected_at_rfc3339: chrono::Utc::now().to_rfc3339(),
        sources,
        dmi,
        cpu_topology,
        numa_nodes,
        cpuinfo_vendor_id,
        cpuinfo_model_name,
        libvirt: lib_caps,
        libvirt_logical_cpus_derived,
        consistency_notes,
    })
}

/// Append-only JSON Lines store next to other Machina state (`audit.log`).
pub fn inventory_history_jsonl_path() -> PathBuf {
    PathBuf::from("/var/lib/machina/hardware-inventory.jsonl")
}

fn trim_jsonl_file_to_budget(path: &Path, target_max_bytes: usize) -> Result<(), LibvirtError> {
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
    let bytes = out.len();
    fs::write(path, out).map_err(|e| io_err(format!("rewrite {}: {e}", path.display())))?;
    tracing::info!(
        "trimmed {} to ~{} bytes (kept {} snapshots)",
        path.display(),
        bytes,
        kept.len()
    );
    Ok(())
}

/// Append one snapshot line; trims oldest records when the file exceeds `max_file_bytes` (0 = no trim).
pub fn append_inventory_history_line(
    report: &HardwareInventoryReport,
    max_file_bytes: u64,
) -> Result<(), LibvirtError> {
    let path = inventory_history_jsonl_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut line = serde_json::to_string(report)
        .map_err(|e| LibvirtError::Internal(format!("inventory JSON: {e}")))?;
    line.push('\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| LibvirtError::Operation(format!("open inventory history: {e}")))?;
    file.write_all(line.as_bytes())
        .map_err(|e| LibvirtError::Operation(format!("write inventory history: {e}")))?;

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

/// Recent snapshots, **newest first** (each line is a full [`HardwareInventoryReport`] JSON).
pub fn load_inventory_history_entries(
    limit: usize,
) -> Result<Vec<HardwareInventoryReport>, LibvirtError> {
    let path = inventory_history_jsonl_path();
    let data = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    let lim = limit.clamp(1, 50_000);
    let out: Vec<HardwareInventoryReport> = data
        .lines()
        .filter(|l| !l.trim().is_empty())
        .rev()
        .filter_map(|line| serde_json::from_str(line).ok())
        .take(lim)
        .collect();
    Ok(out)
}
