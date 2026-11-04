// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Async REST client for `fluxvm-api` (`/v1/vms…`). FluxVM is UUID-keyed;
//! Machina is name-keyed, so every per-VM call goes through [`FluxvmClient::resolve`].

use std::time::Duration;

use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::types::{
    FluxCdrom, FluxCreate, FluxList, FluxMetrics, FluxReceiver, FluxRecord, FluxSnapshot,
    FLUXVM_HYPERVISORS, MIGRATING_FROM_LABEL,
};
use crate::{CreateVmRequest, FluxvmConfig, LibvirtError, VmMetrics};

#[derive(Clone)]
pub struct FluxvmClient {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
    default_backend: String,
}

fn read_token(cfg: &FluxvmConfig) -> Option<String> {
    let from_file = cfg.token_file.trim();
    let tok = if from_file.is_empty() {
        cfg.token.trim().to_string()
    } else {
        std::fs::read_to_string(from_file)
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    Some(tok).filter(|t| !t.is_empty())
}

impl FluxvmClient {
    /// Build a client from `[fluxvm]`; `Forbidden` when the backend is disabled.
    pub fn from_config(cfg: &FluxvmConfig) -> Result<Self, LibvirtError> {
        if !cfg.enabled {
            return Err(LibvirtError::Forbidden(
                "FluxVM backend is disabled ([fluxvm] enabled = false)".into(),
            ));
        }
        let base = cfg.base_url.trim().trim_end_matches('/').to_string();
        if base.is_empty() {
            return Err(LibvirtError::Invalid("fluxvm.base_url is empty".into()));
        }
        let mut b = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(300));
        if cfg.insecure_tls {
            b = b.danger_accept_invalid_certs(true);
        }
        let http = b
            .build()
            .map_err(|e| LibvirtError::Internal(format!("fluxvm http client: {e}")))?;
        Ok(Self {
            http,
            base,
            token: read_token(cfg),
            default_backend: cfg.default_backend.trim().to_string(),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    fn ws_base(&self) -> String {
        if let Some(rest) = self.base.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if let Some(rest) = self.base.strip_prefix("http://") {
            format!("ws://{rest}")
        } else {
            self.base.clone()
        }
    }

    /// `ws(s)://…/v1/vms/{id}/serial` plus the bearer token for the upgrade request.
    /// Interactive on QEMU; a read-only `console.log` stream on the other engines.
    pub fn serial_ws(&self, id: &str) -> (String, Option<String>) {
        (
            format!("{}/v1/vms/{id}/serial", self.ws_base()),
            self.token.clone(),
        )
    }

    /// `ws(s)://…/v1/vms/{id}/console` (guest-agent PTY) plus the bearer token.
    pub fn console_ws(&self, id: &str, cols: u16, rows: u16) -> (String, Option<String>) {
        (
            format!(
                "{}/v1/vms/{id}/console?cols={cols}&rows={rows}",
                self.ws_base()
            ),
            self.token.clone(),
        )
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<(StatusCode, String), LibvirtError> {
        let url = format!("{}{}", self.base, path);
        let mut req = self.http.request(method.clone(), &url);
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.map_err(|e| {
            LibvirtError::Connection(format!("fluxvm-api {method} {path} unreachable: {e}"))
        })?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if status.is_success() {
            return Ok((status, text));
        }
        Err(error_from_status(status, path, &text))
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T, LibvirtError> {
        let (_, text) = self.send(method, path, body).await?;
        let text = if text.trim().is_empty() {
            "null"
        } else {
            &text
        };
        serde_json::from_str(text)
            .map_err(|e| LibvirtError::Internal(format!("fluxvm-api {path}: bad JSON: {e}")))
    }

    pub async fn health(&self) -> Result<Value, LibvirtError> {
        self.call(Method::GET, "/healthz", None).await
    }

    pub async fn capabilities(&self) -> Result<Value, LibvirtError> {
        self.call(Method::GET, "/v1/runtime/capabilities", None)
            .await
    }

    /// Every VM except not-yet-adopted migration receivers.
    pub async fn list(&self) -> Result<Vec<FluxRecord>, LibvirtError> {
        let l: FluxList = self.call(Method::GET, "/v1/vms", None).await?;
        Ok(l.items
            .into_iter()
            .filter(|r| !r.is_incoming_migration())
            .collect())
    }

    /// Full records as FluxVM serialises them, receivers excluded.
    pub async fn list_raw(&self) -> Result<Vec<Value>, LibvirtError> {
        #[derive(serde::Deserialize)]
        struct Items {
            #[serde(default)]
            items: Vec<Value>,
        }
        let l: Items = self.call(Method::GET, "/v1/vms", None).await?;
        Ok(l.items
            .into_iter()
            .filter(|v| {
                v.get("labels")
                    .and_then(|l| l.get(MIGRATING_FROM_LABEL))
                    .is_none()
            })
            .collect())
    }

    /// Name → record, via the server-side `?name=` filter. A migration receiver
    /// carries the source's name until it is adopted, so it is skipped.
    pub async fn resolve(&self, name: &str) -> Result<FluxRecord, LibvirtError> {
        let path = format!("/v1/vms?name={}", urlencode(name));
        let l: FluxList = self.call(Method::GET, &path, None).await?;
        l.items
            .into_iter()
            .find(|r| r.name == name && !r.is_incoming_migration())
            .ok_or_else(|| LibvirtError::NotFound(format!("FluxVM VM '{name}' not found")))
    }

    pub async fn get_by_id(&self, id: &str) -> Result<FluxRecord, LibvirtError> {
        self.call(Method::GET, &format!("/v1/vms/{id}"), None).await
    }

    /// The full FluxVM record as FluxVM serialises it (the source `record` a
    /// migration receiver is launched from, or the request HA re-creates).
    pub async fn export_record(&self, name: &str) -> Result<Value, LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.call(Method::GET, &format!("/v1/vms/{id}"), None).await
    }

    pub async fn get(&self, name: &str) -> Result<FluxRecord, LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.call(Method::GET, &format!("/v1/vms/{id}"), None).await
    }

    /// `start` / `stop` / `pause` / `resume` / `restart`.
    pub async fn action(&self, name: &str, verb: &str) -> Result<(), LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.send(Method::POST, &format!("/v1/vms/{id}/{verb}"), None)
            .await
            .map(|_| ())
    }

    pub async fn delete(&self, name: &str) -> Result<(), LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.send(Method::DELETE, &format!("/v1/vms/{id}"), None)
            .await
            .map(|_| ())
    }

    pub async fn metrics(&self, name: &str) -> Result<VmMetrics, LibvirtError> {
        let rec = self.resolve(name).await?;
        let m: FluxMetrics = self
            .call(Method::GET, &format!("/v1/vms/{}/stats", rec.id), None)
            .await?;
        Ok(m.to_vm_metrics(&rec))
    }

    pub async fn create(&self, req: &CreateVmRequest) -> Result<FluxRecord, LibvirtError> {
        let body = self.build_create(req)?;
        let body = serde_json::to_value(body)
            .map_err(|e| LibvirtError::Internal(format!("fluxvm create body: {e}")))?;
        self.call(Method::POST, "/v1/vms", Some(body)).await
    }

    /// `POST /v1/vms` with a FluxVM-native body (e.g. a stored record's
    /// `request`). `shared_takeover` breaks a `storage: shared` disk lock first;
    /// only for a caller that has fenced the previous holder's host.
    pub async fn create_raw(
        &self,
        body: Value,
        shared_takeover: bool,
    ) -> Result<FluxRecord, LibvirtError> {
        let path = if shared_takeover {
            "/v1/vms?shared_takeover=true"
        } else {
            "/v1/vms"
        };
        self.call(Method::POST, path, Some(body)).await
    }

    // --- snapshots (every engine; running or paused) ---

    pub async fn snapshots(&self, name: &str) -> Result<Vec<FluxSnapshot>, LibvirtError> {
        let id = self.resolve(name).await?.id;
        #[derive(serde::Deserialize)]
        struct Items {
            #[serde(default)]
            items: Vec<FluxSnapshot>,
        }
        let l: Items = self
            .call(Method::GET, &format!("/v1/vms/{id}/snapshots"), None)
            .await?;
        Ok(l.items)
    }

    pub async fn snapshot(&self, name: &str, tag: &str) -> Result<(), LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.send(
            Method::POST,
            &format!("/v1/vms/{id}/snapshot"),
            Some(json!({ "tag": tag })),
        )
        .await
        .map(|_| ())
    }

    /// A stopped VM relaunches from the tag; a running flux-vm restores in
    /// place; other running engines must be stopped first (FluxVM answers 409).
    pub async fn restore(&self, name: &str, tag: &str) -> Result<(), LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.send(
            Method::POST,
            &format!("/v1/vms/{id}/restore"),
            Some(json!({ "tag": tag })),
        )
        .await
        .map(|_| ())
    }

    pub async fn delete_snapshot(&self, name: &str, tag: &str) -> Result<(), LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.send(
            Method::DELETE,
            &format!("/v1/vms/{id}/snapshots/{}", urlencode(tag)),
            None,
        )
        .await
        .map(|_| ())
    }

    // --- install media (QEMU) ---

    /// Removes the medium from CD-ROM `cdrom` (live when running). The empty drive
    /// stays; ejecting an empty drive is a no-op.
    pub async fn eject_cdrom(&self, name: &str, cdrom: &str) -> Result<FluxRecord, LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.call(
            Method::POST,
            &format!("/v1/vms/{id}/cdroms/{}/eject", urlencode(cdrom)),
            None,
        )
        .await
    }

    // --- backups (default or shared storage; a running VM needs QEMU) ---

    pub async fn backup(
        &self,
        name: &str,
        backup_name: Option<&str>,
        compress: bool,
    ) -> Result<Value, LibvirtError> {
        let id = self.resolve(name).await?.id;
        let mut body = json!({ "compress": compress });
        if let Some(n) = backup_name.filter(|n| !n.is_empty()) {
            body["name"] = json!(n);
        }
        self.call(Method::POST, &format!("/v1/vms/{id}/backup"), Some(body))
            .await
    }

    /// FluxVM backups, newest first; `vm` keeps only that VM's (by `vm_name`).
    pub async fn backups(&self, vm: Option<&str>) -> Result<Vec<Value>, LibvirtError> {
        #[derive(serde::Deserialize)]
        struct Items {
            #[serde(default)]
            items: Vec<Value>,
        }
        let l: Items = self.call(Method::GET, "/v1/backups", None).await?;
        Ok(l.items
            .into_iter()
            .filter(|b| vm.is_none_or(|v| b.get("vm_name").and_then(Value::as_str) == Some(v)))
            .collect())
    }

    /// The VM must be stopped.
    pub async fn restore_backup(&self, name: &str, backup: &str) -> Result<Value, LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.call(
            Method::POST,
            &format!("/v1/vms/{id}/restore-backup"),
            Some(json!({ "name": backup })),
        )
        .await
    }

    pub async fn delete_backup(&self, backup: &str) -> Result<(), LibvirtError> {
        self.send(
            Method::DELETE,
            &format!("/v1/backups/{}", urlencode(backup)),
            None,
        )
        .await
        .map(|_| ())
    }

    // --- hotplug ---

    /// QEMU and Cloud Hypervisor. Returns the realized vCPU count.
    pub async fn hotplug_cpu(&self, name: &str, add_vcpus: u32) -> Result<u32, LibvirtError> {
        let id = self.resolve(name).await?.id;
        let v: Value = self
            .call(
                Method::POST,
                &format!("/v1/vms/{id}/hotplug/cpu"),
                Some(json!({ "add_vcpus": add_vcpus.min(u8::MAX as u32) })),
            )
            .await?;
        Ok(v.get("vcpus").and_then(Value::as_u64).unwrap_or(0) as u32)
    }

    /// QEMU and Cloud Hypervisor. Returns the new total memory in MiB.
    pub async fn hotplug_memory(
        &self,
        name: &str,
        add_memory_mib: u64,
    ) -> Result<u64, LibvirtError> {
        let id = self.resolve(name).await?.id;
        let v: Value = self
            .call(
                Method::POST,
                &format!("/v1/vms/{id}/hotplug/memory"),
                Some(json!({ "add_memory_mib": add_memory_mib })),
            )
            .await?;
        Ok(v.get("memory_mib").and_then(Value::as_u64).unwrap_or(0))
    }

    /// QEMU: a virtio NIC on a tap enslaved to `bridge`. Returns its MAC (one
    /// is generated when none is given, so the NIC can be unplugged by MAC).
    pub async fn hotplug_nic(
        &self,
        name: &str,
        bridge: &str,
        mac: Option<&str>,
    ) -> Result<String, LibvirtError> {
        let id = self.resolve(name).await?.id;
        let mac = mac
            .filter(|m| !m.is_empty())
            .map(str::to_string)
            .unwrap_or_else(random_mac);
        self.send(
            Method::POST,
            &format!("/v1/vms/{id}/hotplug/nic"),
            Some(json!({ "bridge": bridge, "mac": mac })),
        )
        .await?;
        Ok(mac)
    }

    pub async fn unplug_nic(&self, name: &str, mac: &str) -> Result<(), LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.send(
            Method::POST,
            &format!("/v1/vms/{id}/hotplug/nic/unplug"),
            Some(json!({ "mac": mac })),
        )
        .await
        .map(|_| ())
    }

    // --- live migration (QEMU on shared storage; ids, not names: during a
    // migration two records share the name) ---

    pub async fn migration_start(
        &self,
        id: &str,
        destination: &str,
        bandwidth_mbps: Option<u64>,
        max_downtime_ms: Option<u64>,
    ) -> Result<Value, LibvirtError> {
        let mut body = json!({ "destination": destination });
        if let Some(b) = bandwidth_mbps.filter(|b| *b > 0) {
            body["bandwidth_mbps"] = json!(b);
        }
        if let Some(d) = max_downtime_ms.filter(|d| *d > 0) {
            body["max_downtime_ms"] = json!(d);
        }
        self.call(
            Method::POST,
            &format!("/v1/vms/{id}/migration/start"),
            Some(body),
        )
        .await
    }

    pub async fn migration_status(&self, id: &str) -> Result<Value, LibvirtError> {
        self.call(Method::GET, &format!("/v1/vms/{id}/migration/status"), None)
            .await
    }

    pub async fn migration_cancel(&self, id: &str) -> Result<Value, LibvirtError> {
        self.call(
            Method::POST,
            &format!("/v1/vms/{id}/migration/cancel"),
            None,
        )
        .await
    }

    /// Source side once the phase is `completed`: removes the paused source.
    pub async fn migration_finish(&self, id: &str) -> Result<(), LibvirtError> {
        self.send(
            Method::POST,
            &format!("/v1/vms/{id}/migration/finish"),
            None,
        )
        .await
        .map(|_| ())
    }

    /// Target side: an adopt-mode receiver launched from the source `record`.
    pub async fn receiver_create(
        &self,
        record: Value,
        listen_host: &str,
        advertise_host: &str,
    ) -> Result<FluxReceiver, LibvirtError> {
        let mut body = json!({ "record": record, "listen_host": listen_host, "listen_port": 0 });
        if !advertise_host.is_empty() {
            body["advertise_host"] = json!(advertise_host);
        }
        self.call(Method::POST, "/v1/migration/receivers", Some(body))
            .await
    }

    pub async fn receiver_activate(&self, id: &str, token: &str) -> Result<(), LibvirtError> {
        self.send(
            Method::POST,
            &format!("/v1/migration/receivers/{id}/activate"),
            Some(json!({ "token": token })),
        )
        .await
        .map(|_| ())
    }

    pub async fn receiver_adopt(&self, id: &str, token: &str) -> Result<FluxRecord, LibvirtError> {
        self.call(
            Method::POST,
            &format!("/v1/migration/receivers/{id}/adopt"),
            Some(json!({ "token": token })),
        )
        .await
    }

    pub async fn receiver_delete(&self, id: &str) -> Result<(), LibvirtError> {
        self.send(
            Method::DELETE,
            &format!("/v1/migration/receivers/{id}"),
            None,
        )
        .await
        .map(|_| ())
    }

    /// Last `lines` lines of the VM's console log.
    pub async fn logs(&self, name: &str, lines: usize) -> Result<String, LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.send(
            Method::GET,
            &format!("/v1/vms/{id}/logs?lines={}", lines.max(1)),
            None,
        )
        .await
        .map(|(_, text)| text)
    }

    /// Mint a single-use browser WebSocket ticket (`kind`: `vnc` | `serial` | `console`).
    /// Returns the redeem path `/console/ws/{ticket}` (no Authorization on the upgrade).
    pub async fn ws_ticket(&self, name: &str, kind: &str) -> Result<String, LibvirtError> {
        let id = self.resolve(name).await?.id;
        let v: Value = self
            .call(
                Method::POST,
                &format!("/v1/vms/{id}/ws-ticket"),
                Some(json!({ "kind": kind })),
            )
            .await?;
        v.get("path")
            .and_then(Value::as_str)
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                LibvirtError::Internal("fluxvm-api ws-ticket response missing path".into())
            })
    }

    /// Full `ws(s)://…/console/ws/{ticket}` URL for a redeem path from [`Self::ws_ticket`].
    pub fn ticket_ws_url(&self, path: &str) -> String {
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        format!("{}{path}", self.ws_base())
    }

    /// Clone a stopped VM (`POST …/clone`). `clone_mode` is ignored (FluxVM always copies).
    pub async fn clone_vm(&self, name: &str, new_name: &str) -> Result<FluxRecord, LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.call(
            Method::POST,
            &format!("/v1/vms/{id}/clone"),
            Some(json!({ "name": new_name })),
        )
        .await
    }

    /// Rename via `PATCH /v1/vms/{id}` `{ "name": … }`.
    pub async fn rename(&self, name: &str, new_name: &str) -> Result<FluxRecord, LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.call(
            Method::PATCH,
            &format!("/v1/vms/{id}"),
            Some(json!({ "name": new_name })),
        )
        .await
    }

    /// List data disks (QEMU). Root is reported separately on the VM record.
    pub async fn disks(&self, name: &str) -> Result<Vec<Value>, LibvirtError> {
        let id = self.resolve(name).await?.id;
        let v: Value = self
            .call(Method::GET, &format!("/v1/vms/{id}/disks"), None)
            .await?;
        if let Some(items) = v.get("items").and_then(Value::as_array) {
            return Ok(items.clone());
        }
        Ok(v.as_array().cloned().unwrap_or_default())
    }

    /// Attach a blank sized disk (`size_gib`) or an existing `path`. `disk_name` is the FluxVM disk name.
    pub async fn attach_disk(
        &self,
        name: &str,
        disk_name: &str,
        path: Option<&str>,
        size_gib: Option<u64>,
    ) -> Result<Value, LibvirtError> {
        let id = self.resolve(name).await?.id;
        let mut body = json!({ "name": disk_name });
        if let Some(p) = path.filter(|p| !p.is_empty()) {
            body["path"] = json!(p);
        } else if let Some(g) = size_gib.filter(|g| *g > 0) {
            body["size_gib"] = json!(g);
        } else {
            return Err(LibvirtError::Invalid(
                "FluxVM disk attach needs source (path) or size_gb > 0".into(),
            ));
        }
        self.call(Method::POST, &format!("/v1/vms/{id}/disks"), Some(body))
            .await
    }

    pub async fn detach_disk(&self, name: &str, disk_name: &str) -> Result<(), LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.send(
            Method::DELETE,
            &format!("/v1/vms/{id}/disks/{}", urlencode(disk_name)),
            None,
        )
        .await
        .map(|_| ())
    }

    pub async fn resize_disk(
        &self,
        name: &str,
        disk_name: &str,
        size_gib: u64,
    ) -> Result<Value, LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.call(
            Method::PATCH,
            &format!("/v1/vms/{id}/disks/{}", urlencode(disk_name)),
            Some(json!({ "size_gib": size_gib })),
        )
        .await
    }

    /// Set balloon target MiB (`0` deflates). Needs a balloon device (create with balloon).
    pub async fn balloon_set(&self, name: &str, balloon_mib: u64) -> Result<Value, LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.call(
            Method::POST,
            &format!("/v1/vms/{id}/balloon"),
            Some(json!({ "balloon_mib": balloon_mib })),
        )
        .await
    }

    pub async fn balloon_get(&self, name: &str) -> Result<Value, LibvirtError> {
        let id = self.resolve(name).await?.id;
        self.call(Method::GET, &format!("/v1/vms/{id}/balloon"), None)
            .await
    }

    pub fn build_create(&self, req: &CreateVmRequest) -> Result<FluxCreate, LibvirtError> {
        let hv = match req.fluxvm_backend.trim() {
            "" => self.default_backend.as_str(),
            other => other,
        };
        let hv = if hv.is_empty() { "auto" } else { hv };
        if !FLUXVM_HYPERVISORS.contains(&hv) {
            return Err(LibvirtError::Invalid(format!(
                "fluxvm_backend '{hv}' is not one of {}",
                FLUXVM_HYPERVISORS.join(", ")
            )));
        }
        let image = match req.fluxvm_image.trim() {
            "" => req.existing_disk.trim(),
            other => other,
        };
        if image.is_empty() {
            return Err(LibvirtError::Invalid(
                "FluxVM create needs a base image (fluxvm_image or existing_disk)".into(),
            ));
        }
        let network = build_network(req, hv)?;
        let cdroms = build_cdroms(&req.fluxvm_isos)?;
        // `auto` may resolve to flux-vm, which has no user-mode NAT or CD-ROMs: pin QEMU.
        let hv = if hv == "auto" && (network["mode"] == "user" || !cdroms.is_empty()) {
            "qemu"
        } else {
            hv
        };
        if !cdroms.is_empty() && hv != "qemu" {
            return Err(LibvirtError::Invalid(format!(
                "fluxvm_isos need fluxvm_backend qemu, not '{hv}'"
            )));
        }
        let opt = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
        let user = req.cloud_init_user.trim();
        let key = req.cloud_init_ssh_pubkey.trim();
        // Always send a NoCloud seed: without one cloud-init never runs its default
        // network config and the guest's DHCP client never comes up.
        let cloud_init = Some(json!({
            "hostname": req.name,
            "user": Some(user).filter(|u| !u.is_empty()),
            "ssh_authorized_keys": if key.is_empty() { vec![] } else { vec![key.to_string()] },
        }));
        let vcpus = req.vcpus.clamp(1, u8::MAX as u32);
        let firmware = opt(&req.fluxvm_firmware);
        let balloon = req.fluxvm_balloon.then(|| {
            json!({
                "free_page_reporting": true,
                "deflate_on_oom": true,
            })
        });
        Ok(FluxCreate {
            name: req.name.clone(),
            backend: hv.to_string(),
            image: image.to_string(),
            vcpus,
            memory_mib: req.memory_mb,
            disk_size_gib: Some(req.disk_gb).filter(|g| *g > 0),
            network,
            cloud_init,
            kernel: opt(&req.fluxvm_kernel),
            initrd: opt(&req.fluxvm_initrd),
            kernel_args: opt(&req.fluxvm_kernel_args),
            agent: Some(json!({ "enabled": req.fluxvm_agent.unwrap_or(true) })),
            storage: req.fluxvm_shared_disk.then(|| "shared".to_string()),
            cdroms,
            tpm: req.vtpm.then_some(true),
            secure_boot: req.secure_boot.then_some(true),
            firmware,
            balloon,
        })
    }
}

/// FluxVM CD-ROM drive names for `fluxvm_isos`, in order.
const CDROM_NAMES: [&str; 4] = ["install", "cd2", "cd3", "cd4"];

fn build_cdroms(isos: &[String]) -> Result<Vec<FluxCdrom>, LibvirtError> {
    let isos: Vec<&str> = isos
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    if isos.len() > CDROM_NAMES.len() {
        return Err(LibvirtError::Invalid(format!(
            "at most {} fluxvm_isos",
            CDROM_NAMES.len()
        )));
    }
    Ok(isos
        .into_iter()
        .zip(CDROM_NAMES)
        .map(|(path, name)| FluxCdrom {
            name: name.into(),
            path: path.into(),
        })
        .collect())
}

/// Every mode except `user`/`none` gives the guest a host-visible tap, which is what
/// FluxVM's eBPF VM edge (TCX policy, identity, accounting) attaches to.
fn build_network(req: &CreateVmRequest, hv: &str) -> Result<Value, LibvirtError> {
    let invalid = |m: String| Err(LibvirtError::Invalid(m));
    let bridge = req.fluxvm_bridge.trim();
    let uplink = req.fluxvm_direct_uplink.trim();
    let mode = req.fluxvm_direct_mode.trim();
    let guest_ips = &req.fluxvm_direct_guest_ips;
    let net = req.fluxvm_network.trim();

    if uplink.is_empty() {
        if !mode.is_empty() || !guest_ips.is_empty() {
            return invalid(
                "fluxvm_direct_mode and fluxvm_direct_guest_ips require fluxvm_direct_uplink"
                    .into(),
            );
        }
    } else {
        if !bridge.is_empty() {
            return invalid("fluxvm_direct_uplink is mutually exclusive with fluxvm_bridge".into());
        }
        if !matches!(net, "" | "netns") {
            return invalid(format!(
                "fluxvm_direct_uplink cannot be combined with fluxvm_network '{net}'"
            ));
        }
        if !(1..=15).contains(&uplink.len()) || uplink.contains('/') || uplink.contains(' ') {
            return invalid(format!(
                "fluxvm_direct_uplink '{uplink}' must be an interface name (1-15 characters, no slash or space)"
            ));
        }
        let mode = if mode.is_empty() { "l2-uplink" } else { mode };
        if !matches!(mode, "l2-uplink" | "peer-veth") {
            return invalid(format!(
                "fluxvm_direct_mode must be 'l2-uplink' or 'peer-veth', got '{mode}'"
            ));
        }
        if mode != "l2-uplink" && !guest_ips.is_empty() {
            return invalid(
                "fluxvm_direct_guest_ips are only valid for fluxvm_direct_mode l2-uplink".into(),
            );
        }
        if guest_ips.len() > 8 {
            return invalid(format!(
                "fluxvm_direct_guest_ips accepts at most 8 addresses, got {}",
                guest_ips.len()
            ));
        }
        if let Some(bad) = guest_ips
            .iter()
            .find(|ip| ip.trim().parse::<std::net::Ipv4Addr>().is_err())
        {
            return invalid(format!(
                "fluxvm_direct_guest_ips entry '{bad}' is not an IPv4 address"
            ));
        }
        let ips: Vec<&str> = guest_ips.iter().map(|ip| ip.trim()).collect();
        return Ok(json!({
            "mode": "tap",
            "mac": random_mac(),
            "netns": false,
            "direct": {"outer": uplink, "mode": mode, "guest_ips": ips},
        }));
    }

    if !bridge.is_empty() {
        if !matches!(net, "" | "netns") {
            return invalid(format!(
                "fluxvm_bridge cannot be combined with fluxvm_network '{net}'"
            ));
        }
        return Ok(json!({"mode": "tap", "bridge": bridge, "mac": random_mac()}));
    }

    match net {
        // FluxVM never persists an auto-generated MAC, and netns networking refuses to
        // start without one.
        "" | "netns" => Ok(json!({"mode": "tap", "netns": true, "mac": random_mac()})),
        "user" if matches!(hv, "qemu" | "auto") => Ok(json!({"mode": "user"})),
        "user" => invalid(format!(
            "fluxvm_network 'user' is QEMU-only; '{hv}' needs netns, a bridge or a direct uplink"
        )),
        "none" => Ok(json!({"mode": "none"})),
        other => invalid(format!(
            "fluxvm_network must be 'netns', 'user' or 'none', got '{other}'"
        )),
    }
}

fn random_mac() -> String {
    let b: [u8; 3] = rand::random();
    format!("52:54:00:{:02x}:{:02x}:{:02x}", b[0], b[1], b[2])
}

fn error_from_status(status: StatusCode, path: &str, body: &str) -> LibvirtError {
    let msg = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| body.chars().take(300).collect());
    let msg = format!("fluxvm-api {path}: HTTP {}: {msg}", status.as_u16());
    match status {
        StatusCode::NOT_FOUND => LibvirtError::NotFound(msg),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => LibvirtError::Forbidden(msg),
        StatusCode::BAD_REQUEST | StatusCode::CONFLICT | StatusCode::UNPROCESSABLE_ENTITY => {
            LibvirtError::Invalid(msg)
        }
        _ => LibvirtError::Operation(msg),
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> FluxvmClient {
        FluxvmClient::from_config(&FluxvmConfig {
            enabled: true,
            base_url: "https://flux.local:7788/".into(),
            token: "t0k".into(),
            ..FluxvmConfig::default()
        })
        .unwrap()
    }

    #[test]
    fn disabled_config_is_forbidden() {
        assert!(matches!(
            FluxvmClient::from_config(&FluxvmConfig::default()),
            Err(LibvirtError::Forbidden(_))
        ));
    }

    #[test]
    fn serial_url_uses_ws_scheme() {
        let (url, tok) = client().serial_ws("abc");
        assert_eq!(url, "wss://flux.local:7788/v1/vms/abc/serial");
        assert_eq!(tok.as_deref(), Some("t0k"));
    }

    #[test]
    fn direct_uplink_network() {
        let c = client();
        let mut req = CreateVmRequest {
            name: "vm1".into(),
            fluxvm_image: "/images/base.raw".into(),
            fluxvm_direct_uplink: "eno1".into(),
            fluxvm_direct_guest_ips: vec!["10.0.0.9".into()],
            ..Default::default()
        };
        let n = c.build_create(&req).unwrap().network;
        assert_eq!(n["mode"], "tap");
        assert_eq!(n["netns"], false);
        assert_eq!(n["direct"]["outer"], "eno1");
        assert_eq!(n["direct"]["mode"], "l2-uplink");
        assert_eq!(n["direct"]["guest_ips"][0], "10.0.0.9");

        req.fluxvm_direct_mode = "peer-veth".into();
        assert!(c.build_create(&req).is_err());
        req.fluxvm_direct_guest_ips.clear();
        assert_eq!(
            c.build_create(&req).unwrap().network["direct"]["mode"],
            "peer-veth"
        );
        req.fluxvm_bridge = "br0".into();
        assert!(c.build_create(&req).is_err());
        req.fluxvm_bridge.clear();
        req.fluxvm_direct_uplink = "bad/name".into();
        assert!(c.build_create(&req).is_err());
        req.fluxvm_direct_uplink.clear();
        assert!(c.build_create(&req).is_err());
    }

    #[test]
    fn create_body_defaults_and_validation() {
        let c = client();
        let mut req = CreateVmRequest {
            name: "vm1".into(),
            vcpus: 2,
            memory_mb: 1024,
            disk_gb: 10,
            existing_disk: "/images/base.qcow2".into(),
            ..Default::default()
        };
        let b = c.build_create(&req).unwrap();
        assert_eq!(b.backend, "auto");
        assert_eq!(b.image, "/images/base.qcow2");
        assert_eq!(b.network["mode"], "tap");
        assert_eq!(b.network["netns"], true);
        assert!(b.network["mac"].as_str().unwrap().starts_with("52:54:00:"));
        assert_eq!(b.cloud_init.as_ref().unwrap()["hostname"], "vm1");

        req.fluxvm_backend = "firecracker".into();
        req.fluxvm_bridge = "vmbr0".into();
        req.cloud_init_ssh_pubkey = "ssh-ed25519 AAAA".into();
        let b = c.build_create(&req).unwrap();
        assert_eq!(b.backend, "firecracker");
        assert_eq!(b.network["bridge"], "vmbr0");
        assert_eq!(
            b.cloud_init.unwrap()["ssh_authorized_keys"][0],
            "ssh-ed25519 AAAA"
        );

        req.fluxvm_backend = "flux-vm".into();
        req.fluxvm_bridge.clear();
        req.fluxvm_network = "user".into();
        assert!(c.build_create(&req).is_err());
        req.fluxvm_network = "none".into();
        assert_eq!(c.build_create(&req).unwrap().network["mode"], "none");
        req.fluxvm_network.clear();

        req.fluxvm_backend = "xen".into();
        assert!(c.build_create(&req).is_err());
        req.fluxvm_backend.clear();
        req.existing_disk.clear();
        assert!(c.build_create(&req).is_err());
    }

    #[test]
    fn console_url_carries_the_terminal_size() {
        let (url, tok) = client().console_ws("abc", 120, 40);
        assert_eq!(
            url,
            "wss://flux.local:7788/v1/vms/abc/console?cols=120&rows=40"
        );
        assert_eq!(tok.as_deref(), Some("t0k"));
    }

    #[test]
    fn create_body_kernel_agent_and_shared_disk() {
        let c = client();
        let mut req = CreateVmRequest {
            name: "fc1".into(),
            fluxvm_backend: "firecracker".into(),
            fluxvm_image: "/srv/nfs/fc1.raw".into(),
            fluxvm_kernel: " /var/lib/fluxvm/kernels/vmlinux ".into(),
            fluxvm_kernel_args: "console=ttyS0".into(),
            fluxvm_shared_disk: true,
            ..Default::default()
        };
        let v = serde_json::to_value(c.build_create(&req).unwrap()).unwrap();
        assert_eq!(v["kernel"], "/var/lib/fluxvm/kernels/vmlinux");
        assert_eq!(v["kernel_args"], "console=ttyS0");
        assert!(v.get("initrd").is_none());
        assert_eq!(v["agent"]["enabled"], true);
        assert_eq!(v["storage"], "shared");

        req.fluxvm_agent = Some(false);
        req.fluxvm_shared_disk = false;
        req.fluxvm_kernel.clear();
        let v = serde_json::to_value(c.build_create(&req).unwrap()).unwrap();
        assert_eq!(v["agent"]["enabled"], false);
        assert!(v.get("storage").is_none());
        assert!(v.get("kernel").is_none());
    }

    #[test]
    fn isos_become_named_cdroms_on_qemu() {
        let c = client();
        let mut req = CreateVmRequest {
            name: "win".into(),
            fluxvm_image: "/images/blank.raw".into(),
            fluxvm_isos: vec![
                "/iso/win11.iso".into(),
                " ".into(),
                "/iso/virtio-win.iso".into(),
            ],
            ..Default::default()
        };
        let v = serde_json::to_value(c.build_create(&req).unwrap()).unwrap();
        assert_eq!(v["backend"], "qemu");
        assert_eq!(
            v["cdroms"],
            json!([{"name": "install", "path": "/iso/win11.iso"},
                   {"name": "cd2", "path": "/iso/virtio-win.iso"}])
        );

        req.fluxvm_backend = "firecracker".into();
        assert!(c.build_create(&req).is_err());
        req.fluxvm_backend = "qemu".into();
        req.fluxvm_isos = vec!["/a.iso".into(); 5];
        assert!(c.build_create(&req).is_err());
        req.fluxvm_isos.clear();
        let v = serde_json::to_value(c.build_create(&req).unwrap()).unwrap();
        assert!(v.get("cdroms").is_none());
    }

    #[test]
    fn user_networking_pins_qemu_instead_of_auto() {
        let c = client();
        let req = CreateVmRequest {
            name: "u1".into(),
            fluxvm_image: "/images/base.qcow2".into(),
            fluxvm_network: "user".into(),
            ..Default::default()
        };
        let b = c.build_create(&req).unwrap();
        assert_eq!(b.backend, "qemu");
        assert_eq!(b.network["mode"], "user");
    }

    #[test]
    fn create_body_passes_tpm_secure_boot_firmware_balloon() {
        let c = client();
        let req = CreateVmRequest {
            name: "win11".into(),
            fluxvm_backend: "qemu".into(),
            fluxvm_image: "/images/blank.raw".into(),
            secure_boot: true,
            vtpm: true,
            fluxvm_firmware: "/usr/share/OVMF/OVMF_CODE.fd".into(),
            fluxvm_balloon: true,
            ..Default::default()
        };
        let v = serde_json::to_value(c.build_create(&req).unwrap()).unwrap();
        assert_eq!(v["tpm"], true);
        assert_eq!(v["secure_boot"], true);
        assert_eq!(v["firmware"], "/usr/share/OVMF/OVMF_CODE.fd");
        assert_eq!(v["balloon"]["free_page_reporting"], true);
    }

    #[test]
    fn error_mapping() {
        assert!(matches!(
            error_from_status(StatusCode::NOT_FOUND, "/v1/vms/x", r#"{"error":"no such vm"}"#),
            LibvirtError::NotFound(m) if m.contains("no such vm")
        ));
        assert!(matches!(
            error_from_status(StatusCode::UNAUTHORIZED, "/v1/vms", ""),
            LibvirtError::Forbidden(_)
        ));
        assert!(matches!(
            error_from_status(StatusCode::INTERNAL_SERVER_ERROR, "/v1/vms", "boom"),
            LibvirtError::Operation(_)
        ));
    }

    #[test]
    fn urlencode_names() {
        assert_eq!(urlencode("a b/c"), "a%20b%2Fc");
        assert_eq!(urlencode("vm-1_x.y"), "vm-1_x.y");
    }
}
