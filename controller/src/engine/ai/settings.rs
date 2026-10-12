// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::db::DbPool;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiSettings {
    pub enabled: bool,
    pub mode: String,
    pub provider: String,
    pub model: String,
    pub api_key_configured: bool,
    pub autopilot_interval_secs: i32,
    pub autopilot_max_actions: i32,
    pub fleet_peer_urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autopilot_last_run: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AiSettingsPatch {
    pub enabled: Option<bool>,
    pub mode: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub autopilot_interval_secs: Option<i32>,
    pub autopilot_max_actions: Option<i32>,
    pub fleet_peer_urls: Option<Vec<String>>,
}

pub async fn get_ai_settings(pool: &DbPool) -> anyhow::Result<AiSettings> {
    let row: (
        bool,
        String,
        String,
        String,
        String,
        i32,
        Option<chrono::DateTime<chrono::Utc>>,
        i32,
        serde_json::Value,
    ) = crate::db::query_as(
        "SELECT ai_enabled, ai_mode, ai_provider, ai_model, COALESCE(ai_api_key, ''),
         ai_autopilot_interval_secs, ai_autopilot_last_run, ai_autopilot_max_actions,
         COALESCE(ai_fleet_peer_urls, '[]')
         FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_one(pool)
    .await?;
    let fleet_peer_urls: Vec<String> = serde_json::from_value(row.8).unwrap_or_default();
    Ok(AiSettings {
        enabled: row.0,
        mode: row.1,
        provider: row.2,
        model: row.3,
        api_key_configured: !row.4.is_empty(),
        autopilot_interval_secs: row.5,
        autopilot_last_run: row.6.map(|t| t.to_rfc3339()),
        autopilot_max_actions: row.7.clamp(1, 10),
        fleet_peer_urls,
    })
}

pub async fn patch_ai_settings(
    pool: &DbPool,
    patch: &AiSettingsPatch,
) -> anyhow::Result<AiSettings> {
    let mut tx = pool.begin().await?;
    if let Some(v) = patch.enabled {
        crate::db::query("UPDATE clusters SET ai_enabled = ?")
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.mode {
        crate::db::query("UPDATE clusters SET ai_mode = ?")
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.provider {
        crate::db::query("UPDATE clusters SET ai_provider = ?")
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.model {
        crate::db::query("UPDATE clusters SET ai_model = ?")
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.api_key {
        // Encrypt at rest like the providers path does — this legacy settings
        // route was binding the raw key, defeating MACHINA_API_KEY_MASTER_KEY.
        // legacy_resolve reads it back through crypto::load_api_key, which
        // transparently handles both encrypted and (dev/legacy) plaintext values.
        let stored = super::crypto::store_api_key(v.trim())?;
        crate::db::query("UPDATE clusters SET ai_api_key = ?")
            .bind(stored)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = patch.autopilot_interval_secs {
        crate::db::query("UPDATE clusters SET ai_autopilot_interval_secs = ?")
            .bind(v.clamp(0, 86400))
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = patch.autopilot_max_actions {
        crate::db::query("UPDATE clusters SET ai_autopilot_max_actions = ?")
            .bind(v.clamp(1, 10))
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.fleet_peer_urls {
        let json = serde_json::to_value(v)?;
        crate::db::query("UPDATE clusters SET ai_fleet_peer_urls = ?")
            .bind(json)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    get_ai_settings(pool).await
}

pub async fn get_fleet_peer_urls(pool: &DbPool) -> anyhow::Result<Vec<String>> {
    Ok(get_ai_settings(pool).await?.fleet_peer_urls)
}

pub async fn autopilot_max_actions(pool: &DbPool) -> anyhow::Result<usize> {
    Ok(get_ai_settings(pool)
        .await?
        .autopilot_max_actions
        .clamp(1, 10) as usize)
}

pub async fn api_key(pool: &DbPool) -> anyhow::Result<Option<String>> {
    if std::env::var("MACHINA_AI_DISABLED").ok().as_deref() == Some("1") {
        return Ok(None);
    }
    if let Ok(k) = std::env::var("MACHINA_AI_API_KEY") {
        if !k.is_empty() {
            return Ok(Some(k));
        }
    }
    let key: String = crate::db::query_scalar(
        "SELECT COALESCE(ai_api_key, '') FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_one(pool)
    .await?;
    if key.is_empty() {
        Ok(None)
    } else {
        // Stored value may be an `enc:`-prefixed ciphertext (patch_ai_settings
        // encrypts via crypto::store_api_key) or legacy plaintext — decrypt so
        // callers get the real key instead of a ciphertext blob that would be
        // sent to the LLM provider as a bearer token and silently fail auth.
        Ok(Some(super::crypto::load_api_key(&key)?))
    }
}

pub async fn llm_enabled(pool: &DbPool) -> anyhow::Result<bool> {
    let s = get_ai_settings(pool).await?;
    if !s.enabled || std::env::var("MACHINA_AI_DISABLED").ok().as_deref() == Some("1") {
        return Ok(false);
    }
    if s.api_key_configured && api_key(pool).await?.is_some() {
        return Ok(true);
    }
    // No hosted key: AI still works when a local provider (Ollama, vLLM,
    // llama.cpp) is enabled, since those endpoints take no key.
    Ok(super::providers::resolve_local(pool).await?.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    async fn cluster_with_ai(pool: &DbPool) {
        crate::db::query("INSERT INTO clusters (id, name) VALUES (?, 'c')")
            .bind(Uuid::from_u128(1))
            .execute(pool)
            .await
            .unwrap();
        crate::db::query("UPDATE clusters SET ai_enabled = TRUE")
            .execute(pool)
            .await
            .unwrap();
    }

    async fn add_provider(pool: &DbPool, kind: &str, base_url: &str, key: &str) -> Uuid {
        let id = Uuid::new_v4();
        crate::db::query(
            "INSERT INTO ai_providers (id, name, kind, base_url, api_key_encrypted, is_default)
             VALUES (?, ?, ?, ?, ?, TRUE)",
        )
        .bind(id)
        .bind(kind)
        .bind(kind)
        .bind(base_url)
        .bind(key)
        .execute(pool)
        .await
        .unwrap();
        id
    }

    #[tokio::test]
    async fn keyless_local_provider_enables_llm() {
        if std::env::var("MACHINA_AI_DISABLED").is_ok()
            || std::env::var("MACHINA_AI_API_KEY").is_ok()
        {
            return;
        }
        let pool = crate::db::testing::pool().await;
        cluster_with_ai(&pool).await;
        assert!(!llm_enabled(&pool).await.unwrap());
        add_provider(&pool, "ollama", "http://127.0.0.1:11434", "").await;
        assert!(llm_enabled(&pool).await.unwrap());
        let r = super::super::providers::resolve_default(&pool)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(r.kind, "ollama");
        assert!(r.api_key.is_empty());
    }

    #[tokio::test]
    async fn keyless_hosted_provider_stays_disabled() {
        if std::env::var("MACHINA_AI_DISABLED").is_ok()
            || std::env::var("MACHINA_AI_API_KEY").is_ok()
        {
            return;
        }
        let pool = crate::db::testing::pool().await;
        cluster_with_ai(&pool).await;
        add_provider(&pool, "openai", "", "").await;
        assert!(!llm_enabled(&pool).await.unwrap());
        assert!(super::super::providers::resolve_default(&pool)
            .await
            .unwrap()
            .is_none());
    }
}
