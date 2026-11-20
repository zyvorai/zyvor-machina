// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Closes the loop for the adaptive migration engine: read the live migration status from the source host's agent, ask
//! `adaptive_migration::decide` for the next step, and apply the part of it that a running libvirt migration can take (bandwidth,
//! maximum downtime, post-copy, a bounded vCPU quota, abort). One call is one step; nothing here loops or retries.
//!
//! The agent is looked up from the VM's own host in the database, never taken from the request: a caller-supplied address would
//! let the controller (and the agent bearer token it sends) be pointed at any host.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent_client;
use crate::db::DbPool;
use crate::engine::adaptive_migration::{
    decide, AdaptiveMigrationDecision, AdaptiveMigrationPolicy, MigrationDecisionKind, MigrationTelemetry,
};

#[derive(Debug, Clone, Deserialize)]
pub struct AdaptiveActuatorRequest {
    pub vm_id: Uuid,
    /// Post-copy can lose the guest if the network fails mid-transition, so it is only ever used when the caller opts in.
    #[serde(default)]
    pub allow_postcopy: bool,
    /// Consecutive rounds the caller has seen without the remaining data shrinking by 10% (libvirt does not report it).
    #[serde(default)]
    pub stalled_rounds: u32,
    #[serde(default)]
    pub compression_enabled: bool,
    /// The vCPU throttle this caller has already applied, in percent of full speed taken away (0 = none).
    #[serde(default)]
    pub throttle_percent: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdaptiveActuatorResponse {
    pub telemetry: MigrationTelemetry,
    pub decision: AdaptiveMigrationDecision,
    pub applied: bool,
    pub message: String,
}

const PAGE_BYTES: f64 = 4096.0;
const MIB: f64 = 1_048_576.0;

fn pages_to_mib(pages: u64) -> f64 {
    pages as f64 * PAGE_BYTES / MIB
}

pub async fn step(pool: &DbPool, req: &AdaptiveActuatorRequest) -> anyhow::Result<AdaptiveActuatorResponse> {
    let (vm_name, agent_addr): (String, String) = crate::db::query_as(
        "SELECT v.name, h.agent_grpc_addr FROM vms v JOIN hosts h ON h.id = v.host_id WHERE v.id = ?",
    )
    .bind(req.vm_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("VM not found, or it has no host"))?;
    let guest_cpu: Option<f64> = crate::db::query_scalar("SELECT cpu_percent FROM vm_metrics WHERE vm_id = ?")
        .bind(req.vm_id)
        .fetch_optional(pool)
        .await
        .unwrap_or(None);

    let mut client = agent_client::connect(&agent_addr).await?;
    let s = agent_client::get_migration_status(&mut client, &vm_name).await?;
    let bandwidth_mib_s = s.memory_bps as f64 / MIB;
    let telemetry = MigrationTelemetry {
        elapsed_secs: s.time_elapsed_ms / 1000,
        iteration: 0, // libvirt does not report the pre-copy round
        remaining_mib: s.memory_remaining_bytes as f64 / MIB,
        transferred_mib: s.memory_processed_bytes as f64 / MIB,
        dirty_rate_mib_s: pages_to_mib(s.memory_dirty_rate_pages_s),
        effective_bandwidth_mib_s: bandwidth_mib_s,
        guest_cpu_pct: guest_cpu.unwrap_or(0.0),
        source_cpu_pct: 0.0,
        destination_cpu_pct: 0.0,
        packet_loss_pct: 0.0,
        current_bandwidth_mib_s: bandwidth_mib_s.round() as u64,
        compression_enabled: req.compression_enabled,
        postcopy_active: s.job_type.to_ascii_lowercase().contains("post"),
        vcpu_throttle_pct: req.throttle_percent,
        stalled_rounds: req.stalled_rounds,
    };
    let policy = AdaptiveMigrationPolicy { allow_postcopy: req.allow_postcopy, ..Default::default() };
    let decision = decide(&telemetry, &policy);

    if !s.active {
        return Ok(AdaptiveActuatorResponse { telemetry, decision, applied: false, message: "no active migration".into() });
    }

    let (applied, message) = match decision.kind {
        MigrationDecisionKind::IncreaseBandwidth => {
            agent_client::control_migration(&mut client, &vm_name, "set_speed", decision.bandwidth_mib_s, false).await?;
            (true, format!("bandwidth limit set to {} MiB/s", decision.bandwidth_mib_s))
        }
        MigrationDecisionKind::RaiseDowntime => {
            agent_client::control_migration(&mut client, &vm_name, "set_downtime", decision.target_downtime_ms, false).await?;
            (true, format!("maximum downtime set to {} ms", decision.target_downtime_ms))
        }
        MigrationDecisionKind::SwitchPostcopy if req.allow_postcopy => {
            agent_client::control_migration(&mut client, &vm_name, "postcopy", 0, true).await?;
            (true, "post-copy started".into())
        }
        MigrationDecisionKind::ThrottleVcpu => {
            let quota = 100u64.saturating_sub(u64::from(decision.vcpu_throttle_pct));
            agent_client::control_migration(&mut client, &vm_name, "throttle_vcpu", quota, false).await?;
            (true, format!("vCPU quota set to {quota}%"))
        }
        MigrationDecisionKind::Abort => {
            agent_client::control_migration(&mut client, &vm_name, "abort", 0, false).await?;
            // best effort: a throttle this migration applied must not outlive it
            let _ = agent_client::control_migration(&mut client, &vm_name, "restore_vcpu", 100, false).await;
            (true, "migration aborted; vCPU quota restore requested".into())
        }
        // libvirt has no switch to turn compression on in a running migration, so it is reported but never faked
        MigrationDecisionKind::EnableCompression => (false, "compression is recommended but cannot be changed on a running migration".into()),
        MigrationDecisionKind::SwitchPostcopy => (false, "post-copy is recommended but the caller did not allow it".into()),
        MigrationDecisionKind::ReadyForStopAndCopy | MigrationDecisionKind::Hold => (false, "no control change required".into()),
    };
    Ok(AdaptiveActuatorResponse { telemetry, decision, applied, message })
}
