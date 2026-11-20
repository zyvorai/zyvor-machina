// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Wake-on-traffic: restores a sleeping (managed-saved) VM when the local
//! machina-bpfd publishes `vm_wake` for it. The controller notices the VM is
//! back through inventory and drops it from the wake set.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::libvirt_ops::LibvirtCtx;

const RESTORE_COOLDOWN: Duration = Duration::from_secs(10);

pub fn spawn(libvirt: Arc<Mutex<LibvirtCtx>>) {
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        let mut recent: HashMap<String, Instant> = HashMap::new();
        loop {
            let mut rx = match machina_bpf::BpfdClient::from_env()
                .subscribe(&["vm_wake"])
                .await
            {
                Ok(rx) => {
                    backoff = Duration::from_secs(1);
                    rx
                }
                Err(e) => {
                    tracing::debug!("wake: bpfd subscribe: {e:#}");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                    continue;
                }
            };
            while let Some(ev) = rx.recv().await {
                let Some(vm) = ev.event.get("vm").and_then(|v| v.as_str()) else {
                    continue;
                };
                let vm = vm.to_string();
                let now = Instant::now();
                recent.retain(|_, t| now.duration_since(*t) < RESTORE_COOLDOWN);
                if recent.contains_key(&vm) {
                    continue;
                }
                recent.insert(vm.clone(), now);
                let lv = libvirt.clone();
                let name = vm.clone();
                let started = Instant::now();
                let res = tokio::task::spawn_blocking(move || {
                    let mut ctx = lv.lock().unwrap_or_else(|e| e.into_inner());
                    ctx.wake(&name)
                })
                .await;
                match res {
                    Ok(Ok(true)) => tracing::info!(
                        vm = %vm,
                        address = ev.event.get("address").and_then(|a| a.as_str()).unwrap_or(""),
                        via = ev.event.get("via").and_then(|a| a.as_str()).unwrap_or(""),
                        ms = started.elapsed().as_millis() as u64,
                        "wake: restored sleeping vm"
                    ),
                    Ok(Ok(false)) => {}
                    Ok(Err(e)) => tracing::warn!(vm = %vm, "wake: restore failed: {e}"),
                    Err(e) => tracing::warn!(vm = %vm, "wake: restore task: {e}"),
                }
            }
            tokio::time::sleep(backoff).await;
        }
    });
}
