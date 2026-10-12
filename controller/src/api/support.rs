// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::response::IntoResponse;
use axum::Extension;
use axum::Json;
use serde::Serialize;

use crate::api::ApiError;
use crate::auth::{require_admin, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct SupportBundleMeta {
    pub controller_id: String,
    pub generated_at: String,
    pub task_count: i64,
    pub host_count: i64,
    pub vm_count: i64,
    pub recent_failures: serde_json::Value,
    pub audit_tail: serde_json::Value,
    pub version_matrix: serde_json::Value,
}

/// A JSON array with one object per row of `from` (object keys are the column names), as text: SQLite builds it with
/// `json_group_array(json_object(..))`, PostgreSQL with `json_agg(json_build_object(..))`.
fn json_rows(columns: &[&str], from: &str) -> String {
    let pairs = columns.iter().map(|c| format!("'{c}', {c}")).collect::<Vec<_>>().join(", ");
    if cfg!(feature = "postgres") {
        format!("SELECT COALESCE(json_agg(json_build_object({pairs}))::text, '[]') FROM ({from}) AS rows")
    } else {
        format!("SELECT COALESCE(json_group_array(json_object({pairs})), '[]') FROM ({from})")
    }
}

pub async fn support_bundle(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<impl IntoResponse, ApiError> {
    require_admin(&actor)?;
    Ok(Json(collect(&state).await?))
}

async fn collect(state: &AppState) -> Result<SupportBundleMeta, ApiError> {
    let task_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(&state.pool)
        .await?;
    let host_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts")
        .fetch_one(&state.pool)
        .await?;
    let vm_count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vms")
        .fetch_one(&state.pool)
        .await?;

    let recent_failures: serde_json::Value = crate::db::query_scalar(&json_rows(
        &["id", "operation", "status", "message", "created_at"],
        "SELECT id, operation, status, message, created_at FROM tasks WHERE status = 'failed' ORDER BY created_at DESC LIMIT 20",
    ))
    .fetch_one(&state.pool)
    .await
    .unwrap_or(serde_json::json!([]));

    let audit_tail: serde_json::Value = crate::db::query_scalar(&json_rows(
        &["actor", "action", "resource_type", "resource_id", "created_at"],
        "SELECT actor, action, resource_type, resource_id, created_at FROM audit_logs ORDER BY created_at DESC LIMIT 50",
    ))
    .fetch_one(&state.pool)
    .await
    .unwrap_or(serde_json::json!([]));

    let version_matrix: serde_json::Value = crate::db::query_scalar(&json_rows(
        &["hostname", "agent_version", "libvirt_version", "qemu_version", "cpu_model", "state"],
        "SELECT hostname, agent_version, libvirt_version, qemu_version, cpu_model, state FROM hosts ORDER BY hostname",
    ))
    .fetch_one(&state.pool)
    .await
    .unwrap_or(serde_json::json!([]));

    let bundle = SupportBundleMeta {
        controller_id: state.config.controller_id.clone(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        task_count,
        host_count,
        vm_count,
        recent_failures,
        audit_tail,
        version_matrix,
    };

    Ok(bundle)
}

/// Words that name a secret: the value that follows (`key=value`, `key: value`) is never written to a bundle.
const SECRET_WORDS: [&str; 9] = [
    "password", "passwd", "secret", "token", "apikey", "api_key", "private", "credential", "authorization",
];

fn is_secret_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    SECRET_WORDS.iter().any(|w| k.contains(w)) || k.ends_with("_key") || k.ends_with("key_pem")
}

/// One log or config line with anything secret-looking replaced by `<redacted>`: values of secret-named keys,
/// the platform's own token shapes (`mat-…`, `join-…`), JWTs, bearer credentials and `user:password@` in URLs.
pub fn redact_line(line: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut mask_next = false;
    let mut mask_rest = false;
    for word in line.split(' ') {
        if mask_rest && !word.is_empty() {
            out.push("<redacted>".into());
            continue;
        }
        if mask_next && !word.is_empty() {
            mask_next = false;
            out.push("<redacted>".into());
            continue;
        }
        let lower = word.to_ascii_lowercase();
        if lower == "authorization:" {
            // the scheme and the credential after it ("Bearer x", "Basic x") are both secret
            mask_rest = true;
            out.push(word.into());
        } else if lower == "bearer" || lower == "bearer:" {
            mask_next = true;
            out.push(word.into());
        } else if let Some((k, _)) = word.split_once('=') {
            if is_secret_key(k) {
                out.push(format!("{k}=<redacted>"));
            } else {
                out.push(redact_url(word));
            }
        } else if word.starts_with("mat-") || word.starts_with("join-") || word.starts_with("eyJ") {
            out.push("<redacted>".into());
        } else {
            out.push(redact_url(word));
        }
    }
    out.join(" ")
}

/// `scheme://user:password@host` becomes `scheme://user:<redacted>@host`.
fn redact_url(word: &str) -> String {
    if let Some(i) = word.find("://") {
        let rest = &word[i + 3..];
        if let Some(at) = rest.find('@') {
            if let Some(colon) = rest[..at].find(':') {
                return format!("{}{}:<redacted>{}", &word[..i + 3], &rest[..colon], &rest[at..]);
            }
        }
    }
    word.to_string()
}

fn redact_text(text: &str) -> String {
    text.lines().map(redact_line).collect::<Vec<_>>().join("\n")
}

/// The controller's own MACHINA_* / DATABASE_URL / NATS_URL settings, names always, values only when not secret.
fn config_report() -> String {
    let mut vars: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| k.starts_with("MACHINA_") || k == "DATABASE_URL" || k == "NATS_URL" || k.starts_with("ATLAS_"))
        .collect();
    vars.sort();
    vars.into_iter()
        .map(|(k, v)| {
            if is_secret_key(&k) {
                format!("{k}=<redacted>")
            } else {
                redact_line(&format!("{k}={v}"))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The tail of a unit's journal, redacted; a note when journalctl is not there or the unit has no entries.
async fn journal_tail(unit: &str) -> String {
    let run = tokio::process::Command::new("journalctl")
        .args(["-u", unit, "-n", "1000", "--no-pager", "-o", "short-iso"])
        .output();
    match tokio::time::timeout(std::time::Duration::from_secs(10), run).await {
        Ok(Ok(o)) if o.status.success() && !o.stdout.is_empty() => redact_text(&String::from_utf8_lossy(&o.stdout)),
        _ => format!("(no journal available for {unit} from this controller process)"),
    }
}

/// `GET /api/v1/support/bundle.zip`: the JSON summary plus the redacted configuration and recent logs, one file to
/// attach to a support request. Nothing leaves the controller by itself.
pub async fn support_bundle_zip(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<impl IntoResponse, ApiError> {
    require_admin(&actor)?;
    let meta = collect(&state).await?;
    let summary = redact_text(&serde_json::to_string_pretty(&meta).map_err(|e| ApiError::internal(e.to_string()))?);
    let mut files: Vec<(String, String)> = vec![
        ("summary.json".into(), summary),
        ("config.env".into(), config_report()),
    ];
    for unit in ["machina-controller", "machina-daemon", "machina-agent", "machina-bpfd"] {
        files.push((format!("logs/{unit}.log"), journal_tail(unit).await));
    }
    files.push((
        "README.txt".into(),
        "Machina support bundle. Secrets (passwords, tokens, keys, URL credentials) are replaced by <redacted>; \
         review before sending. Generated by the controller; it is not uploaded anywhere.\n"
            .into(),
    ));
    let mut buf = Vec::new();
    {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (path, content) in files {
            zip.start_file(path, opts).map_err(|e| ApiError::internal(e.to_string()))?;
            zip.write_all(content.as_bytes()).map_err(|e| ApiError::internal(e.to_string()))?;
        }
        zip.finish().map_err(|e| ApiError::internal(e.to_string()))?;
    }
    let name = format!("machina-support-{}.zip", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "application/zip".to_string()),
            (axum::http::header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        buf,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_never_survive_redaction() {
        let tok = format!("mat-{}", "a".repeat(64));
        for line in [
            "MACHINA_ADMIN_PASSWORD=hunter2 started".to_string(),
            format!("agent token {tok} stored"),
            "Authorization: Bearer abc.def.ghi".to_string(),
            "dial postgres://machina:s3cret@db:5432/machina failed".to_string(),
            "jwt eyJhbGciOiJIUzI1NiJ9.e30.sig".to_string(),
            "join join-0123 now".to_string(),
        ] {
            let r = redact_line(&line);
            for bad in ["hunter2", tok.as_str(), "abc.def.ghi", "s3cret", "eyJhbGci", "join-0123"] {
                assert!(!r.contains(bad), "{bad} leaked in {r}");
            }
        }
        assert_eq!(redact_line("task failed host.sync"), "task failed host.sync");
        assert!(redact_line("postgres://machina:s3cret@db:5432/x").contains("machina:<redacted>@db"));
    }

    #[test]
    fn secret_named_keys_are_masked_in_the_config_report() {
        assert!(is_secret_key("MACHINA_JWT_SECRET"));
        assert!(is_secret_key("MACHINA_API_KEY_MASTER_KEY"));
        assert!(is_secret_key("ATLAS_TOKEN"));
        assert!(!is_secret_key("MACHINA_PUBLIC_URL"));
    }
}
