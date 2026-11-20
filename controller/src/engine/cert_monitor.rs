// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//
// Day-2 (Tier-1, secret/cert rotation — cert-expiry slice): watch the platform TLS cert and
// raise an alert as it nears expiry. An expired cert is a fleet-wide outage, so this is the
// cheap early-warning half of cert rotation. Uses the openssl CLI (already a platform dep),
// so no new crate. Emits through the central event dispatcher, so warnings reach in-app
// notifications, webhooks, and the Slack/email channels.

use std::time::Duration;

use crate::state::AppState;

pub fn default_cert_path() -> String {
    std::env::var("MACHINA_TLS_CERT_PATH").unwrap_or_else(|_| "/etc/machina/ssl/cert.pem".into())
}

fn warn_days() -> i64 {
    std::env::var("MACHINA_CERT_WARN_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30)
}

/// (not_after string, days_remaining) for a PEM cert, via `openssl x509 -enddate`.
/// None if the file is missing/unreadable or openssl/date parsing fails.
pub async fn cert_status(path: &str) -> Option<(String, i64)> {
    let out = tokio::process::Command::new("openssl")
        .args(["x509", "-enddate", "-noout", "-in", path])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let not_after = stdout.trim().strip_prefix("notAfter=")?.trim().to_string();
    // openssl prints e.g. "Jun  4 17:59:52 2027 GMT" (GMT == UTC).
    let cleaned = not_after.trim_end_matches(" GMT");
    let dt = chrono::NaiveDateTime::parse_from_str(cleaned, "%b %e %H:%M:%S %Y").ok()?;
    let days = (dt - chrono::Utc::now().naive_utc()).num_days();
    Some((not_after, days))
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        // A tokio interval's first tick fires immediately, which races leader election on a
        // fresh start — the startup cert check would be skipped and not retried for 6h. Delay
        // the first check so leadership is settled, then poll every 6h (cert expiry moves on a
        // scale of days, so frequent polling is wasteful).
        tokio::time::sleep(Duration::from_secs(45)).await;
        let mut interval = tokio::time::interval(Duration::from_secs(6 * 3600));
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = tick(&state).await {
                tracing::warn!("cert monitor: {e:#}");
            }
        }
    });
}

async fn tick(state: &AppState) -> anyhow::Result<()> {
    let path = default_cert_path();
    let Some((not_after, days)) = cert_status(&path).await else {
        // No cert to monitor (TLS terminated upstream, or file absent) — nothing to warn about.
        return Ok(());
    };
    let threshold = warn_days();
    if days <= threshold {
        let severity = if days <= 7 { "critical" } else { "warning" };
        let kind = format!("cert.expiring.{severity}");
        let msg = if days < 0 {
            format!(
                "TLS certificate {path} EXPIRED {} days ago (notAfter {not_after})",
                -days
            )
        } else {
            format!(
                "TLS certificate {path} expires in {days} days (notAfter {not_after}) — rotate it"
            )
        };
        crate::engine::webhooks::dispatch_webhooks(
            &state.pool,
            &kind,
            serde_json::json!({ "message": msg, "path": path, "not_after": not_after, "days_remaining": days }),
        )
        .await;
        state.emit_event(&kind, msg);
    }
    Ok(())
}
