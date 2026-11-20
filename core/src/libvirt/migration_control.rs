// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Status and control of an in-flight live migration, through `virsh` on the source host.
//!
//! Every operation names the VM as an argument to `virsh`, so the name is checked first: a name that begins with `-` would be read
//! as an option. Values are clamped to safe ranges here as well as by the callers.

use std::process::Command;

use crate::LibvirtError;

#[derive(Debug, Clone, Default)]
pub struct MigrationRuntimeStatus {
    pub active: bool,
    pub job_type: String,
    pub time_elapsed_ms: u64,
    pub data_total_bytes: u64,
    pub data_processed_bytes: u64,
    pub data_remaining_bytes: u64,
    pub memory_total_bytes: u64,
    pub memory_processed_bytes: u64,
    pub memory_remaining_bytes: u64,
    pub memory_bps: u64,
    pub memory_dirty_rate_pages_s: u64,
    pub downtime_ms: u64,
    pub setup_time_ms: u64,
}

/// The vCPU quota is only ever lowered to this share (or restored to 100%): a migration helper must not starve a guest.
pub const MIN_VCPU_QUOTA_PCT: u32 = 70;

fn check_vm(vm: &str) -> Result<(), LibvirtError> {
    crate::validate::validate_name(vm)?;
    if vm.starts_with('-') {
        return Err(LibvirtError::Invalid("VM name must not start with '-'".into()));
    }
    Ok(())
}

fn run(uri: &str, args: &[String]) -> Result<String, LibvirtError> {
    let out = Command::new("virsh")
        .arg("-c")
        .arg(uri)
        .args(args)
        .output()
        .map_err(|e| LibvirtError::Operation(format!("virsh {}: {e}", args.join(" "))))?;
    if !out.status.success() {
        return Err(LibvirtError::Operation(format!(
            "virsh {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// The first whitespace-separated token as a number (`4294967296`, or `11 ms` -> 11); 0 when it is not one.
fn number(s: &str) -> u64 {
    s.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// Parse `virsh domjobinfo --rawstats`. Keys are matched case-insensitively with `_` and spaces treated alike, because the
/// human-readable form says `Memory remaining` and the raw one `memory_remaining`.
pub fn parse_domjobinfo(text: &str) -> MigrationRuntimeStatus {
    let mut st = MigrationRuntimeStatus::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let key = key.trim().to_ascii_lowercase().replace('_', " ");
        let value = value.trim();
        match key.as_str() {
            "job type" => {
                st.job_type = value.into();
                st.active = !matches!(value.to_ascii_lowercase().as_str(), "none" | "completed" | "failed" | "cancelled");
            }
            "time elapsed" => st.time_elapsed_ms = number(value),
            "data total" => st.data_total_bytes = number(value),
            "data processed" => st.data_processed_bytes = number(value),
            "data remaining" => st.data_remaining_bytes = number(value),
            "memory total" => st.memory_total_bytes = number(value),
            "memory processed" => st.memory_processed_bytes = number(value),
            "memory remaining" => st.memory_remaining_bytes = number(value),
            "memory bps" => st.memory_bps = number(value),
            "memory dirty rate" => st.memory_dirty_rate_pages_s = number(value),
            "downtime" => st.downtime_ms = number(value),
            "setup time" => st.setup_time_ms = number(value),
            _ => {}
        }
    }
    st
}

pub fn migration_status(uri: &str, vm: &str) -> Result<MigrationRuntimeStatus, LibvirtError> {
    check_vm(vm)?;
    Ok(parse_domjobinfo(&run(uri, &["domjobinfo".into(), vm.into(), "--rawstats".into()])?))
}

pub fn set_speed(uri: &str, vm: &str, mib_s: u64, postcopy: bool) -> Result<(), LibvirtError> {
    check_vm(vm)?;
    let mut args = vec!["migrate-setspeed".into(), vm.into(), mib_s.clamp(1, 16384).to_string()];
    if postcopy {
        args.push("--postcopy".into());
    }
    run(uri, &args).map(|_| ())
}

pub fn set_downtime(uri: &str, vm: &str, ms: u64) -> Result<(), LibvirtError> {
    check_vm(vm)?;
    run(uri, &["migrate-setmaxdowntime".into(), vm.into(), ms.clamp(1, 60000).to_string()]).map(|_| ())
}

pub fn postcopy(uri: &str, vm: &str) -> Result<(), LibvirtError> {
    check_vm(vm)?;
    run(uri, &["migrate-start-postcopy".into(), vm.into()]).map(|_| ())
}

pub fn abort(uri: &str, vm: &str) -> Result<(), LibvirtError> {
    check_vm(vm)?;
    run(uri, &["domjobabort".into(), vm.into()]).map(|_| ())
}

/// Limit the VM's vCPUs to `pct` percent of a CPU each (70..=100). 100 restores full speed.
pub fn set_vcpu_quota(uri: &str, vm: &str, pct: u32) -> Result<(), LibvirtError> {
    check_vm(vm)?;
    let pct = pct.clamp(MIN_VCPU_QUOTA_PCT, 100);
    let period = 100_000u64;
    let quota = period * u64::from(pct) / 100;
    run(
        uri,
        &[
            "schedinfo".into(),
            vm.into(),
            "--set".into(),
            format!("vcpu_period={period}"),
            "--set".into(),
            format!("vcpu_quota={quota}"),
            "--live".into(),
        ],
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_readable_form() {
        let s = "Job type: Unbounded\nMemory remaining: 4294967296\nMemory bps: 536870912\nMemory dirty rate: 32000\nDowntime: 11\n";
        let x = parse_domjobinfo(s);
        assert!(x.active);
        assert_eq!(x.memory_remaining_bytes, 4294967296);
        assert_eq!(x.memory_bps, 536870912);
        assert_eq!(x.memory_dirty_rate_pages_s, 32000);
        assert_eq!(x.downtime_ms, 11);
    }

    #[test]
    fn parses_the_raw_form_with_underscores() {
        let s = "Job type: Unbounded\ntime_elapsed: 2500\nmemory_total: 8589934592\nmemory_remaining: 1073741824\nmemory_dirty_rate: 900\nmemory_bps: 1000\n";
        let x = parse_domjobinfo(s);
        assert_eq!((x.time_elapsed_ms, x.memory_total_bytes, x.memory_remaining_bytes), (2500, 8589934592, 1073741824));
        assert_eq!((x.memory_dirty_rate_pages_s, x.memory_bps), (900, 1000));
    }

    #[test]
    fn an_idle_domain_is_not_active() {
        assert!(!parse_domjobinfo("Job type: None\n").active);
        assert!(!parse_domjobinfo("").active);
        assert!(!parse_domjobinfo("Job type: Completed\n").active);
    }

    #[test]
    fn a_vm_name_that_would_be_an_option_is_refused_before_virsh_runs() {
        for bad in ["--help", "-c", "", "a b", "x;rm", "a/b"] {
            assert!(check_vm(bad).is_err(), "{bad:?}");
        }
        assert!(check_vm("web-1").is_ok());
        // refused by name, so none of these needs virsh to be installed
        assert!(abort("qemu:///system", "--help").is_err());
        assert!(set_vcpu_quota("qemu:///system", "-x", 80).is_err());
    }
}
