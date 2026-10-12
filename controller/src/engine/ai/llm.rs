// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::db::DbPool;

use super::providers::ResolvedProvider;
use super::routing::{RoutingRequest, TaskClass};

#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub task_class: TaskClass,
    pub system: String,
    pub user: String,
    pub agent_id: Option<String>,
    pub user_id: Option<String>,
}

/// Optional LLM completion — returns None when disabled or on failure.
pub async fn complete(pool: &DbPool, req: CompletionRequest) -> anyhow::Result<Option<String>> {
    if !super::settings::llm_enabled(pool).await? {
        return Ok(None);
    }
    let routing = RoutingRequest {
        task_class: req.task_class,
        agent_id: req.agent_id.clone(),
        user_id: req.user_id.clone(),
    };
    let Some(resolved) = super::routing::resolve(pool, &routing).await? else {
        return Ok(None);
    };
    complete_resolved(&resolved, &req.system, &req.user).await
}

/// Backward-compatible helper for existing call sites.
pub async fn complete_simple(
    pool: &DbPool,
    system: &str,
    user: &str,
) -> anyhow::Result<Option<String>> {
    complete(
        pool,
        CompletionRequest {
            task_class: TaskClass::Infrastructure,
            system: system.to_string(),
            user: user.to_string(),
            agent_id: None,
            user_id: None,
        },
    )
    .await
}

pub async fn probe_provider(resolved: &ResolvedProvider) -> anyhow::Result<bool> {
    let text = complete_resolved(
        resolved,
        "You are a connectivity probe.",
        "Reply with OK only.",
    )
    .await?;
    Ok(text.is_some())
}

async fn complete_resolved(
    resolved: &ResolvedProvider,
    system: &str,
    user: &str,
) -> anyhow::Result<Option<String>> {
    if resolved.api_key.is_empty() && !uses_local_endpoint(&resolved.kind) {
        return Ok(None);
    }
    if let Some(why) = unusable_kind(resolved) {
        tracing::warn!(provider = %resolved.kind, "refusing LLM request: {why}");
        return Ok(None);
    }
    if let Err(e) = validate_base_url(&resolved.base_url) {
        tracing::warn!(provider = %resolved.kind, "refusing LLM request: {e}");
        return Ok(None);
    }
    match resolved.kind.as_str() {
        "anthropic" => anthropic_complete(resolved, system, user).await,
        "google" | "gemini" => google_complete(resolved, system, user).await,
        _ => openai_compatible_complete(resolved, system, user).await,
    }
}

/// Reject cloud-metadata endpoints for an admin-configured LLM `base_url`
/// (BYOK / custom OpenAI-compatible endpoints, incl. Ollama and vLLM).
///
/// This is intentionally NOT a general private-IP/SSRF blocklist: RFC1918
/// ranges (10/8, 172.16/12, 192.168/16) and loopback (127.0.0.1, ::1,
/// localhost) are the whole point of self-hosted local inference (Ollama on
/// 127.0.0.1:11434, vLLM on a LAN GPU box, etc.) and must keep working.
/// What has zero legitimate use as an LLM endpoint is the cloud instance
/// metadata service — a classic SSRF target for stealing IAM/IMDS
/// credentials — so only those well-known addresses are blocked here.
pub(super) fn validate_base_url(base_url: &str) -> Result<(), String> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let host = trimmed
        .parse::<reqwest::Url>()
        .map(|u| u.host_str().unwrap_or("").to_string())
        .unwrap_or_default();
    let host = normalize_host_for_metadata_check(&host);
    const BLOCKED_METADATA_HOSTS: &[&str] = &[
        // AWS / Azure / Alibaba / Oracle / DigitalOcean IMDS (all serve on this address)
        "169.254.169.254",
        // GCP metadata server
        "metadata.google.internal",
        "metadata.google",
        "metadata",
        // AWS IMDSv2 IPv6 endpoint
        "fd00:ec2::254",
        // Alibaba Cloud alias
        "100.100.100.200",
    ];
    if BLOCKED_METADATA_HOSTS.contains(&host.as_str()) {
        return Err(format!(
            "base_url host '{host}' is a cloud metadata endpoint and is not a valid LLM target"
        ));
    }
    Ok(())
}

/// Canonicalize a `Url::host_str()` value before comparing it against the
/// blocklist above.
///
/// `host_str()` returns IPv6 hosts bracketed (e.g. `"[fd00:ec2::254]"`) and
/// leaves an IPv4-mapped IPv6 literal (e.g. `"::ffff:169.254.169.254"`) in its
/// compressed hextet form (`"::ffff:a9fe:a9fe"`) rather than the dotted-quad
/// the blocklist is written in — a base_url written either way previously sailed
/// straight past the literal string comparison and reached the metadata IP
/// anyway, defeating the SSRF guard entirely.
fn normalize_host_for_metadata_check(host: &str) -> String {
    let mut host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if let Some(stripped) = host.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        host = stripped.to_string();
    }
    if let Ok(std::net::IpAddr::V6(v6)) = host.parse::<std::net::IpAddr>() {
        if let Some(v4) = v6.to_ipv4_mapped() {
            host = v4.to_string();
        }
    }
    host
}

pub(super) fn uses_local_endpoint(kind: &str) -> bool {
    matches!(kind, "ollama" | "vllm" | "openai_compatible")
}

/// Why a provider cannot be called, or `None` when it can. Kinds the code no longer has
/// an endpoint for must not fall through to api.openai.com and leak their key there.
pub(super) fn unusable_kind(resolved: &ResolvedProvider) -> Option<&'static str> {
    match resolved.kind.as_str() {
        "azure_openai" => Some("azure_openai was removed (local-first); use ollama, vllm or openai_compatible"),
        "meta" | "qwen" if resolved.base_url.trim().is_empty() => {
            Some("this provider kind has no built-in endpoint; set a base URL (or use openai_compatible)")
        }
        _ => None,
    }
}

pub(super) fn openai_base(resolved: &ResolvedProvider) -> String {
    if !resolved.base_url.trim().is_empty() {
        let base = resolved.base_url.trim().trim_end_matches('/');
        if base.ends_with("/v1") {
            return format!("{base}/chat/completions");
        }
        return format!("{base}/v1/chat/completions");
    }
    match resolved.kind.as_str() {
        "xai" => "https://api.x.ai/v1/chat/completions".into(),
        "deepseek" => "https://api.deepseek.com/v1/chat/completions".into(),
        "mistral" => "https://api.mistral.ai/v1/chat/completions".into(),
        "ollama" => "http://127.0.0.1:11434/v1/chat/completions".into(),
        "vllm" => {
            let base = resolved.base_url.trim().trim_end_matches('/');
            let base = if base.is_empty() {
                "http://127.0.0.1:8000"
            } else {
                base
            };
            format!("{base}/v1/chat/completions")
        }
        _ => "https://api.openai.com/v1/chat/completions".into(),
    }
}

async fn openai_compatible_complete(
    resolved: &ResolvedProvider,
    system: &str,
    user: &str,
) -> anyhow::Result<Option<String>> {
    let model = resolved.model_id.clone();
    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ],
        "max_tokens": 1024
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .build()?;
    let url = openai_base(resolved);
    let mut req = client.post(&url).json(&body);
    if !resolved.api_key.is_empty() {
        req = req.bearer_auth(&resolved.api_key);
    }
    if !resolved.org_id.is_empty() {
        req = req.header("OpenAI-Organization", &resolved.org_id);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        tracing::warn!(
            "llm error ({}): {}",
            resolved.kind,
            resp.text().await.unwrap_or_default()
        );
        return Ok(None);
    }
    let v: serde_json::Value = resp.json().await?;
    let text = v["choices"][0]["message"]["content"]
        .as_str()
        .map(String::from);
    if text.is_none() {
        tracing::warn!(provider = %resolved.kind, "unexpected LLM response shape (no choices[0].message.content): {}", v);
    }
    Ok(text)
}

async fn anthropic_complete(
    resolved: &ResolvedProvider,
    system: &str,
    user: &str,
) -> anyhow::Result<Option<String>> {
    let url = if resolved.base_url.trim().is_empty() {
        "https://api.anthropic.com/v1/messages".to_string()
    } else {
        format!(
            "{}/v1/messages",
            resolved.base_url.trim().trim_end_matches('/')
        )
    };
    let body = serde_json::json!({
        "model": resolved.model_id,
        "max_tokens": 1024,
        "system": system,
        "messages": [{"role": "user", "content": user}]
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .build()?;
    let resp = client
        .post(url)
        .header("x-api-key", &resolved.api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await?;
    if !resp.status().is_success() {
        tracing::warn!("anthropic error: {}", resp.text().await.unwrap_or_default());
        return Ok(None);
    }
    let v: serde_json::Value = resp.json().await?;
    let text = v["content"][0]["text"].as_str().map(String::from);
    if text.is_none() {
        tracing::warn!(
            provider = "anthropic",
            "unexpected Anthropic response shape: {}",
            v
        );
    }
    Ok(text)
}

async fn google_complete(
    resolved: &ResolvedProvider,
    system: &str,
    user: &str,
) -> anyhow::Result<Option<String>> {
    let model = resolved.model_id.clone();
    let url = if resolved.base_url.trim().is_empty() {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            model
        )
    } else {
        format!(
            "{}/v1beta/models/{}:generateContent",
            resolved.base_url.trim().trim_end_matches('/'),
            model,
        )
    };
    let body = serde_json::json!({
        "contents": [{"parts": [{"text": format!("{system}\n\n{user}")}]}]
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .build()?;
    let resp = client
        .post(url)
        .header("x-goog-api-key", &resolved.api_key)
        .json(&body)
        .send()
        .await?;
    if !resp.status().is_success() {
        tracing::warn!("google error: {}", resp.text().await.unwrap_or_default());
        return Ok(None);
    }
    let v: serde_json::Value = resp.json().await?;
    let text = v["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(String::from);
    if text.is_none() {
        tracing::warn!(
            provider = "google",
            "unexpected Google response shape (safety block or empty parts?): {}",
            v
        );
    }
    Ok(text)
}

#[cfg(test)]
mod local_first_tests {
    use super::*;

    fn p(kind: &str, base_url: &str) -> ResolvedProvider {
        ResolvedProvider {
            provider_id: uuid::Uuid::nil(),
            kind: kind.into(),
            base_url: base_url.into(),
            org_id: String::new(),
            deployment_name: String::new(),
            api_key: String::new(),
            model_id: "m".into(),
        }
    }

    #[test]
    fn removed_and_endpointless_kinds_are_unusable() {
        assert!(unusable_kind(&p("azure_openai", "https://x")).is_some());
        assert!(unusable_kind(&p("meta", "")).is_some());
        assert!(unusable_kind(&p("meta", "http://127.0.0.1:8000")).is_none());
        assert!(unusable_kind(&p("ollama", "")).is_none());
        assert!(unusable_kind(&p("openai", "")).is_none());
    }
}
