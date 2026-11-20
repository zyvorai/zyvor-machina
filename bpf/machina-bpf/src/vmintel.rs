// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! VM runtime intelligence: pure helpers (feature names, histograms, vCPU
//! thread naming).

use machina_bpf_common::{VMI_F_FLIGHT, VMI_F_IO, VMI_F_MEM, VMI_F_TOPO};

use crate::api::{VmIntelBucket, VmIntelHist};

pub const FEATURES: &[(&str, u32)] = &[
    ("flight", VMI_F_FLIGHT),
    ("io", VMI_F_IO),
    ("mem", VMI_F_MEM),
    ("topology", VMI_F_TOPO),
];

pub fn features_mask(names: &[String]) -> Result<u32, String> {
    if names.is_empty() {
        return Ok(FEATURES.iter().fold(0, |m, (_, b)| m | b));
    }
    names.iter().try_fold(0, |m, n| {
        FEATURES
            .iter()
            .find(|(f, _)| f.eq_ignore_ascii_case(n))
            .map(|(_, b)| m | b)
            .ok_or_else(|| format!("unknown feature `{n}` (flight, io, mem, topology)"))
    })
}

/// QEMU names vCPU threads `CPU <n>/KVM` (or `CPU <n>/HVF` etc.).
pub fn vcpu_index(comm: &str) -> Option<u32> {
    let rest = comm.trim().strip_prefix("CPU ")?;
    let (n, _) = rest.split_once('/')?;
    n.parse().ok()
}

/// Build a histogram from (log2 slot, count) pairs.
pub fn hist(slots: &[(u16, u64)]) -> VmIntelHist {
    let mut v: Vec<(u16, u64)> = slots.iter().copied().filter(|(_, c)| *c > 0).collect();
    v.sort_unstable();
    let count: u64 = v.iter().map(|(_, c)| c).sum();
    let le = |s: u16| 1u64.checked_shl(s as u32 + 1).unwrap_or(u64::MAX);
    let pct = |p: f64| {
        let target = (count as f64 * p).ceil() as u64;
        let mut acc = 0;
        for (s, c) in &v {
            acc += c;
            if acc >= target {
                return le(*s);
            }
        }
        0
    };
    VmIntelHist {
        count,
        p50_ns: if count > 0 { pct(0.5) } else { 0 },
        p99_ns: if count > 0 { pct(0.99) } else { 0 },
        buckets: v
            .iter()
            .map(|(s, c)| VmIntelBucket {
                le_ns: le(*s),
                count: *c,
            })
            .collect(),
    }
}

/// ms from process start (`/proc/<pid>/stat` starttime, clock ticks since
/// boot) to a monotonic-ns timestamp.
pub fn boot_ms(start_ticks: u64, clk_tck: u64, first_entry_ns: u64) -> Option<f64> {
    if clk_tck == 0 {
        return None;
    }
    let start_ns = start_ticks as f64 * 1e9 / clk_tck as f64;
    let d = (first_entry_ns as f64 - start_ns) / 1e6;
    (d >= 0.0).then_some(d)
}

/// Field 22 (starttime) of `/proc/<pid>/stat`.
pub fn stat_starttime(stat: &str) -> Option<u64> {
    let after = &stat[stat.rfind(')')? + 1..];
    after.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks() {
        assert_eq!(features_mask(&[]).unwrap(), 15);
        assert_eq!(
            features_mask(&["flight".into(), "MEM".into()]).unwrap(),
            VMI_F_FLIGHT | VMI_F_MEM
        );
        assert!(features_mask(&["bogus".into()]).is_err());
    }

    #[test]
    fn vcpu_names() {
        assert_eq!(vcpu_index("CPU 3/KVM"), Some(3));
        assert_eq!(vcpu_index("CPU 12/KVM\n"), Some(12));
        assert_eq!(vcpu_index("qemu-system-x86"), None);
        assert_eq!(vcpu_index("IO mon_iothread"), None);
    }

    #[test]
    fn histogram_percentiles() {
        let h = hist(&[(10, 98), (20, 2)]);
        assert_eq!(h.count, 100);
        assert_eq!(h.p50_ns, 2048);
        assert_eq!(h.p99_ns, 1 << 21);
        assert_eq!(h.buckets.len(), 2);
        assert_eq!(hist(&[]).p99_ns, 0);
    }

    #[test]
    fn starttime_and_boot() {
        let stat = "1234 (qemu (x) y) S 1 1234 1234 0 -1 4194560 100 0 0 0 5 3 0 0 20 0 4 0 98765 1000 100";
        assert_eq!(stat_starttime(stat), Some(98765));
        assert_eq!(boot_ms(100, 100, 1_500_000_000), Some(500.0));
        assert_eq!(boot_ms(100, 100, 500_000_000), None);
    }
}
