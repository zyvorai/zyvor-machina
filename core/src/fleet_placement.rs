// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Fleet placement scoring helpers (unit-tested, no I/O).

/// Headroom score from host CPU, memory, and root disk utilization (0–100, higher is better).
pub fn fleet_capacity_score(cpu: f64, mem: f64, disk: f64) -> (f64, &'static str) {
    let score = ((100.0 - cpu).max(0.0) + (100.0 - mem).max(0.0) + (100.0 - disk).max(0.0)) / 3.0;
    let label = if score >= 40.0 {
        "high"
    } else if score >= 20.0 {
        "medium"
    } else {
        "low"
    };
    ((score * 10.0).round() / 10.0, label)
}

/// Lower score when the requested VM is larger (penalty on headroom).
pub fn placement_adjusted_score(base: f64, vcpus: u32, memory_mb: u64) -> f64 {
    let mem_gb = memory_mb as f64 / 1024.0;
    let penalty = (vcpus as f64 * 3.0) + (mem_gb * 2.0);
    ((base - penalty) * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_high_when_host_has_headroom() {
        let (score, label) = fleet_capacity_score(20.0, 25.0, 30.0);
        assert!(score > 40.0);
        assert_eq!(label, "high");
    }

    #[test]
    fn capacity_low_when_host_is_saturated() {
        let (score, label) = fleet_capacity_score(95.0, 92.0, 90.0);
        assert!(score < 20.0);
        assert_eq!(label, "low");
    }

    #[test]
    fn placement_penalizes_large_vm() {
        let (base, _) = fleet_capacity_score(30.0, 30.0, 30.0);
        let small = placement_adjusted_score(base, 1, 1024);
        let large = placement_adjusted_score(base, 16, 65536);
        assert!(large < small);
    }
}
