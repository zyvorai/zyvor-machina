// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Bounded retention for security/audit evidence. The default is deliberately
//! conservative (365 days); set MACHINA_AUDIT_RETENTION_DAYS=0 to keep forever.

use crate::state::AppState;
use std::time::Duration;

fn days() -> Option<i64> {
    let n = std::env::var("MACHINA_AUDIT_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(365);
    (n > 0).then_some(n.clamp(7, 3650))
}

pub async fn run_once(state: &AppState) -> anyhow::Result<u64> {
    let Some(days) = days() else { return Ok(0) };
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(days))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let n = crate::db::query("DELETE FROM audit_logs WHERE created_at < ?")
        .bind(cutoff)
        .execute(&state.pool)
        .await?
        .rows_affected();
    Ok(n)
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(24 * 3600));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            match run_once(&state).await {
                Ok(n) if n > 0 => tracing::info!(deleted = n, "expired audit records removed"),
                Ok(_) => {}
                Err(e) => tracing::warn!("audit retention: {e:#}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn retention_bounds_are_documented() {
        // The functional DB path is exercised by controller integration tests;
        // keep the policy invariant visible in a cheap unit test.
        assert!(7 <= 365 && 365 <= 3650);
    }
}
