// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! FluxVM live migration between two `fluxvm-api` endpoints (or one, host to
//! itself): adopt-mode receiver on the target, `migration/start` on the source,
//! then finish the source and adopt the receiver. QEMU on `storage: shared`
//! (or Ceph RBD in place) only; FluxVM refuses everything else up front.

use std::time::{Duration, Instant};

use serde_json::Value;

use super::client::FluxvmClient;
use super::types::FluxRecord;
use crate::LibvirtError;

#[derive(Debug, Clone, Default)]
pub struct MigrateOptions {
    /// Address the target's TCP listener binds (`0.0.0.0` for a remote source).
    pub listen_host: String,
    /// Address the source dials when it differs from `listen_host`.
    pub advertise_host: String,
    pub bandwidth_mbps: Option<u64>,
    pub max_downtime_ms: Option<u64>,
    pub timeout: Duration,
}

/// Where a `migration/status` body says the stream is.
#[derive(Debug, PartialEq, Eq)]
pub enum Progress {
    Running,
    Completed,
    Failed(String),
}

pub fn progress(status: &Value) -> Progress {
    let phase = status
        .get("phase")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .replace(['_', '-'], "");
    match phase.as_str() {
        "completed" => Progress::Completed,
        "failed" | "cancelled" => Progress::Failed(
            status
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("migration {phase}")),
        ),
        _ => Progress::Running,
    }
}

/// Move running VM `name` from `src` to `dst`. Returns the adopted record on
/// the target (new id; the disk and name are unchanged).
pub async fn live_migrate(
    src: &FluxvmClient,
    dst: &FluxvmClient,
    name: &str,
    opts: &MigrateOptions,
) -> Result<FluxRecord, LibvirtError> {
    let rec = src.get(name).await?;
    if rec.status != "running" {
        return Err(LibvirtError::Invalid(format!(
            "VM '{name}' is {}, live migration needs it running",
            rec.status
        )));
    }
    let record = src.export_record(name).await?;
    let listen = if opts.listen_host.is_empty() {
        "127.0.0.1"
    } else {
        &opts.listen_host
    };
    let recv = dst
        .receiver_create(record, listen, &opts.advertise_host)
        .await?;
    let abort = |e| abort(src, dst, &rec.id, &recv.id, e);
    if let Err(e) = dst.receiver_activate(&recv.id, &recv.token).await {
        return abort(e).await;
    }
    if let Err(e) = src
        .migration_start(
            &rec.id,
            &recv.uri,
            opts.bandwidth_mbps,
            opts.max_downtime_ms,
        )
        .await
    {
        return abort(e).await;
    }
    let timeout = if opts.timeout.is_zero() {
        Duration::from_secs(600)
    } else {
        opts.timeout
    };
    let started = Instant::now();
    loop {
        match src.migration_status(&rec.id).await.map(|s| progress(&s)) {
            Ok(Progress::Completed) => break,
            Ok(Progress::Failed(msg)) => {
                return abort(LibvirtError::Operation(format!("FluxVM migration: {msg}"))).await
            }
            Ok(Progress::Running) => {}
            Err(e) => return abort(e).await,
        }
        if started.elapsed() > timeout {
            return abort(LibvirtError::Operation(format!(
                "FluxVM migration of '{name}' did not finish within {}s",
                timeout.as_secs()
            )))
            .await;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    src.migration_finish(&rec.id).await?;
    adopt_with_retry(dst, &recv.id, &recv.token).await
}

async fn abort(
    src: &FluxvmClient,
    dst: &FluxvmClient,
    src_id: &str,
    receiver_id: &str,
    e: LibvirtError,
) -> Result<FluxRecord, LibvirtError> {
    let _ = src.migration_cancel(src_id).await;
    let _ = dst.receiver_delete(receiver_id).await;
    Err(e)
}

/// The target's run state can trail the source's `completed` briefly; adopt
/// answers 409 until it is running.
pub async fn adopt_with_retry(
    dst: &FluxvmClient,
    id: &str,
    token: &str,
) -> Result<FluxRecord, LibvirtError> {
    let mut last = None;
    for _ in 0..40 {
        match dst.receiver_adopt(id, token).await {
            Ok(r) => return Ok(r),
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }
    Err(last.unwrap_or_else(|| LibvirtError::Operation("adopt failed".into())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_phases_classify() {
        assert_eq!(progress(&json!({"phase": "active"})), Progress::Running);
        assert_eq!(
            progress(&json!({"phase": "postcopy_active"})),
            Progress::Running
        );
        assert_eq!(
            progress(&json!({"phase": "completed"})),
            Progress::Completed
        );
        assert_eq!(
            progress(&json!({"phase": "failed", "error": "boom"})),
            Progress::Failed("boom".into())
        );
        assert!(matches!(
            progress(&json!({"phase": "cancelled"})),
            Progress::Failed(_)
        ));
        assert_eq!(progress(&json!({})), Progress::Running);
    }
}
