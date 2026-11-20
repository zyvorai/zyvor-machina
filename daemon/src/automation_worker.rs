// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Background worker for alerts, schedules, and snapshot schedules.

use machina_core::libvirt::automation_runner::run_automation_tick;
use machina_core::LibvirtManager;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

/// Unix timestamp of the last successful automation tick (for `/integrations/status`).
pub static AUTOMATION_LAST_TICK_UNIX: AtomicI64 = AtomicI64::new(0);

pub fn automation_last_tick_unix() -> Option<i64> {
    let ts = AUTOMATION_LAST_TICK_UNIX.load(Ordering::Relaxed);
    if ts > 0 {
        Some(ts)
    } else {
        None
    }
}

pub fn spawn_automation_worker(manager: LibvirtManager) {
    tokio::spawn(async move {
        tracing::info!("automation worker: evaluating alerts and schedules every 60s");
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let mgr = manager.clone();
            let res = tokio::task::spawn_blocking(move || run_automation_tick(&mgr)).await;
            match res {
                Ok(()) => {
                    AUTOMATION_LAST_TICK_UNIX
                        .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
                }
                Err(e) => tracing::warn!("automation worker join error: {e}"),
            }
        }
    });
}
