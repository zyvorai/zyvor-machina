// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Optional integration with systemd `Type=notify` and `WatchdogSec=`.
//!
//! When `NOTIFY_SOCKET` is set, we send `READY=1` after the HTTP listener is bound
//! and periodically send `WATCHDOG=1` if `WATCHDOG_USEC` is present.

use std::time::Duration;

use sd_notify::NotifyState;
use tokio::time::MissedTickBehavior;
use tracing::{debug, warn};

/// Notify systemd that the service is ready to accept traffic (listener bound).
pub fn notify_ready() {
    if std::env::var_os("NOTIFY_SOCKET").is_none() {
        return;
    }
    match sd_notify::notify(&[NotifyState::Ready]) {
        Ok(()) => debug!("sent systemd READY=1"),
        Err(e) => warn!("systemd READY=1 notify failed: {e}"),
    }
}

/// If `WATCHDOG_USEC` is set, spawn a task that pings systemd before the watchdog expires.
pub fn spawn_watchdog_pinger() {
    let Some(usec_str) = std::env::var_os("WATCHDOG_USEC") else {
        return;
    };
    let Some(usec_str) = usec_str.to_str() else {
        return;
    };
    let Ok(watchdog_usec) = usec_str.parse::<u64>() else {
        warn!("invalid WATCHDOG_USEC, ignoring systemd watchdog");
        return;
    };
    if watchdog_usec == 0 {
        return;
    }
    if std::env::var_os("NOTIFY_SOCKET").is_none() {
        warn!("WATCHDOG_USEC set but NOTIFY_SOCKET missing; watchdog pings skipped");
        return;
    }

    // Ping at half the configured interval so we stay well under the limit.
    let period_us = (watchdog_usec / 2).max(1_000_000);
    let period = Duration::from_micros(period_us);

    tokio::spawn(async move {
        let mut tick = tokio::time::interval(period);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            if let Err(e) = sd_notify::notify(&[NotifyState::Watchdog]) {
                warn!("systemd WATCHDOG=1 notify failed: {e}");
                break;
            }
        }
    });
}
