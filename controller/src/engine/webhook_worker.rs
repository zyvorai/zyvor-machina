// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::time::Duration;

use crate::db::DbPool;

use crate::leader::LeaderHandle;

pub fn spawn(pool: DbPool, leader: LeaderHandle) {
    tokio::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            // Don't follow redirects: the create-time SSRF filter only vets the
            // original URL, so a 302 to http://169.254.169.254/… would otherwise
            // reach cloud metadata / internal services.
            .redirect(reqwest::redirect::Policy::none())
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("webhook worker: failed to build HTTP client: {e:#}");
                return;
            }
        };
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            if !leader.is_leader() {
                continue;
            }
            if let Err(e) = process_batch(&pool, &client).await {
                tracing::warn!("webhook worker: {e:#}");
            }
        }
    });
}

async fn process_batch(pool: &DbPool, client: &reqwest::Client) -> anyhow::Result<()> {
    // Atomically claim the due rows in one statement (mirroring the
    // `tasks.claimed_by` UPDATE...RETURNING claim pattern in
    // tasks/worker.rs::claim_task) instead of a separate SELECT followed by
    // an UPDATE-by-id later. A plain SELECT-then-update-by-id left a window
    // where, if leadership flipped mid-batch, a second controller could
    // select and deliver the same rows before the first one updated them.
    // The claim value itself doesn't need to be a stable controller id — it
    // only needs to make this claiming UPDATE atomic — so a fresh id per
    // batch is enough.
    let claim_id = uuid::Uuid::new_v4().to_string();
    let rows: Vec<(uuid::Uuid, String, String, serde_json::Value, i32, i32)> = crate::db::query_as(
        "UPDATE webhook_deliveries SET status = 'processing', claimed_by = ?
         WHERE id IN (
             SELECT id FROM webhook_deliveries
             WHERE status = 'pending' AND next_retry_at <= datetime('now')
             ORDER BY next_retry_at LIMIT 20
         )
         RETURNING id, url, secret, body, attempts, max_attempts",
    )
    .bind(&claim_id)
    .fetch_all(pool)
    .await?;

    for (id, url, secret, body, attempts, max_attempts) in rows {
        let body_str = body.to_string();
        let mut req = client.post(&url).header("Content-Type", "application/json");
        if !secret.is_empty() {
            if let Some(sig) = crate::engine::webhooks::sign_payload(&secret, &body_str) {
                req = req.header("X-Machina-Signature", format!("sha256={sig}"));
            }
        }
        match req.body(body_str).send().await {
            Ok(resp) if resp.status().is_success() => {
                crate::db::query(
                    "UPDATE webhook_deliveries SET status = 'delivered', last_error = '', attempts = attempts + 1 WHERE id = ?",
                )
                .bind(id)
                .execute(pool)
                .await?;
            }
            Ok(resp) => {
                let err = format!("HTTP {}", resp.status());
                mark_retry(pool, id, attempts, max_attempts, &err).await?;
            }
            Err(e) => {
                mark_retry(pool, id, attempts, max_attempts, &e.to_string()).await?;
            }
        }
    }
    Ok(())
}

async fn mark_retry(
    pool: &DbPool,
    id: uuid::Uuid,
    attempts: i32,
    max_attempts: i32,
    err: &str,
) -> anyhow::Result<()> {
    let next = attempts + 1;
    if next >= max_attempts {
        crate::db::query(
            "UPDATE webhook_deliveries SET status = 'failed', attempts = ?, last_error = ? WHERE id = ?",
        )
        .bind(next)
        .bind(err)
        .bind(id)
        .execute(pool)
        .await?;
    } else {
        let backoff_secs = 2_i32.saturating_pow(next as u32).min(300);
        // Release the claim back to 'pending' (clearing claimed_by, same as
        // tasks/worker.rs does when a claimed task goes back to pending) so
        // the row is eligible to be claimed again once next_retry_at elapses.
        crate::db::query(
            "UPDATE webhook_deliveries SET status = 'pending', claimed_by = NULL, attempts = ?, last_error = ?,
             next_retry_at = datetime('now', '+' || ? || ' seconds') WHERE id = ?",
        )
        .bind(next)
        .bind(err)
        .bind(backoff_secs)
        .bind(id)
        .execute(pool)
        .await?;
    }
    Ok(())
}
