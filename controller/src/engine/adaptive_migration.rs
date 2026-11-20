// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Adaptive live-migration controller.
//!
//! This module is the deterministic control brain. Given live migration telemetry,
//! it returns the next safe tuning decision. It does not itself mutate libvirt.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AdaptiveMigrationPolicy {
    #[serde(default = "default_min_bw")]
    pub min_bandwidth_mib_s: u64,
    #[serde(default = "default_max_bw")]
    pub max_bandwidth_mib_s: u64,
    #[serde(default = "default_downtime")]
    pub target_downtime_ms: u64,
    #[serde(default = "default_abort")]
    pub abort_after_secs: u64,
    #[serde(default = "default_stall")]
    pub stall_rounds_before_escalation: u32,
    #[serde(default = "default_true")]
    pub allow_compression: bool,
    #[serde(default)]
    pub allow_postcopy: bool,
    #[serde(default = "default_max_throttle")]
    pub max_vcpu_throttle_pct: u32,
}

fn default_min_bw() -> u64 { 128 }
fn default_max_bw() -> u64 { 4096 }
fn default_downtime() -> u64 { 250 }
fn default_abort() -> u64 { 1800 }
fn default_stall() -> u32 { 3 }
fn default_true() -> bool { true }
fn default_max_throttle() -> u32 { 30 }

impl Default for AdaptiveMigrationPolicy {
    fn default() -> Self {
        Self {
            min_bandwidth_mib_s: default_min_bw(),
            max_bandwidth_mib_s: default_max_bw(),
            target_downtime_ms: default_downtime(),
            abort_after_secs: default_abort(),
            stall_rounds_before_escalation: default_stall(),
            allow_compression: true,
            allow_postcopy: false,
            max_vcpu_throttle_pct: default_max_throttle(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MigrationTelemetry {
    pub elapsed_secs: u64,
    pub iteration: u32,
    pub remaining_mib: f64,
    pub transferred_mib: f64,
    pub dirty_rate_mib_s: f64,
    pub effective_bandwidth_mib_s: f64,
    pub guest_cpu_pct: f64,
    pub source_cpu_pct: f64,
    pub destination_cpu_pct: f64,
    pub packet_loss_pct: f64,
    pub current_bandwidth_mib_s: u64,
    pub compression_enabled: bool,
    pub postcopy_active: bool,
    pub vcpu_throttle_pct: u32,
    /// Number of consecutive rounds where remaining bytes failed to shrink by >= 10%.
    pub stalled_rounds: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MigrationDecisionKind {
    Hold,
    IncreaseBandwidth,
    EnableCompression,
    ThrottleVcpu,
    RaiseDowntime,
    SwitchPostcopy,
    Abort,
    ReadyForStopAndCopy,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdaptiveMigrationDecision {
    pub kind: MigrationDecisionKind,
    pub reason: String,
    pub bandwidth_mib_s: u64,
    pub target_downtime_ms: u64,
    pub compression: bool,
    pub postcopy: bool,
    pub vcpu_throttle_pct: u32,
    pub convergence_ratio: f64,
    pub estimated_downtime_ms: f64,
    pub risk_score: f64,
}

fn convergence_ratio(t: &MigrationTelemetry) -> f64 {
    if t.effective_bandwidth_mib_s <= 0.0 { f64::INFINITY }
    else { t.dirty_rate_mib_s / t.effective_bandwidth_mib_s }
}

fn estimated_downtime_ms(t: &MigrationTelemetry) -> f64 {
    if t.effective_bandwidth_mib_s <= 0.0 { f64::INFINITY }
    else { t.remaining_mib / t.effective_bandwidth_mib_s * 1000.0 }
}

fn risk_score(t: &MigrationTelemetry, p: &AdaptiveMigrationPolicy) -> f64 {
    let ratio = convergence_ratio(t);
    let mut risk = (ratio * 55.0).clamp(0.0, 70.0);
    risk += (t.packet_loss_pct * 5.0).clamp(0.0, 15.0);
    risk += (t.stalled_rounds as f64 * 4.0).clamp(0.0, 12.0);
    if t.elapsed_secs > p.abort_after_secs * 3 / 4 { risk += 8.0; }
    if t.destination_cpu_pct > 90.0 { risk += 10.0; }
    risk.clamp(0.0, 100.0)
}

pub fn decide(t: &MigrationTelemetry, p: &AdaptiveMigrationPolicy) -> AdaptiveMigrationDecision {
    let ratio = convergence_ratio(t);
    let downtime = estimated_downtime_ms(t);
    let risk = risk_score(t, p);
    let mut bw = t.current_bandwidth_mib_s.clamp(p.min_bandwidth_mib_s, p.max_bandwidth_mib_s);
    let mut compression = t.compression_enabled;
    let mut postcopy = t.postcopy_active;
    let mut throttle = t.vcpu_throttle_pct.min(p.max_vcpu_throttle_pct);
    let mut target_dt = p.target_downtime_ms;

    let (kind, reason) = if t.elapsed_secs >= p.abort_after_secs && !t.postcopy_active {
        (MigrationDecisionKind::Abort,
         format!("migration exceeded {}s guardrail without completing", p.abort_after_secs))
    } else if t.packet_loss_pct >= 5.0 {
        (MigrationDecisionKind::Abort,
         format!("migration path packet loss {:.1}% exceeds safety threshold", t.packet_loss_pct))
    } else if downtime <= p.target_downtime_ms as f64 && ratio < 0.95 {
        (MigrationDecisionKind::ReadyForStopAndCopy,
         format!("remaining {:.1} MiB fits {:.0} ms estimated downtime", t.remaining_mib, downtime))
    } else if ratio >= 1.0 && bw < p.max_bandwidth_mib_s {
        let need = (t.dirty_rate_mib_s * 1.35).ceil() as u64;
        bw = need.max(bw.saturating_mul(5) / 4).min(p.max_bandwidth_mib_s);
        (MigrationDecisionKind::IncreaseBandwidth,
         format!("dirty rate {:.1} MiB/s is at/above effective bandwidth {:.1} MiB/s", t.dirty_rate_mib_s, t.effective_bandwidth_mib_s))
    } else if t.stalled_rounds >= p.stall_rounds_before_escalation && p.allow_compression && !compression {
        compression = true;
        (MigrationDecisionKind::EnableCompression,
         format!("{} consecutive migration rounds failed to shrink working set", t.stalled_rounds))
    } else if t.stalled_rounds >= p.stall_rounds_before_escalation && throttle < p.max_vcpu_throttle_pct && t.guest_cpu_pct >= 70.0 {
        throttle = (throttle + 10).min(p.max_vcpu_throttle_pct);
        (MigrationDecisionKind::ThrottleVcpu,
         format!("guest CPU {:.0}% and dirty rate {:.1} MiB/s are preventing convergence", t.guest_cpu_pct, t.dirty_rate_mib_s))
    } else if t.stalled_rounds >= p.stall_rounds_before_escalation + 2 && p.allow_postcopy && !postcopy {
        postcopy = true;
        (MigrationDecisionKind::SwitchPostcopy,
         "pre-copy remains stalled after bandwidth/compression/throttle escalation".into())
    } else if ratio < 0.75 && downtime > p.target_downtime_ms as f64 && downtime <= p.target_downtime_ms as f64 * 2.0 {
        target_dt = ((downtime.ceil() as u64 + 24) / 25 * 25).min(p.target_downtime_ms * 2);
        (MigrationDecisionKind::RaiseDowntime,
         format!("migration is converging but needs {:.0} ms stop-and-copy window", downtime))
    } else {
        (MigrationDecisionKind::Hold,
         format!("migration remains within adaptive envelope (ratio {:.2}, risk {:.0}/100)", ratio, risk))
    };

    AdaptiveMigrationDecision {
        kind,
        reason,
        bandwidth_mib_s: bw,
        target_downtime_ms: target_dt,
        compression,
        postcopy,
        vcpu_throttle_pct: throttle,
        convergence_ratio: ratio,
        estimated_downtime_ms: downtime,
        risk_score: risk,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> MigrationTelemetry {
        MigrationTelemetry {
            elapsed_secs: 30, iteration: 2, remaining_mib: 1024.0, transferred_mib: 8192.0,
            dirty_rate_mib_s: 100.0, effective_bandwidth_mib_s: 800.0,
            guest_cpu_pct: 50.0, source_cpu_pct: 60.0, destination_cpu_pct: 20.0,
            packet_loss_pct: 0.0, current_bandwidth_mib_s: 800,
            compression_enabled: false, postcopy_active: false, vcpu_throttle_pct: 0,
            stalled_rounds: 0,
        }
    }

    #[test]
    fn ready_when_remaining_fits_downtime() {
        let mut x=t(); x.remaining_mib=100.0;
        assert_eq!(decide(&x,&AdaptiveMigrationPolicy::default()).kind, MigrationDecisionKind::ReadyForStopAndCopy);
    }
    #[test]
    fn increase_bw_when_dirty_exceeds_bw() {
        let mut x=t(); x.dirty_rate_mib_s=900.0;
        assert_eq!(decide(&x,&AdaptiveMigrationPolicy::default()).kind, MigrationDecisionKind::IncreaseBandwidth);
    }
    #[test]
    fn compression_before_throttle() {
        let mut x=t(); x.stalled_rounds=3; x.guest_cpu_pct=90.0;
        assert_eq!(decide(&x,&AdaptiveMigrationPolicy::default()).kind, MigrationDecisionKind::EnableCompression);
    }
    #[test]
    fn throttle_after_compression() {
        let mut x=t(); x.stalled_rounds=3; x.guest_cpu_pct=90.0; x.compression_enabled=true;
        assert_eq!(decide(&x,&AdaptiveMigrationPolicy::default()).kind, MigrationDecisionKind::ThrottleVcpu);
    }
    #[test]
    fn abort_on_path_loss() {
        let mut x=t(); x.packet_loss_pct=6.0;
        assert_eq!(decide(&x,&AdaptiveMigrationPolicy::default()).kind, MigrationDecisionKind::Abort);
    }
}
