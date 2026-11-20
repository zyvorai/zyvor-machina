// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Append full [`machina_core::host_inventory::HardwareInventoryReport`] snapshots to JSON Lines on a timer.

use machina_core::config::InventoryHistoryConfig;
use machina_core::host_inventory::{
    append_inventory_history_line, gather_hardware_inventory_report, inventory_history_jsonl_path,
};
use machina_core::libvirt::node;
use machina_core::LibvirtManager;
use std::time::Duration;

pub fn spawn_inventory_history_worker(manager: LibvirtManager, cfg: InventoryHistoryConfig) {
    if !cfg.enabled || cfg.interval_secs == 0 {
        tracing::info!(
            "hardware inventory history disabled (set [inventory_history] enabled = true and interval_secs > 0)"
        );
        return;
    }

    let interval = Duration::from_secs(cfg.interval_secs.max(60));
    let max_bytes = cfg.max_file_mb.saturating_mul(1024 * 1024);

    tokio::spawn(async move {
        tracing::info!(
            "hardware inventory history: snapshot every {:?}, max file {} MiB → {}",
            interval,
            cfg.max_file_mb,
            inventory_history_jsonl_path().display()
        );

        loop {
            let m = manager.clone();
            let mb = max_bytes;
            let res = tokio::task::spawn_blocking(move || {
                let libvirt = m.with_conn(node::get_node_info).ok();
                let report = gather_hardware_inventory_report(libvirt)?;
                append_inventory_history_line(&report, mb)
            })
            .await;

            match res {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::warn!("inventory history snapshot failed: {e}"),
                Err(e) => tracing::warn!("inventory history task join error: {e}"),
            }

            tokio::time::sleep(interval).await;
        }
    });
}
