// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//
// Day-2: notification-channel delivery worker. Delivers channel_deliveries rows produced by
// webhooks::dispatch_channels to Slack (incoming webhook), email (SMTP), or a generic
// webhook. Retries with exponential backoff, mirroring webhook_worker.

use std::time::Duration;

use crate::db::DbPool;

use crate::leader::LeaderHandle;

pub fn spawn(pool: DbPool, leader: LeaderHandle) {
    tokio::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("channel worker: failed to build HTTP client: {e:#}");
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
                tracing::warn!("channel worker: {e:#}");
            }
        }
    });
}

async fn process_batch(pool: &DbPool, client: &reqwest::Client) -> anyhow::Result<()> {
    // Atomically claim the due rows in one statement (mirroring the
    // webhook_deliveries.claimed_by pattern in webhook_worker.rs::process_batch)
    // instead of a separate SELECT followed by an UPDATE-by-id later. A plain
    // SELECT-then-update-by-id left a window where, if leadership flipped
    // mid-batch, a second controller could select and deliver the same rows
    // before the first one updated them.
    let claim_id = uuid::Uuid::new_v4().to_string();
    let rows: Vec<(uuid::Uuid, String, String, String, String, i32, i32)> = crate::db::query_as(
        "UPDATE channel_deliveries SET status = 'processing', claimed_by = ?
         WHERE id IN (
             SELECT id FROM channel_deliveries
             WHERE status = 'pending' AND next_retry_at <= datetime('now')
             ORDER BY next_retry_at LIMIT 20
         )
         RETURNING id, kind, target, subject, body, attempts, max_attempts",
    )
    .bind(&claim_id)
    .fetch_all(pool)
    .await?;

    for (id, kind, target, subject, body, attempts, max_attempts) in rows {
        let result = match kind.as_str() {
            "slack" => deliver_slack(client, &target, &subject, &body).await,
            "email" => deliver_email(&target, &subject, &body).await,
            _ => deliver_webhook(client, &target, &subject, &body).await, // generic webhook
        };
        match result {
            Ok(()) => {
                crate::db::query(
                    "UPDATE channel_deliveries SET status = 'delivered', last_error = '', attempts = attempts + 1 WHERE id = ?",
                )
                .bind(id)
                .execute(pool)
                .await?;
            }
            Err(e) => mark_retry(pool, id, attempts, max_attempts, &e.to_string()).await?,
        }
    }
    Ok(())
}

async fn deliver_slack(
    client: &reqwest::Client,
    url: &str,
    subject: &str,
    body: &str,
) -> anyhow::Result<()> {
    let payload = serde_json::json!({ "text": format!("*{subject}*\n{body}") });
    let resp = client.post(url).json(&payload).send().await?;
    if resp.status().is_success() {
        Ok(())
    } else {
        anyhow::bail!("slack HTTP {}", resp.status())
    }
}

async fn deliver_webhook(
    client: &reqwest::Client,
    url: &str,
    subject: &str,
    body: &str,
) -> anyhow::Result<()> {
    let payload = serde_json::json!({ "subject": subject, "text": body });
    let resp = client.post(url).json(&payload).send().await?;
    if resp.status().is_success() {
        Ok(())
    } else {
        anyhow::bail!("webhook HTTP {}", resp.status())
    }
}

/// Send via SMTP using MACHINA_SMTP_* env (host required). STARTTLS on the configured port
/// (default 587); optional username/password auth; MACHINA_SMTP_FROM is the sender.
async fn deliver_email(to: &str, subject: &str, body: &str) -> anyhow::Result<()> {
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

    let host = std::env::var("MACHINA_SMTP_HOST")
        .map_err(|_| anyhow::anyhow!("email channel: MACHINA_SMTP_HOST not configured"))?;
    let port: u16 = std::env::var("MACHINA_SMTP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(587);
    let from = std::env::var("MACHINA_SMTP_FROM").unwrap_or_else(|_| "machina@localhost".into());

    let email = Message::builder()
        .from(from.parse()?)
        .to(to.parse()?)
        .subject(subject)
        .body(body.to_string())?;

    let mut builder = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)?.port(port);
    if let (Ok(user), Ok(pass)) = (
        std::env::var("MACHINA_SMTP_USER"),
        std::env::var("MACHINA_SMTP_PASS"),
    ) {
        if !user.is_empty() {
            builder = builder.credentials(Credentials::new(user, pass));
        }
    }
    let mailer = builder.build();
    mailer.send(email).await?;
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
            "UPDATE channel_deliveries SET status = 'failed', attempts = ?, last_error = ? WHERE id = ?",
        )
        .bind(next)
        .bind(err)
        .bind(id)
        .execute(pool)
        .await?;
    } else {
        let backoff_secs = 2_i32.saturating_pow(next as u32).min(300);
        // Release the claim back to 'pending' (clearing claimed_by, same as
        // webhook_worker.rs does) so the row is eligible to be claimed again
        // once next_retry_at elapses.
        crate::db::query(
            "UPDATE channel_deliveries SET status = 'pending', claimed_by = NULL, attempts = ?, last_error = ?,
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
