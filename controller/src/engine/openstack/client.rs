// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub auth_url: String,
    pub username: String,
    pub password: String,
    pub user_domain: String,
    pub project_name: String,
    pub project_domain: String,
    pub region: String,
    pub app_cred_id: String,
    pub app_cred_secret: String,
    pub insecure_tls: bool,
}

#[derive(Debug, Clone)]
pub struct OpenStackClient {
    http: Client,
    token: String,
    pub compute: String,
    pub image: String,
    pub volume: String,
}

impl OpenStackClient {
    pub async fn connect(cfg: &ConnectionConfig) -> anyhow::Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(120))
            .danger_accept_invalid_certs(cfg.insecure_tls)
            .build()?;
        let auth_url = cfg.auth_url.trim_end_matches('/');
        let identity = if !cfg.app_cred_secret.is_empty() {
            json!({
                "methods": ["application_credential"],
                "application_credential": {
                    "id": cfg.app_cred_id,
                    "secret": cfg.app_cred_secret,
                }
            })
        } else {
            json!({
                "methods": ["password"],
                "password": {
                    "user": {
                        "name": cfg.username,
                        "domain": {"name": cfg.user_domain},
                        "password": cfg.password,
                    }
                }
            })
        };
        let body = json!({
            "auth": {
                "identity": identity,
                "scope": {
                    "project": {
                        "name": cfg.project_name,
                        "domain": {"name": cfg.project_domain},
                    }
                }
            }
        });
        let resp = http
            .post(format!("{auth_url}/auth/tokens"))
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Keystone auth failed ({status}): {}", text.chars().take(500).collect::<String>());
        }
        let token = resp
            .headers()
            .get("x-subject-token")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| anyhow::anyhow!("Keystone response missing X-Subject-Token"))?
            .to_string();
        let payload: Value = resp.json().await.unwrap_or(json!({}));
        let catalog = payload
            .pointer("/token/catalog")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let endpoint = |type_name: &str| -> String {
            for svc in &catalog {
                if svc.get("type").and_then(|v| v.as_str()) != Some(type_name) {
                    continue;
                }
                if let Some(eps) = svc.get("endpoints").and_then(|v| v.as_array()) {
                    for ep in eps {
                        let iface = ep.get("interface").and_then(|v| v.as_str()).unwrap_or("");
                        let region = ep.get("region").and_then(|v| v.as_str()).unwrap_or("");
                        if (iface == "public" || iface == "internal")
                            && (cfg.region.is_empty() || region == cfg.region || region.is_empty())
                        {
                            if let Some(url) = ep.get("url").and_then(|v| v.as_str()) {
                                return url.trim_end_matches('/').to_string();
                            }
                        }
                    }
                }
            }
            String::new()
        };
        Ok(Self {
            http,
            token,
            compute: endpoint("compute"),
            image: endpoint("image"),
            volume: {
                let v = endpoint("volumev3");
                if v.is_empty() {
                    endpoint("volume")
                } else {
                    v
                }
            },
        })
    }

    pub async fn get_json(&self, url: &str) -> anyhow::Result<Value> {
        let resp = self
            .http
            .get(url)
            .header("X-Auth-Token", &self.token)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET {url} failed ({status}): {}", text.chars().take(500).collect::<String>());
        }
        Ok(resp.json().await?)
    }

    pub async fn download_bytes(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        let resp = self
            .http
            .get(url)
            .header("X-Auth-Token", &self.token)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            anyhow::bail!("download {url} failed ({status})");
        }
        Ok(resp.bytes().await?.to_vec())
    }

    pub fn token(&self) -> &str {
        &self.token
    }
}
