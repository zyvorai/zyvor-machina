// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Atlas — Zyvor storage control plane bridge.
//
// Machina calls stable Atlas REST APIs (`/api/atlas/v1/...`) to provision VM
// disks as backend volumes (Ceph RBD / NFS / ZFS) and to route VM
// snapshot / backup / restore through Atlas. Atlas owns the storage backends,
// inventory, ownership bindings and audit; machina stays decoupled from Ceph.
//
// This module is the thin async HTTP client + typed DTOs. Higher-level,
// VM-oriented orchestration (create a volume for a VM, snapshot/backup a VM
// disk) lives in `engine::atlas_vm`.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::ControllerConfig;

const API: &str = "/api/atlas/v1";

/// Reachability + version probe surfaced on the platform Storage page.
#[derive(Debug, Clone, Serialize)]
pub struct AtlasStatus {
    pub enabled: bool,
    pub base_url: String,
    pub reachable: bool,
    pub authenticated: bool,
    pub version: Option<String>,
    pub tenant_id: String,
    pub default_policy: String,
    pub backup_bucket_id: Option<String>,
    pub summary: String,
}

/// A registered Atlas storage backend (ceph/nfs/zfs/...).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtlasBackend {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub backend_type: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub capabilities: serde_json::Value,
}

/// A normalized volume in Atlas inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtlasVolume {
    pub id: String,
    #[serde(default)]
    pub cluster_id: Option<String>,
    #[serde(default)]
    pub pool_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub backend_native_id: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<i64>,
    #[serde(default)]
    pub used_bytes: Option<i64>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub health: Option<String>,
}

/// An Atlas async job. Create responses carry `job_id`; `GET /jobs/{id}` carries
/// `id` — [`AtlasJob::job_id`] normalizes over both.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtlasJob {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default, rename = "job_id")]
    pub job_id_field: Option<String>,
    #[serde(default)]
    pub job_type: Option<String>,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub progress_percent: Option<i64>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub result: serde_json::Value,
    #[serde(default)]
    pub resource: serde_json::Value,
}

impl AtlasJob {
    /// The job id regardless of which field Atlas populated.
    pub fn job_id(&self) -> Option<&str> {
        self.job_id_field.as_deref().or(self.id.as_deref())
    }

    /// The `volume_id` from a create/clone/restore job's `resource` block, if any.
    pub fn resource_volume_id(&self) -> Option<String> {
        self.resource_str("volume_id")
    }

    /// The `backup_id` from a backup job's `resource` block, if any.
    pub fn resource_backup_id(&self) -> Option<String> {
        self.resource_str("backup_id")
    }

    fn resource_str(&self, key: &str) -> Option<String> {
        self.resource
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.state.as_str(), "succeeded" | "failed")
    }
}

/// Typed error from an Atlas API call, carrying the upstream HTTP status so the
/// API layer can propagate it (e.g. a 409 conflict stays a 409, not a flat 502).
#[derive(Debug, Clone)]
pub struct AtlasApiError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for AtlasApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.code.is_empty() {
            write!(f, "atlas {}: {}", self.status, self.message)
        } else {
            write!(f, "atlas {} {}: {}", self.status, self.code, self.message)
        }
    }
}

impl std::error::Error for AtlasApiError {}

/// Owner binding recorded on an Atlas volume so a product can enumerate only the
/// volumes it owns (`ListVolumesByOwner`). See Atlas API `owner` block.
#[derive(Debug, Clone, Serialize)]
pub struct AtlasOwner {
    pub product: String,
    pub resource_type: String,
    pub resource_id: String,
    pub role: String,
}

/// Async client bound to the configured Atlas gateway + service-account token.
#[derive(Clone)]
pub struct AtlasClient {
    http: reqwest::Client,
    base_url: String,
    token: Option<String>,
}

impl AtlasClient {
    /// Build a client from controller config. Returns `None` when the Atlas
    /// integration is disabled (`ATLAS_ENABLED=0`).
    pub fn from_config(cfg: &ControllerConfig) -> anyhow::Result<Option<Self>> {
        if !cfg.atlas_enabled {
            return Ok(None);
        }
        Ok(Some(Self::build(cfg)?))
    }

    /// Build a client ignoring the enabled flag (used by `status`).
    fn build(cfg: &ControllerConfig) -> anyhow::Result<Self> {
        let mut b = reqwest::Client::builder().timeout(Duration::from_secs(120));
        if cfg.atlas_insecure_tls {
            b = b.danger_accept_invalid_certs(true);
        }
        Ok(Self {
            http: b.build()?,
            base_url: cfg.atlas_base_url.trim_end_matches('/').to_string(),
            token: cfg.atlas_token.clone(),
        })
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.token.as_deref().filter(|t| !t.is_empty()) {
            Some(t) => req.header("Authorization", format!("Bearer {t}")),
            None => req,
        }
    }

    fn url(&self, path: &str) -> String {
        if path.starts_with('/') {
            format!("{}{}{}", self.base_url, API, path)
        } else {
            format!("{}{}/{}", self.base_url, API, path)
        }
    }

    /// Percent-encode a caller-supplied resource id before it is spliced into a
    /// URL path or query string. IDs (volume/snapshot/backup/job) ultimately
    /// come from HTTP path/query parameters one hop up (see `api/atlas.rs`) and
    /// are never validated as UUIDs before reaching this client — without
    /// encoding, a `/`, `?`, `&`, or `..` in an id could redirect the request to
    /// a different Atlas endpoint or smuggle extra query parameters into the
    /// upstream call made with our privileged service-account bearer token.
    fn seg(id: &str) -> String {
        urlencoding::encode(id).into_owned()
    }

    async fn get(&self, path: &str) -> anyhow::Result<serde_json::Value> {
        let resp = self.auth(self.http.get(self.url(path))).send().await?;
        Self::json(resp).await
    }

    async fn get_as<T: for<'de> Deserialize<'de>>(&self, path: &str) -> anyhow::Result<T> {
        let v = self.get(path).await?;
        Ok(serde_json::from_value(v)?)
    }

    async fn post(&self, path: &str, body: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        let resp = self
            .auth(self.http.post(self.url(path)).json(&body))
            .send()
            .await?;
        Self::json(resp).await
    }

    async fn delete(&self, path: &str) -> anyhow::Result<serde_json::Value> {
        let resp = self.auth(self.http.delete(self.url(path))).send().await?;
        Self::json(resp).await
    }

    /// Consume a response, mapping non-2xx to an error that carries Atlas'
    /// `{ error: { code, message } }` body when present.
    async fn json(resp: reqwest::Response) -> anyhow::Result<serde_json::Value> {
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
        if status.is_success() {
            return Ok(body);
        }
        let message = body
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| body.to_string());
        let code = body
            .pointer("/error/code")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Err(anyhow::Error::new(AtlasApiError {
            status: status.as_u16(),
            code,
            message,
        }))
    }

    // ---- Meta -------------------------------------------------------------

    pub async fn version(&self) -> anyhow::Result<serde_json::Value> {
        // /version and /health are served off the gateway root, not under /api.
        let url = format!("{}/version", self.base_url);
        let resp = self.auth(self.http.get(url)).send().await?;
        Self::json(resp).await
    }

    pub async fn health(&self) -> anyhow::Result<serde_json::Value> {
        let url = format!("{}/health", self.base_url);
        let resp = self.auth(self.http.get(url)).send().await?;
        Self::json(resp).await
    }

    // ---- Inventory --------------------------------------------------------

    pub async fn list_backends(&self) -> anyhow::Result<Vec<AtlasBackend>> {
        self.get_as("/backends").await
    }

    pub async fn list_pools(&self) -> anyhow::Result<serde_json::Value> {
        self.get("/pools").await
    }

    pub async fn list_clusters(&self) -> anyhow::Result<serde_json::Value> {
        self.get("/clusters").await
    }

    pub async fn metrics_summary(&self) -> anyhow::Result<serde_json::Value> {
        self.get("/metrics/summary").await
    }

    /// Least-squares capacity projection (`days_to_full`, `growth_bytes_per_day`, ...)
    /// over the last `minutes` of Atlas' persisted history.
    pub async fn forecast(&self, minutes: i64) -> anyhow::Result<serde_json::Value> {
        self.get(&format!(
            "/metrics/forecast?minutes={}",
            minutes.clamp(1, 20160)
        ))
        .await
    }

    /// Alerts raised by Atlas' monitor worker; `state` is e.g. `open`.
    pub async fn list_alerts(&self, state: Option<&str>) -> anyhow::Result<serde_json::Value> {
        match state {
            Some(st) => {
                self.get(&format!("/alerts?state={}", urlencoding::encode(st)))
                    .await
            }
            None => self.get("/alerts").await,
        }
    }

    pub async fn list_policies(&self) -> anyhow::Result<serde_json::Value> {
        self.get("/policies").await
    }

    /// Volumes, optionally filtered (`state`, `tenant`, `backend`, `kind`).
    pub async fn list_volumes(&self, query: &str) -> anyhow::Result<serde_json::Value> {
        let path = if query.is_empty() {
            "/volumes".to_string()
        } else {
            format!("/volumes?{query}")
        };
        self.get(&path).await
    }

    pub async fn get_volume(&self, volume_id: &str) -> anyhow::Result<AtlasVolume> {
        self.get_as(&format!("/volumes/{}", Self::seg(volume_id)))
            .await
    }

    /// Volumes owned by a given product (+ optional resource id).
    pub async fn list_volumes_by_owner(
        &self,
        product: &str,
        resource_id: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        let mut q = format!("owner_product={}", Self::seg(product));
        if let Some(rid) = resource_id {
            q.push_str(&format!("&owner_resource_id={}", Self::seg(rid)));
        }
        self.list_volumes(&q).await
    }

    // ---- Volume write path (async jobs) -----------------------------------

    /// Create a backend volume. Returns the enqueued job (`202`).
    #[allow(clippy::too_many_arguments)]
    pub async fn create_volume(
        &self,
        tenant_id: &str,
        name: &str,
        size_bytes: i64,
        policy: &str,
        owner: Option<&AtlasOwner>,
        storage_class: Option<&str>,
        namespace: Option<&str>,
    ) -> anyhow::Result<AtlasJob> {
        let mut body = serde_json::json!({
            "tenant_id": tenant_id,
            "name": name,
            "size_bytes": size_bytes,
            "kind": "block",
            "policy": policy,
        });
        if let Some(o) = owner {
            body["owner"] = serde_json::json!({
                "product": o.product,
                "resource_type": o.resource_type,
                "resource_id": o.resource_id,
                "role": o.role,
            });
        }
        let mut k8s = serde_json::json!({ "create_pvc": true });
        if let Some(ns) = namespace {
            k8s["namespace"] = serde_json::json!(ns);
        }
        if let Some(sc) = storage_class {
            k8s["storage_class"] = serde_json::json!(sc);
        }
        body["kubernetes"] = k8s;
        let v = self.post("/volumes", body).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn delete_volume(&self, volume_id: &str) -> anyhow::Result<serde_json::Value> {
        self.delete(&format!("/volumes/{}", Self::seg(volume_id)))
            .await
    }

    pub async fn expand_volume(
        &self,
        volume_id: &str,
        new_size_bytes: i64,
    ) -> anyhow::Result<AtlasJob> {
        let v = self
            .post(
                &format!("/volumes/{}/expand", Self::seg(volume_id)),
                serde_json::json!({ "new_size_bytes": new_size_bytes }),
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    // ---- Snapshots / clone / restore --------------------------------------

    pub async fn snapshot_volume(
        &self,
        volume_id: &str,
        name: Option<&str>,
    ) -> anyhow::Result<AtlasJob> {
        let mut body = serde_json::json!({});
        if let Some(n) = name {
            body["name"] = serde_json::json!(n);
        }
        let v = self
            .post(
                &format!("/volumes/{}/snapshots", Self::seg(volume_id)),
                body,
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn list_snapshots(&self) -> anyhow::Result<serde_json::Value> {
        self.get("/snapshots").await
    }

    pub async fn clone_snapshot(
        &self,
        snapshot_id: &str,
        name: &str,
        namespace: Option<&str>,
    ) -> anyhow::Result<AtlasJob> {
        let mut body = serde_json::json!({ "name": name });
        if let Some(ns) = namespace {
            body["namespace"] = serde_json::json!(ns);
        }
        let v = self
            .post(
                &format!("/snapshots/{}/clone", Self::seg(snapshot_id)),
                body,
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn restore_snapshot(
        &self,
        snapshot_id: &str,
        name: Option<&str>,
        namespace: Option<&str>,
    ) -> anyhow::Result<AtlasJob> {
        let mut body = serde_json::json!({});
        if let Some(n) = name {
            body["name"] = serde_json::json!(n);
        }
        if let Some(ns) = namespace {
            body["namespace"] = serde_json::json!(ns);
        }
        let v = self
            .post(
                &format!("/snapshots/{}/restore", Self::seg(snapshot_id)),
                body,
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn delete_snapshot(
        &self,
        snapshot_id: &str,
        force: bool,
    ) -> anyhow::Result<serde_json::Value> {
        let seg = Self::seg(snapshot_id);
        let path = if force {
            format!("/snapshots/{seg}?force=true")
        } else {
            format!("/snapshots/{seg}")
        };
        self.delete(&path).await
    }

    // ---- Object storage + backups -----------------------------------------

    pub async fn list_buckets(&self) -> anyhow::Result<serde_json::Value> {
        self.get("/buckets").await
    }

    pub async fn create_bucket(
        &self,
        name: &str,
        namespace: Option<&str>,
    ) -> anyhow::Result<AtlasJob> {
        let mut body = serde_json::json!({ "name": name });
        if let Some(ns) = namespace {
            body["namespace"] = serde_json::json!(ns);
        }
        let v = self.post("/buckets", body).await?;
        Ok(serde_json::from_value(v)?)
    }

    /// Snapshot a volume and write a backup to a bound bucket. `mode` is
    /// `manifest` (metadata only) or `data` (real RBD `export-diff` to S3).
    pub async fn backup_volume(
        &self,
        volume_id: &str,
        bucket_id: &str,
        mode: &str,
        keep: i64,
    ) -> anyhow::Result<AtlasJob> {
        let v = self
            .post(
                "/backup-jobs",
                serde_json::json!({
                    "volume_id": volume_id,
                    "bucket_id": bucket_id,
                    "mode": mode,
                    "keep": keep,
                }),
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    /// List backups, optionally scoped to one volume.
    pub async fn list_backups(&self, volume_id: Option<&str>) -> anyhow::Result<serde_json::Value> {
        let path = match volume_id {
            Some(id) => format!("/backups?volume_id={}", Self::seg(id)),
            None => "/backups".to_string(),
        };
        self.get(&path).await
    }

    pub async fn restore_backup(
        &self,
        backup_id: &str,
        name: Option<&str>,
        mode: &str,
    ) -> anyhow::Result<AtlasJob> {
        let mut body = serde_json::json!({ "backup_id": backup_id, "mode": mode });
        if let Some(n) = name {
            body["name"] = serde_json::json!(n);
        }
        let v = self.post("/restore-jobs", body).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn delete_backup(&self, backup_id: &str) -> anyhow::Result<serde_json::Value> {
        self.delete(&format!("/backups/{}", Self::seg(backup_id)))
            .await
    }

    // ---- Jobs -------------------------------------------------------------

    pub async fn list_jobs(&self) -> anyhow::Result<serde_json::Value> {
        self.get("/jobs").await
    }

    pub async fn get_job(&self, job_id: &str) -> anyhow::Result<AtlasJob> {
        self.get_as(&format!("/jobs/{}", Self::seg(job_id))).await
    }

    /// Poll a job until it reaches a terminal state or the deadline elapses.
    /// Returns the last observed job. Used by VM orchestration that needs the
    /// created `volume_id` before continuing (create → attach).
    pub async fn wait_for_job(&self, job_id: &str, max_wait: Duration) -> anyhow::Result<AtlasJob> {
        let start = std::time::Instant::now();
        let mut last = self.get_job(job_id).await?;
        while !last.is_terminal() {
            if start.elapsed() >= max_wait {
                break;
            }
            tokio::time::sleep(Duration::from_millis(750)).await;
            last = self.get_job(job_id).await?;
        }
        Ok(last)
    }
}

/// Reachability + version probe for the platform Storage page. Never errors —
/// unreachable Atlas is reported as `reachable: false` with a summary.
pub async fn status(cfg: &ControllerConfig) -> AtlasStatus {
    let base_url = cfg.atlas_base_url.trim_end_matches('/').to_string();
    if !cfg.atlas_enabled {
        return AtlasStatus {
            enabled: false,
            base_url,
            reachable: false,
            authenticated: false,
            version: None,
            tenant_id: cfg.atlas_tenant_id.clone(),
            default_policy: cfg.atlas_default_policy.clone(),
            backup_bucket_id: cfg.atlas_backup_bucket_id.clone(),
            summary: "Atlas storage integration disabled — set ATLAS_ENABLED=1".into(),
        };
    }

    let client = match AtlasClient::build(cfg) {
        Ok(c) => c,
        Err(e) => {
            return AtlasStatus {
                enabled: true,
                base_url,
                reachable: false,
                authenticated: false,
                version: None,
                tenant_id: cfg.atlas_tenant_id.clone(),
                default_policy: cfg.atlas_default_policy.clone(),
                backup_bucket_id: cfg.atlas_backup_bucket_id.clone(),
                summary: format!("Atlas client init failed: {e}"),
            };
        }
    };

    let (reachable, version) = match client.version().await {
        Ok(v) => (
            true,
            v.get("version")
                .and_then(|x| x.as_str())
                .map(str::to_string),
        ),
        Err(_) => (false, None),
    };
    let authenticated = reachable && cfg.atlas_token.is_some();
    let summary = if reachable {
        format!(
            "Atlas gateway reachable at {base_url}{}",
            if cfg.atlas_token.is_some() {
                " · authenticated"
            } else {
                " · unauthenticated (dev)"
            }
        )
    } else {
        format!("Atlas gateway unreachable at {base_url}")
    };

    AtlasStatus {
        enabled: true,
        base_url,
        reachable,
        authenticated,
        version,
        tenant_id: cfg.atlas_tenant_id.clone(),
        default_policy: cfg.atlas_default_policy.clone(),
        backup_bucket_id: cfg.atlas_backup_bucket_id.clone(),
        summary,
    }
}

/// Build a client or fail with a user-facing message when disabled.
pub fn require_client(cfg: &ControllerConfig) -> anyhow::Result<AtlasClient> {
    AtlasClient::from_config(cfg)?
        .ok_or_else(|| anyhow::anyhow!("Atlas storage integration disabled (ATLAS_ENABLED=0)"))
}

/// Prometheus gauges for the Atlas pools this controller is attached to, appended to
/// `/api/v1/metrics/prometheus` so a decision engine (Zyntra) or Grafana reads storage
/// headroom from one scrape. Empty when the integration is off; bounded to a few seconds
/// so a dead Atlas cannot stall the whole scrape.
pub async fn prometheus_gauges(cfg: &ControllerConfig) -> String {
    let Ok(Some(client)) = AtlasClient::from_config(cfg) else {
        return String::new();
    };
    let probe = async {
        let up = client.version().await.is_ok();
        let mut out = format!(
            "# HELP machina_atlas_up Atlas gateway reachable (1) or not (0)\n\
             # TYPE machina_atlas_up gauge\n\
             machina_atlas_up {}\n",
            u8::from(up)
        );
        if !up {
            return out;
        }
        if let Ok(f) = client.forecast(1440).await {
            let num = |k: &str| f.get(k).and_then(|v| v.as_f64());
            for (key, name, help) in [
                (
                    "raw_capacity_bytes",
                    "machina_atlas_capacity_raw_bytes",
                    "Atlas raw capacity (bytes)",
                ),
                (
                    "used_capacity_bytes",
                    "machina_atlas_capacity_used_bytes",
                    "Atlas used capacity (bytes)",
                ),
                (
                    "growth_bytes_per_day",
                    "machina_atlas_growth_bytes_per_day",
                    "Atlas growth rate (bytes/day)",
                ),
                (
                    "days_to_full",
                    "machina_atlas_days_to_full",
                    "Projected days until Atlas capacity is full (absent when not growing)",
                ),
            ] {
                if let Some(v) = num(key) {
                    out.push_str(&format!(
                        "# HELP {name} {help}\n# TYPE {name} gauge\n{name} {v}\n"
                    ));
                }
            }
        }
        if let Ok(alerts) = client.list_alerts(Some("open")).await {
            let n = alerts.as_array().map(|a| a.len()).unwrap_or(0);
            out.push_str(&format!(
                "# HELP machina_atlas_alerts_open Open Atlas alerts\n\
                 # TYPE machina_atlas_alerts_open gauge\n\
                 machina_atlas_alerts_open {n}\n"
            ));
        }
        out
    };
    tokio::time::timeout(Duration::from_secs(4), probe)
        .await
        .unwrap_or_default()
}
