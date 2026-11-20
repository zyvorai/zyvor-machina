// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Offline guest repair ("Boot Doctor"): diagnose and repair a powered-off VM's disk with GuestKit.
//!
//! Every operation refuses a running VM — writing into a disk a live guest has open would corrupt it.
//! Repairs ask GuestKit for a backup first, and a dry run previews the change without writing.

use std::path::PathBuf;
use std::process::Command;

use virt::connect::Connect;

use crate::LibvirtError;

/// Result of an offline diagnose or repair run.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GuestRepairReport {
    pub vm: String,
    pub disk: String,
    /// "diagnose" or "repair"
    pub action: String,
    pub dry_run: bool,
    pub backup: bool,
    /// GuestKit stdout/stderr (the tail; `diagnose` asks for JSON so the UI can render findings).
    pub output: String,
    pub exit_ok: bool,
}

/// Keep responses small: the tail of an unbounded tool log is the useful part.
fn tail(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let count = trimmed.chars().count();
    if count <= max_chars {
        return trimmed.to_string();
    }
    trimmed.chars().skip(count - max_chars).collect()
}

/// The VM's primary file-backed disk, only if the domain is powered off.
fn offline_disk(conn: &Connect, name: &str, what: &str) -> Result<PathBuf, LibvirtError> {
    let domain = super::domain::lookup_domain(conn, name)?;
    let active = domain.is_active().map_err(|e| {
        LibvirtError::Operation(format!("Failed to read state of VM '{name}': {e}"))
    })?;
    if active {
        return Err(LibvirtError::Invalid(format!(
            "VM '{name}' is running — shut it down before {what}"
        )));
    }
    let xml = domain
        .get_xml_desc(0)
        .map_err(|e| LibvirtError::Operation(format!("Failed to read XML of VM '{name}': {e}")))?;
    let disk = super::template_apply::primary_disk_path_from_xml(&xml).ok_or_else(|| {
        LibvirtError::Invalid(format!("VM '{name}' has no file-backed primary disk"))
    })?;
    if !disk.is_file() {
        return Err(LibvirtError::Invalid(format!(
            "Disk image {} for VM '{name}' is not a file on this host",
            disk.display()
        )));
    }
    Ok(disk)
}

fn run_guestkit(mut cmd: Command) -> Result<(String, bool), LibvirtError> {
    let out = cmd
        .output()
        .map_err(|e| LibvirtError::Operation(format!("Failed to run guestkit: {e}")))?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(err.trim());
    }
    Ok((tail(&text, 16000), out.status.success()))
}

/// `guestkit doctor --explain -o json` against the powered-off disk.
pub fn diagnose_offline(conn: &Connect, name: &str) -> Result<GuestRepairReport, LibvirtError> {
    let disk = offline_disk(conn, name, "diagnosing its disk")?;
    let mut cmd = Command::new("guestkit");
    cmd.arg("doctor")
        .arg(&disk)
        .args(["--explain", "-o", "json"]);
    let (output, exit_ok) = run_guestkit(cmd)?;
    Ok(GuestRepairReport {
        vm: name.to_string(),
        disk: disk.display().to_string(),
        action: "diagnose".into(),
        dry_run: false,
        backup: false,
        output,
        exit_ok,
    })
}

/// `guestkit repair --fix boot` (optionally `--dry-run`, and `-b` to back the disk up first).
pub fn repair_offline(
    conn: &Connect,
    name: &str,
    dry_run: bool,
    backup: bool,
) -> Result<GuestRepairReport, LibvirtError> {
    let disk = offline_disk(conn, name, "repairing its disk")?;
    let mut cmd = Command::new("guestkit");
    cmd.arg("repair").arg(&disk).args(["--fix", "boot"]);
    if dry_run {
        cmd.arg("--dry-run");
    }
    if backup && !dry_run {
        cmd.arg("--backup");
    }
    let (output, exit_ok) = run_guestkit(cmd)?;
    if !exit_ok && !dry_run {
        return Err(LibvirtError::Operation(format!(
            "guestkit repair failed for VM '{name}': {output}"
        )));
    }
    Ok(GuestRepairReport {
        vm: name.to_string(),
        disk: disk.display().to_string(),
        action: "repair".into(),
        dry_run,
        backup: backup && !dry_run,
        output,
        exit_ok,
    })
}

/// `guestkit drift <baseline> <current> -R`: how far `name`'s disk has moved from `baseline`'s. Both machines must
/// be powered off; GuestKit opens both images read-only, so nothing is written.
pub fn drift_offline(
    conn: &Connect,
    name: &str,
    baseline: &str,
) -> Result<GuestRepairReport, LibvirtError> {
    if name == baseline {
        return Err(LibvirtError::Invalid(
            "Pick a different machine to compare against".into(),
        ));
    }
    let base_disk = offline_disk(conn, baseline, "comparing against it")?;
    let disk = offline_disk(conn, name, "checking its drift")?;
    let mut cmd = Command::new("guestkit");
    cmd.arg("drift")
        .arg(&base_disk)
        .arg(&disk)
        .args(["-R", "--report", "--no-color"]);
    let (output, exit_ok) = run_guestkit(cmd)?;
    Ok(GuestRepairReport {
        vm: name.to_string(),
        disk: disk.display().to_string(),
        action: "drift".into(),
        dry_run: true,
        backup: false,
        output,
        // guestkit exits non-zero when drift passes its threshold; the text still carries the findings.
        exit_ok,
    })
}

#[cfg(test)]
mod tests {
    use super::tail;

    #[test]
    fn tail_keeps_the_end_and_trims_whitespace() {
        assert_eq!(tail("  hello  ", 100), "hello");
        assert_eq!(tail("abcdef", 3), "def");
    }

    #[test]
    fn tail_counts_chars_not_bytes() {
        assert_eq!(tail("ééé", 2), "éé");
    }
}
