// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! AES-256-GCM encryption for LLM provider API keys stored in Postgres.
//!
//! Encrypted values are stored as `enc:<base64(12-byte-nonce || ciphertext)>`.
//! Plaintext values (legacy rows) have no prefix and pass through unchanged so
//! existing deployments keep working without a migration.
//!
//! Call `init()` once at startup (before any DB operations) to load the master
//! key from `MACHINA_API_KEY_MASTER_KEY` (64 hex chars = 32 bytes).

use std::sync::OnceLock;

use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::OsRng;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};

const ENC_PREFIX: &str = "enc:";

static MASTER_KEY: OnceLock<Option<[u8; 32]>> = OnceLock::new();

/// Load the master key from `MACHINA_API_KEY_MASTER_KEY_FILE` or the legacy
/// environment variable. The file form is preferred in production (systemd
/// credentials, Vault/secret-agent rendered file, Kubernetes secret volume).
pub fn init() {
    let raw = std::env::var("MACHINA_API_KEY_MASTER_KEY_FILE")
        .ok()
        .filter(|p| !p.trim().is_empty())
        .and_then(|p| match std::fs::read_to_string(p.trim()) {
            Ok(v) => Some(v),
            Err(e) => { tracing::error!("cannot read MACHINA_API_KEY_MASTER_KEY_FILE: {e}"); None }
        })
        .or_else(|| std::env::var("MACHINA_API_KEY_MASTER_KEY").ok());
    let key = raw.and_then(|s| {
        match hex::decode(s.trim()) {
            Ok(bytes) => bytes.try_into().ok(),
            Err(_) => {
                tracing::warn!("MACHINA_API_KEY_MASTER_KEY is set but is not valid 64-char hex — API key encryption disabled");
                None
            }
        }
    });
    if key.is_some() {
        tracing::info!("AES-256-GCM API key encryption enabled");
    } else {
        tracing::warn!("no API-key master key configured — provider/BMC secrets may be stored in plaintext; production profile refuses this");
    }
    let _ = MASTER_KEY.set(key);
}

fn master_key() -> Option<&'static [u8; 32]> {
    MASTER_KEY.get().and_then(|k| k.as_ref())
}

/// Encrypt `plaintext` with AES-256-GCM. Returns `enc:<b64(nonce||ct)>`.
fn encrypt(key: &[u8; 32], plaintext: &str) -> anyhow::Result<String> {
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|e| anyhow::anyhow!("aes-gcm key init: {e}"))?;
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("aes-gcm encrypt: {e}"))?;
    let mut blob = nonce_bytes.to_vec();
    blob.extend_from_slice(&ciphertext);
    use base64::Engine as _;
    Ok(format!(
        "{ENC_PREFIX}{}",
        base64::engine::general_purpose::STANDARD.encode(&blob)
    ))
}

fn decrypt(key: &[u8; 32], stored: &str) -> anyhow::Result<String> {
    let b64 = stored
        .strip_prefix(ENC_PREFIX)
        .ok_or_else(|| anyhow::anyhow!("value is not in enc: format"))?;
    use base64::Engine as _;
    let blob = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| anyhow::anyhow!("base64 decode: {e}"))?;
    if blob.len() < 28 {
        anyhow::bail!("encrypted blob too short (need ≥28 bytes: 12 nonce + 16 GCM tag)");
    }
    let (nonce_bytes, ciphertext) = blob.split_at(12);
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|e| anyhow::anyhow!("aes-gcm key init: {e}"))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext_bytes = cipher.decrypt(nonce, ciphertext).map_err(|_| {
        anyhow::anyhow!("API key decryption failed — wrong MACHINA_API_KEY_MASTER_KEY?")
    })?;
    String::from_utf8(plaintext_bytes).map_err(|e| anyhow::anyhow!("utf8: {e}"))
}

/// Prepare an API key for storage. Encrypts if master key is configured.
pub fn store_api_key(plaintext: &str) -> anyhow::Result<String> {
    match master_key() {
        Some(k) => encrypt(k, plaintext),
        None => Ok(plaintext.to_owned()),
    }
}

/// Retrieve an API key from storage. Decrypts `enc:` values; passes through plaintext.
pub fn load_api_key(stored: &str) -> anyhow::Result<String> {
    if stored.starts_with(ENC_PREFIX) {
        let k = master_key().ok_or_else(|| {
            anyhow::anyhow!("API key is encrypted but MACHINA_API_KEY_MASTER_KEY is not configured")
        })?;
        decrypt(k, stored)
    } else {
        Ok(stored.to_owned())
    }
}
