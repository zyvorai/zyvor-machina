// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! User-facing HTTP/API error formatting (shared by daemon helpers and TUI).

use serde::Deserialize;

const API_ERROR_LABELS: &[(&str, &str)] = &[
    ("operation_failed", "The operation failed on the server"),
    ("not_found", "The requested resource was not found"),
    ("invalid_request", "The request was invalid"),
    ("forbidden", "You do not have permission for this action"),
    (
        "libvirt_connection",
        "Could not connect to libvirt on the host",
    ),
    ("internal_error", "An internal server error occurred"),
    (
        "unauthorized",
        "Authentication required or your session expired",
    ),
];

/// Human label for a stable API `error_code`.
pub fn friendly_error_code(code: &str) -> String {
    for (k, v) in API_ERROR_LABELS {
        if *k == code {
            return (*v).to_string();
        }
    }
    code.replace('_', " ")
}

/// Strip HTML pages and trim noisy API text for UI display.
pub fn sanitize_error_text(text: &str) -> String {
    let t = text.trim();
    if t.is_empty() {
        return String::new();
    }

    let lower = t.to_ascii_lowercase();
    if lower.contains("<!doctype html") || lower.contains("<html") || t.contains("</html>") {
        return "The API returned an HTML error page instead of JSON. \
            This usually means a reverse proxy failure or a backend service is down."
            .to_string();
    }

    if t.contains('<') && t.contains('>') {
        let stripped: String = t
            .split('<')
            .map(|seg| seg.split('>').nth(1).unwrap_or(seg))
            .collect::<Vec<_>>()
            .join(" ");
        let stripped = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
        if !stripped.is_empty() && stripped.len() < t.len() * 6 / 10 {
            return truncate_plain(&stripped, 500);
        }
    }

    truncate_plain(t, 600)
}

fn truncate_plain(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let end = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
        format!("{}…", &s[..end])
    }
}

#[derive(Debug, Deserialize)]
struct ApiErrorJson {
    error: Option<String>,
    message: Option<String>,
    error_code: Option<String>,
}

/// Build a user-facing message from an HTTP error body (JSON `{ error, error_code }` or plain/HTML).
pub fn format_http_error_body(status: u16, status_text: &str, text: &str) -> String {
    let raw = text.trim();
    let status_label = if status_text.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status} {status_text}")
    };

    if raw.is_empty() {
        return format!("Request failed ({status_label})");
    }

    let lower = raw.to_ascii_lowercase();
    if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
        return format!(
            "Request failed ({status_label}): the server returned an HTML error page instead of JSON. \
             Check that machina-daemon is running and reachable."
        );
    }

    if let Ok(j) = serde_json::from_str::<ApiErrorJson>(raw) {
        let code = j.error_code.as_deref();
        let raw_msg = j.error.as_deref().or(j.message.as_deref()).unwrap_or("");
        let clean = sanitize_error_text(raw_msg);

        if !clean.is_empty() {
            if let Some(c) = code {
                let label = friendly_error_code(c);
                if clean == c || clean == label {
                    return label;
                }
                if !clean.to_ascii_lowercase().contains(&c.replace('_', " ")) {
                    return format!("{clean} ({label})");
                }
            }
            return clean;
        }
        if let Some(c) = code {
            return friendly_error_code(c);
        }
    }

    let sanitized = sanitize_error_text(raw);
    if sanitized != raw {
        return sanitized;
    }
    if raw.len() > 400 {
        // Byte-index slicing panics if `raw` contains multi-byte UTF-8 and byte
        // 200 doesn't land on a char boundary (this text comes from an external
        // HTTP response body, so it's not guaranteed to be ASCII); truncate on a
        // char boundary instead, same as `truncate_plain` above.
        let end = raw
            .char_indices()
            .nth(200)
            .map(|(i, _)| i)
            .unwrap_or(raw.len());
        return format!("Request failed ({status_label}): {}…", &raw[..end]);
    }
    format!("Request failed ({status_label}): {raw}")
}

/// Format any error string for status bars and toasts (strips HTML, maps bare error codes).
pub fn format_user_error(msg: &str) -> String {
    let msg = sanitize_error_text(msg);
    if msg.is_empty() {
        return "Unknown error".to_string();
    }
    for (code, label) in API_ERROR_LABELS {
        let suffix = format!("({code})");
        if msg.ends_with(&suffix) {
            let base = msg.trim_end_matches(&suffix).trim();
            if base.len() < 8 {
                return label.to_string();
            }
        }
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_body_is_sanitized() {
        let body = "<!DOCTYPE html><html><body>Bad Gateway</body></html>";
        let msg = format_http_error_body(502, "Bad Gateway", body);
        assert!(!msg.contains("</html>"));
        assert!(msg.contains("HTML"));
    }

    #[test]
    fn json_error_code_only() {
        let body = r#"{"error_code":"operation_failed"}"#;
        let msg = format_http_error_body(500, "", body);
        assert!(msg.contains("operation failed"));
    }

    #[test]
    fn json_error_and_code() {
        let body = r#"{"error":"Neutron down","error_code":"operation_failed"}"#;
        let msg = format_http_error_body(503, "", body);
        assert!(msg.contains("Neutron"));
        assert!(msg.contains("operation failed"));
    }
}
