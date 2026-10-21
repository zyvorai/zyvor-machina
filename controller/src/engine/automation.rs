// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Background loops for the features that used to need a client or cron job to nudge them:
//!
//! * desktop pools: every 30 s, advance clones, recycles and surplus removal;
//! * migration waves: every 60 s, advance waves an operator has started;
//! * backup replication: every 5 minutes, copy new backups to S3 targets whose config sets
//!   `"auto_replicate": true`, and prune off-site copies beyond a target's `"retain"` count.
//!
//! Like the other schedulers they run only on the leader controller, and one failing pass is logged
//! and retried on the next tick.

use std::time::Duration;

use crate::state::AppState;

/// Tick length; every loop below runs on a multiple of it.
const TICK: Duration = Duration::from_secs(30);

/// Whether a loop that runs every `every` ticks fires on tick number `n` (counting from 0).
fn due(n: u64, every: u64) -> bool {
    every != 0 && n % every == 0
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(TICK);
        let mut n: u64 = 0;
        loop {
            interval.tick().await;
            let this = n;
            n = n.wrapping_add(1);
            if !state.leader.is_leader() {
                continue;
            }
            if due(this, 1) {
                crate::api::desktop_pools::reconcile_all(&state).await;
            }
            if due(this, 2) {
                crate::api::migration_waves::advance_started(&state).await;
            }
            if due(this, 10) {
                crate::api::backup_replication::replicate_automatically(&state).await;
                crate::api::backup_replication::prune_old_copies(&state).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loops_fire_on_their_multiples() {
        assert!((0..5).all(|n| due(n, 1)));
        assert_eq!(
            (0..6).filter(|n| due(*n, 2)).collect::<Vec<_>>(),
            vec![0, 2, 4]
        );
        assert_eq!(
            (0..25).filter(|n| due(*n, 10)).collect::<Vec<_>>(),
            vec![0, 10, 20]
        );
        assert!(!due(3, 0));
    }

    #[tokio::test]
    async fn started_waves_query_prepares_and_selects_only_started_ones() {
        let pool = crate::db::testing::pool().await;
        let (on, off) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
        for (id, name, auto) in [(on, "a", true), (off, "b", false)] {
            crate::db::query("INSERT INTO migration_waves (id, name, auto_advance, max_parallel) VALUES (?, ?, ?, 2)")
                .bind(id)
                .bind(name)
                .bind(auto)
                .execute(&pool)
                .await
                .unwrap();
        }
        crate::db::query("INSERT INTO migration_wave_items (id, wave_id, position, name) VALUES (?, ?, 0, 'vm1')")
            .bind(uuid::Uuid::new_v4())
            .bind(on)
            .execute(&pool)
            .await
            .unwrap();
        let rows: Vec<(uuid::Uuid, i64, bool)> =
            crate::db::query_as(crate::api::migration_waves::STARTED_WAVES_SQL)
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(rows, vec![(on, 2, true)]);
    }
}
