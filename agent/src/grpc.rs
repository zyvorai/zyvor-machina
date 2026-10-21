// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#![allow(clippy::result_large_err)]

use std::sync::Arc;

use machina_spec::VirtualMachine;
use tonic::{Request, Response, Status};

use crate::libvirt_ops;
use crate::pb::host_agent_server::HostAgent;
use crate::pb::*;
use crate::state::SharedAgentState;

pub struct AgentService {
    pub state: SharedAgentState,
    pub libvirt: Arc<std::sync::Mutex<libvirt_ops::LibvirtCtx>>,
}

impl AgentService {
    pub fn new(
        state: SharedAgentState,
        libvirt: Arc<std::sync::Mutex<libvirt_ops::LibvirtCtx>>,
    ) -> Self {
        Self { state, libvirt }
    }

    async fn libvirt_call<T, F>(&self, f: F) -> Result<T, Status>
    where
        F: FnOnce(&mut libvirt_ops::LibvirtCtx) -> Result<T, machina_core::LibvirtError>
            + Send
            + 'static,
        T: Send + 'static,
    {
        let libvirt = self.libvirt.clone();
        tokio::task::spawn_blocking(move || {
            let mut ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            f(&mut ctx)
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .map_err(|e| Status::internal(e.to_string()))
    }
}

impl AgentService {
    /// Runs `f` on a dedicated libvirt connection so long block jobs (commit,
    /// pull) don't hold the shared one that inventory and power ops use.
    async fn own_conn_call<T, F>(&self, f: F) -> Result<T, Status>
    where
        F: FnOnce(&virt::connect::Connect) -> Result<T, machina_core::LibvirtError>
            + Send
            + 'static,
        T: Send + 'static,
    {
        let uri = self
            .libvirt
            .lock()
            .map_err(|e| Status::internal(e.to_string()))?
            .uri()
            .to_string();
        tokio::task::spawn_blocking(move || {
            let mut conn = virt::connect::Connect::open(Some(&uri)).map_err(|e| {
                machina_core::LibvirtError::Connection(format!("libvirt connect {uri}: {e}"))
            })?;
            let r = f(&conn);
            let _ = conn.close();
            r
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .map_err(|e| match e {
            machina_core::LibvirtError::Invalid(m) => Status::invalid_argument(m),
            machina_core::LibvirtError::NotFound(m) => Status::not_found(m),
            other => Status::internal(other.to_string()),
        })
    }
}

fn to_layers(v: Vec<DiskLayer>) -> Vec<machina_core::libvirt::fork::Layer> {
    v.into_iter()
        .map(|l| machina_core::libvirt::fork::Layer {
            target: l.target,
            file: l.file,
        })
        .collect()
}

fn from_layers(v: Vec<machina_core::libvirt::fork::Layer>) -> Vec<DiskLayer> {
    v.into_iter()
        .map(|l| DiskLayer {
            target: l.target,
            file: l.file,
        })
        .collect()
}

fn grpc_port_u16(field: &str, value: u32) -> Result<u16, Status> {
    if value == 0 || value > u16::MAX as u32 {
        return Err(Status::invalid_argument(format!("invalid {field}")));
    }
    Ok(value as u16)
}

async fn run_vm_op<Req, F>(
    request: Request<Req>,
    libvirt: Arc<std::sync::Mutex<libvirt_ops::LibvirtCtx>>,
    op: F,
) -> Result<(), String>
where
    Req: Send + 'static,
    F: FnOnce(&libvirt_ops::LibvirtCtx, Req) -> Result<(), machina_core::LibvirtError>
        + Send
        + 'static,
{
    let req = request.into_inner();
    tokio::task::spawn_blocking(move || {
        let ctx = libvirt
            .lock()
            .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
        op(&ctx, req)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Pull the sprite inventory from this host's co-located `machina-daemon`,
/// authenticated with a short-lived, read-only platform JWT (see
/// `crate::jwt`). Sprites live only in the daemon's in-memory registry —
/// this is a pure read for fleet visibility, and never touches the
/// controller's `vms` table/reconciler.
async fn fetch_local_sprites() -> anyhow::Result<Vec<SpriteSummary>> {
    let secret = std::env::var("MACHINA_JWT_SECRET")
        .map_err(|_| anyhow::anyhow!("MACHINA_JWT_SECRET not set"))?;
    let token = crate::jwt::issue_local_token(&secret, 60)?;
    // Same env var + default as the controller's own `daemon_base_url`
    // (`controller/src/config.rs`) — both point at this host's co-located
    // daemon.
    let base =
        std::env::var("MACHINA_DAEMON_URL").unwrap_or_else(|_| "http://127.0.0.1:5092".to_string());
    let base = base.trim_end_matches('/');
    let path = "/api/v1/sprites";

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        // Loopback self-call to the daemon's own (commonly self-signed) TLS
        // listener — same trust posture as the controller's KubeVirt sync.
        .danger_accept_invalid_certs(true)
        .build()?;

    // Deployments commonly run the daemon TLS-only even on loopback, but
    // MACHINA_DAEMON_URL defaults to `http://…` — try HTTPS first, fall back
    // to the URL as configured (mirrors
    // controller/src/engine/kubevirt_inventory.rs::fetch_inventory_rows).
    let configured_url = format!("{base}{path}");
    let (host_port, configured_is_https) = match base.strip_prefix("https://") {
        Some(rest) => (rest, true),
        None => (base.strip_prefix("http://").unwrap_or(base), false),
    };
    let resp = if configured_is_https {
        client
            .get(&configured_url)
            .bearer_auth(&token)
            .send()
            .await?
    } else {
        let https_url = format!("https://{host_port}{path}");
        match client.get(&https_url).bearer_auth(&token).send().await {
            Ok(r) => r,
            Err(_) => {
                client
                    .get(&configured_url)
                    .bearer_auth(&token)
                    .send()
                    .await?
            }
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("daemon sprites HTTP {status}: {body}");
    }

    #[derive(serde::Deserialize)]
    struct DaemonSpriteHandle {
        sprite_id: String,
        state: String,
        backend: String,
        created_at: String,
        expires_at: String,
        #[serde(default)]
        vsock_cid: Option<u32>,
        #[serde(default)]
        network_egress: bool,
    }

    let handles: Vec<DaemonSpriteHandle> = resp.json().await?;
    Ok(handles
        .into_iter()
        .map(|h| SpriteSummary {
            sprite_id: h.sprite_id,
            state: h.state,
            backend: h.backend,
            created_at: h.created_at,
            expires_at: h.expires_at,
            vsock_cid: h.vsock_cid.unwrap_or(0),
            network_egress: h.network_egress,
        })
        .collect())
}

#[tonic::async_trait]
impl HostAgent for AgentService {
    async fn register(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        let req = request.into_inner();
        let mut st = self.state.write().await;
        st.hostname = req.hostname;
        Ok(Response::new(RegisterResponse {
            host_id: st.host_id.clone(),
            accepted: true,
        }))
    }

    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let _ = request.into_inner();
        // Read the maintenance flag into a local and drop the guard before the
        // blocking libvirt call. tokio's RwLock is write-preferring, so holding
        // the read guard across the (potentially slow) list_vms would stall a
        // concurrent register/maintenance writer — and every reader behind it —
        // for the whole call.
        let maintenance_state = {
            let st = self.state.read().await;
            format!("{:?}", st.maintenance).to_ascii_lowercase()
        };
        let libvirt = self.libvirt.clone();
        let (vms, stats) = tokio::task::spawn_blocking(move || {
            let mut ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            let vms = ctx.list_vms()?;
            let stats = ctx.host_resource_stats().unwrap_or((0.0, 0, 0));
            Ok::<_, machina_core::LibvirtError>((vms, stats))
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(HeartbeatResponse {
            state: maintenance_state,
            vm_count: vms.len() as u32,
            cpu_percent: stats.0,
            memory_used_mib: stats.1,
            memory_total_mib: stats.2,
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
        }))
    }

    async fn list_vms(
        &self,
        _request: Request<ListVmsRequest>,
    ) -> Result<Response<ListVmsResponse>, Status> {
        let vms = self.libvirt_call(|ctx| ctx.list_vms()).await?;
        let flux = crate::fluxvm::summaries().await;
        let fluxvm_ok = flux.is_some();
        Ok(Response::new(ListVmsResponse {
            vms: vms
                .into_iter()
                .map(|v| VmSummary {
                    name: v.name,
                    uuid: v.uuid,
                    state: v.state,
                    vcpus: v.vcpus,
                    memory_mb: v.memory_mb,
                    cpu_percent: v.cpu_percent,
                    memory_used_mib: v.memory_used_mib,
                    disk_read_iops: v.disk_read_iops,
                    disk_write_iops: v.disk_write_iops,
                    guest_ip: v.guest_ip,
                    guest_ips: v.guest_ips,
                    net_bytes: v.net_bytes,
                    ..Default::default()
                })
                .chain(flux.unwrap_or_default())
                .collect(),
            fluxvm_ok,
        }))
    }

    async fn export_fluxvm_record(
        &self,
        request: Request<ExportFluxvmRecordRequest>,
    ) -> Result<Response<ExportFluxvmRecordResponse>, Status> {
        let req = request.into_inner();
        let rec = crate::fluxvm::require()?
            .export_record(&req.vm_name)
            .await
            .map_err(crate::fluxvm::status)?;
        Ok(Response::new(ExportFluxvmRecordResponse {
            id: rec["id"].as_str().unwrap_or_default().to_string(),
            record_json: rec.to_string(),
        }))
    }

    async fn prepare_fluxvm_receiver(
        &self,
        request: Request<PrepareFluxvmReceiverRequest>,
    ) -> Result<Response<PrepareFluxvmReceiverResponse>, Status> {
        let req = request.into_inner();
        let record: serde_json::Value = serde_json::from_str(&req.record_json)
            .map_err(|e| Status::invalid_argument(format!("record_json: {e}")))?;
        let listen = if req.listen_host.is_empty() {
            "0.0.0.0"
        } else {
            req.listen_host.as_str()
        };
        let c = crate::fluxvm::require()?;
        let recv = c
            .receiver_create(record, listen, &req.advertise_host)
            .await
            .map_err(crate::fluxvm::status)?;
        if let Err(e) = c.receiver_activate(&recv.id, &recv.token).await {
            let _ = c.receiver_delete(&recv.id).await;
            return Err(crate::fluxvm::status(e));
        }
        Ok(Response::new(PrepareFluxvmReceiverResponse {
            receiver_id: recv.id,
            uri: recv.uri,
            token: recv.token,
        }))
    }

    async fn start_fluxvm_migration(
        &self,
        request: Request<StartFluxvmMigrationRequest>,
    ) -> Result<Response<StartFluxvmMigrationResponse>, Status> {
        let req = request.into_inner();
        crate::fluxvm::require()?
            .migration_start(
                &req.vm_id,
                &req.destination,
                Some(req.bandwidth_mbps),
                Some(req.max_downtime_ms),
            )
            .await
            .map_err(crate::fluxvm::status)?;
        Ok(Response::new(StartFluxvmMigrationResponse {}))
    }

    async fn get_fluxvm_migration_status(
        &self,
        request: Request<GetFluxvmMigrationStatusRequest>,
    ) -> Result<Response<GetFluxvmMigrationStatusResponse>, Status> {
        let req = request.into_inner();
        let st = crate::fluxvm::require()?
            .migration_status(&req.vm_id)
            .await
            .map_err(crate::fluxvm::status)?;
        let (progress, error) = crate::fluxvm::progress_of(&st);
        Ok(Response::new(GetFluxvmMigrationStatusResponse {
            progress: progress.into(),
            error,
            status_json: st.to_string(),
        }))
    }

    async fn finish_fluxvm_migration(
        &self,
        request: Request<FinishFluxvmMigrationRequest>,
    ) -> Result<Response<FinishFluxvmMigrationResponse>, Status> {
        let req = request.into_inner();
        crate::fluxvm::require()?
            .migration_finish(&req.vm_id)
            .await
            .map_err(crate::fluxvm::status)?;
        Ok(Response::new(FinishFluxvmMigrationResponse {}))
    }

    async fn adopt_fluxvm_vm(
        &self,
        request: Request<AdoptFluxvmVmRequest>,
    ) -> Result<Response<AdoptFluxvmVmResponse>, Status> {
        let req = request.into_inner();
        let c = crate::fluxvm::require()?;
        let rec = machina_core::fluxvm::migrate::adopt_with_retry(&c, &req.receiver_id, &req.token)
            .await
            .map_err(crate::fluxvm::status)?;
        Ok(Response::new(AdoptFluxvmVmResponse {
            id: rec.id,
            name: rec.name,
            state: machina_core::fluxvm::map_status(&rec.status),
        }))
    }

    async fn abort_fluxvm_migration(
        &self,
        request: Request<AbortFluxvmMigrationRequest>,
    ) -> Result<Response<AbortFluxvmMigrationResponse>, Status> {
        let req = request.into_inner();
        let c = crate::fluxvm::require()?;
        if !req.source_vm_id.is_empty() {
            let _ = c.migration_cancel(&req.source_vm_id).await;
        }
        if !req.receiver_id.is_empty() {
            let _ = c.receiver_delete(&req.receiver_id).await;
        }
        Ok(Response::new(AbortFluxvmMigrationResponse {}))
    }

    async fn list_sprites(
        &self,
        _request: Request<ListSpritesRequest>,
    ) -> Result<Response<ListSpritesResponse>, Status> {
        match fetch_local_sprites().await {
            Ok(sprites) => Ok(Response::new(ListSpritesResponse {
                sprites,
                daemon_reachable: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(ListSpritesResponse {
                sprites: vec![],
                daemon_reachable: false,
                error: e.to_string(),
            })),
        }
    }

    async fn list_networks(
        &self,
        _request: Request<ListNetworksRequest>,
    ) -> Result<Response<ListNetworksResponse>, Status> {
        let networks = self.libvirt_call(|ctx| ctx.list_networks()).await?;
        Ok(Response::new(ListNetworksResponse {
            networks: networks
                .into_iter()
                .map(|n| NetworkSummary {
                    name: n.name,
                    uuid: n.uuid,
                    active: n.active,
                    persistent: n.persistent,
                    autostart: n.autostart,
                    bridge: n.bridge,
                })
                .collect(),
        }))
    }

    async fn list_storage_pools(
        &self,
        _request: Request<ListStoragePoolsRequest>,
    ) -> Result<Response<ListStoragePoolsResponse>, Status> {
        let pools = self.libvirt_call(|ctx| ctx.list_storage_pools()).await?;
        Ok(Response::new(ListStoragePoolsResponse {
            pools: pools
                .into_iter()
                .map(|(p, path, backend)| StoragePoolSummary {
                    name: p.name,
                    uuid: p.uuid,
                    state: p.state,
                    capacity_gib: p.capacity_gb,
                    used_gib: p.allocation_gb,
                    available_gib: p.available_gb,
                    autostart: p.autostart,
                    path,
                    backend,
                })
                .collect(),
        }))
    }

    async fn apply_vm(
        &self,
        request: Request<ApplyVmRequest>,
    ) -> Result<Response<ApplyVmResponse>, Status> {
        let req = request.into_inner();
        if machina_core::fluxvm::is_fluxvm(Some(&req.backend)) {
            let body: serde_json::Value = serde_json::from_str(&req.fluxvm_create_json)
                .map_err(|e| Status::invalid_argument(format!("fluxvm_create_json: {e}")))?;
            let rec = crate::fluxvm::require()?
                .create_raw(body, req.shared_takeover)
                .await
                .map_err(crate::fluxvm::status)?;
            return Ok(Response::new(ApplyVmResponse {
                vm_name: rec.name,
                uuid: rec.id,
                created: true,
            }));
        }
        let vm: VirtualMachine = serde_json::from_str(&req.spec_json)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;
        let libvirt = self.libvirt.clone();
        let disk_path = req.disk_path.clone();
        let template_source = req.template_source.clone();
        let cloud = libvirt_ops::CloudInitParams {
            user: req.cloud_init_user.clone(),
            password: req.cloud_init_password.clone(),
            ssh_pubkey: req.cloud_init_ssh_pubkey.clone(),
        };
        let images_dir = std::path::Path::new(&disk_path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("/var/lib/libvirt/images")
            .to_string();
        let (name, uuid) = tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            let tpl = if template_source.is_empty() {
                None
            } else {
                Some(template_source.as_str())
            };
            ctx.apply_vm(&vm, &disk_path, tpl, &cloud, &images_dir)
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(ApplyVmResponse {
            vm_name: name,
            uuid,
            created: true,
        }))
    }

    async fn vm_power(
        &self,
        request: Request<VmPowerRequest>,
    ) -> Result<Response<VmPowerResponse>, Status> {
        let req = request.into_inner();
        if machina_core::fluxvm::is_fluxvm(Some(&req.backend)) {
            let c = crate::fluxvm::require()?;
            let verb = crate::fluxvm::verb(&req.action)?;
            c.action(&req.vm_name, verb)
                .await
                .map_err(crate::fluxvm::status)?;
            let rec = c.get(&req.vm_name).await.map_err(crate::fluxvm::status)?;
            return Ok(Response::new(VmPowerResponse {
                vm_name: req.vm_name,
                state: machina_core::fluxvm::map_status(&rec.status),
            }));
        }
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        let action = req.action.clone();
        let power_mode = if req.mode.is_empty() {
            None
        } else {
            Some(req.mode)
        };
        let state = tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.power(&vm_name, &action, power_mode.as_deref())
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(VmPowerResponse {
            vm_name: req.vm_name,
            state,
        }))
    }

    async fn get_domain_xml(
        &self,
        request: Request<GetDomainXmlRequest>,
    ) -> Result<Response<GetDomainXmlResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        let xml = tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.get_domain_xml(&vm_name)
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(GetDomainXmlResponse { xml }))
    }

    async fn delete_vm(
        &self,
        request: Request<DeleteVmRequest>,
    ) -> Result<Response<DeleteVmResponse>, Status> {
        let req = request.into_inner();
        if machina_core::fluxvm::is_fluxvm(Some(&req.backend)) {
            crate::fluxvm::require()?
                .delete(&req.vm_name)
                .await
                .map_err(crate::fluxvm::status)?;
            return Ok(Response::new(DeleteVmResponse { deleted: true }));
        }
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.delete(&vm_name)
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(DeleteVmResponse { deleted: true }))
    }

    async fn migrate_vm(
        &self,
        request: Request<MigrateVmRequest>,
    ) -> Result<Response<MigrateVmResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        let dest_uri = req.dest_uri.clone();
        let live = req.live;
        let bandwidth_mib = req.bandwidth_mib;
        let postcopy = req.postcopy;
        let undefine_source = req.undefine_source;
        let tunnelled = req.tunnelled;
        let migrate_disks = req.migrate_disks;
        let disks_uri = if req.disks_uri.is_empty() {
            None
        } else {
            Some(req.disks_uri.clone())
        };
        let copy_storage = req.copy_storage;
        tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.migrate(
                &vm_name,
                &dest_uri,
                live,
                bandwidth_mib,
                postcopy,
                undefine_source,
                tunnelled,
                migrate_disks,
                disks_uri,
                copy_storage,
            )
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(MigrateVmResponse {
            ok: true,
            message: "migration complete".into(),
        }))
    }

    async fn get_console(
        &self,
        request: Request<GetConsoleRequest>,
    ) -> Result<Response<GetConsoleResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        let (host, port) = tokio::task::spawn_blocking(move || {
            let mut ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.resolve_vnc(&vm_name)
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(GetConsoleResponse {
            console_type: "vnc".into(),
            host,
            port: i32::from(port),
            websocket_port: -1,
        }))
    }

    async fn get_console_access_plan(
        &self,
        request: Request<GetConsoleAccessPlanRequest>,
    ) -> Result<Response<GetConsoleAccessPlanResponse>, Status> {
        let req = request.into_inner();
        let vm_name = req.vm_name.clone();
        let libvirt = self.libvirt.clone();

        let plan =
            tokio::task::spawn_blocking(move || build_console_access_plan(&libvirt, &vm_name))
                .await
                .map_err(|e| Status::internal(e.to_string()))?
                .map_err(Status::internal)?;

        Ok(Response::new(plan))
    }

    async fn clone_vm(
        &self,
        request: Request<CloneVmRequest>,
    ) -> Result<Response<CloneVmResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let source = req.source_name.clone();
        let new_name = req.new_name.clone();
        let clone_mode = req.clone_mode.clone();
        let (vm_name, uuid) = tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            let uuid = ctx.clone_vm(&source, &new_name, &clone_mode)?;
            Ok::<_, machina_core::LibvirtError>((new_name, uuid))
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(CloneVmResponse { vm_name, uuid }))
    }

    async fn get_migration_status(
        &self,
        request: Request<GetMigrationStatusRequest>,
    ) -> Result<Response<GetMigrationStatusResponse>, Status> {
        let vm = request.into_inner().vm_name;
        let uri = self
            .libvirt
            .lock()
            .map_err(|e| Status::internal(e.to_string()))?
            .uri()
            .to_string();
        let st = tokio::task::spawn_blocking(move || {
            machina_core::libvirt::migration_control::migration_status(&uri, &vm)
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .map_err(|e| Status::failed_precondition(e.to_string()))?;
        Ok(Response::new(GetMigrationStatusResponse {
            active: st.active,
            job_type: st.job_type,
            time_elapsed_ms: st.time_elapsed_ms,
            data_total_bytes: st.data_total_bytes,
            data_processed_bytes: st.data_processed_bytes,
            data_remaining_bytes: st.data_remaining_bytes,
            memory_total_bytes: st.memory_total_bytes,
            memory_processed_bytes: st.memory_processed_bytes,
            memory_remaining_bytes: st.memory_remaining_bytes,
            memory_bps: st.memory_bps,
            memory_dirty_rate_pages_s: st.memory_dirty_rate_pages_s,
            downtime_ms: st.downtime_ms,
            setup_time_ms: st.setup_time_ms,
        }))
    }

    async fn control_migration(
        &self,
        request: Request<ControlMigrationRequest>,
    ) -> Result<Response<ControlMigrationResponse>, Status> {
        let r = request.into_inner();
        let uri = self
            .libvirt
            .lock()
            .map_err(|e| Status::internal(e.to_string()))?
            .uri()
            .to_string();
        let done = tokio::task::spawn_blocking(move || {
            use machina_core::libvirt::migration_control as m;
            match r.action.as_str() {
                "set_speed" => m::set_speed(&uri, &r.vm_name, r.value, r.postcopy),
                "set_downtime" => m::set_downtime(&uri, &r.vm_name, r.value),
                "postcopy" => m::postcopy(&uri, &r.vm_name),
                "abort" => m::abort(&uri, &r.vm_name),
                "throttle_vcpu" => m::set_vcpu_quota(&uri, &r.vm_name, r.value as u32),
                "restore_vcpu" => m::set_vcpu_quota(&uri, &r.vm_name, 100),
                other => Err(machina_core::LibvirtError::Invalid(format!(
                    "unknown migration action {other}"
                ))),
            }
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        done.map_err(|e| Status::failed_precondition(e.to_string()))?;
        Ok(Response::new(ControlMigrationResponse {
            ok: true,
            message: "migration control applied".into(),
        }))
    }

    async fn maintenance(
        &self,
        request: Request<MaintenanceRequest>,
    ) -> Result<Response<MaintenanceResponse>, Status> {
        let req = request.into_inner();
        let mut st = self.state.write().await;
        st.maintenance = match req.action.as_str() {
            "enter" => machina_spec::HostState::Maintenance,
            "drain" => machina_spec::HostState::Draining,
            _ => machina_spec::HostState::Online,
        };
        // NOTE: the agent does not orchestrate VM evacuation itself — it only flips
        // this host's maintenance/drain state. Actual live-migration of guests off
        // this host is driven by the controller (HA/DRS engines) via per-VM migrate
        // calls, not by this handler. No VMs are evacuated in this code path, so the
        // count is honestly 0 regardless of `req.evacuate` (the previous
        // `if req.evacuate { 0 } else { 0 }` was a dead copy-paste ternary).
        Ok(Response::new(MaintenanceResponse {
            state: format!("{:?}", st.maintenance).to_ascii_lowercase(),
            evacuated: 0,
        }))
    }

    async fn get_host_info(
        &self,
        _request: Request<GetHostInfoRequest>,
    ) -> Result<Response<GetHostInfoResponse>, Status> {
        let libvirt = self.libvirt.clone();
        let (cpu, lv, qemu) = tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.host_info()
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(GetHostInfoResponse {
            cpu_model: cpu,
            libvirt_version: lv,
            qemu_version: qemu,
        }))
    }

    async fn precheck_migrate(
        &self,
        request: Request<PrecheckMigrateRequest>,
    ) -> Result<Response<PrecheckMigrateResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        let dest_cpu = req.dest_cpu_model.clone();
        let dest_lv = req.dest_libvirt_version.clone();
        let checks = tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.precheck_migrate(&vm_name, &dest_cpu, &dest_lv)
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .map_err(|e| Status::internal(e.to_string()))?;
        let ok = checks.iter().all(|(_, passed, _)| *passed);
        Ok(Response::new(PrecheckMigrateResponse {
            ok,
            checks: checks
                .into_iter()
                .map(|(name, passed, message)| MigrateCheckItem {
                    name,
                    passed,
                    message,
                })
                .collect(),
        }))
    }

    async fn fence_host(
        &self,
        request: Request<FenceHostRequest>,
    ) -> Result<Response<FenceHostResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let hostname = if req.hostname.is_empty() {
            self.state.read().await.hostname.clone()
        } else {
            req.hostname.clone()
        };
        let method = req.method.clone();
        let ipmi_address = req.ipmi_address.clone();
        let ipmi_username = req.ipmi_username.clone();
        let ipmi_password = req.ipmi_password.clone();
        let shell_command = req.shell_command.clone();
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.fence_host(
                &hostname,
                &method,
                &ipmi_address,
                &ipmi_username,
                &ipmi_password,
                &shell_command,
            )
        })
        .await
        {
            Ok(Ok(msg)) => Ok(Response::new(FenceHostResponse {
                ok: true,
                message: msg,
            })),
            Ok(Err(e)) => Ok(Response::new(FenceHostResponse {
                ok: false,
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn create_snapshot(
        &self,
        request: Request<CreateSnapshotRequest>,
    ) -> Result<Response<CreateSnapshotResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        let snap_name = req.snapshot_name.clone();
        let desc = req.description.clone();
        let disk_only = req.disk_only;
        let quiesce = req.quiesce;
        let storage_mode = req.storage_mode.clone();
        let snap_out = snap_name.clone();
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.create_snapshot(
                &vm_name,
                &snap_name,
                &desc,
                disk_only,
                quiesce,
                &storage_mode,
            )?;
            // Best-effort disk_path — the snapshot already succeeded, so a failure
            // to read it back must not turn a real success into a reported failure.
            let disk_path = virt::domain::Domain::lookup_by_name(&ctx.conn, &vm_name)
                .ok()
                .and_then(|dom| dom.get_xml_desc(0).ok())
                .and_then(|xml| libvirt_ops::disk_path_from_xml(&xml))
                .unwrap_or_default();
            Ok::<_, machina_core::LibvirtError>(disk_path)
        })
        .await
        {
            Ok(Ok(disk_path)) => Ok(Response::new(CreateSnapshotResponse {
                ok: true,
                snapshot_name: snap_out,
                message: "created".into(),
                disk_path,
            })),
            Ok(Err(e)) => Ok(Response::new(CreateSnapshotResponse {
                ok: false,
                snapshot_name: String::new(),
                message: e.to_string(),
                disk_path: String::new(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn delete_snapshot(
        &self,
        request: Request<DeleteSnapshotRequest>,
    ) -> Result<Response<DeleteSnapshotResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        let snap_name = req.snapshot_name.clone();
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.delete_snapshot(&vm_name, &snap_name)
        })
        .await
        {
            Ok(Ok(())) => Ok(Response::new(DeleteSnapshotResponse { deleted: true })),
            Ok(Err(e)) => Err(Status::internal(e.to_string())),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn list_snapshots(
        &self,
        request: Request<ListSnapshotsRequest>,
    ) -> Result<Response<ListSnapshotsResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        let snaps = tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.list_snapshots(&vm_name)
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(ListSnapshotsResponse {
            snapshots: snaps
                .into_iter()
                .map(|(name, state, creation_time, is_current)| SnapshotInfo {
                    name,
                    state,
                    creation_time,
                    is_current,
                })
                .collect(),
        }))
    }

    async fn backup_vm(
        &self,
        request: Request<BackupVmRequest>,
    ) -> Result<Response<BackupVmResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        let dest = req.dest_path.clone();
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.backup_vm_disk(&vm_name, &dest)
        })
        .await
        {
            Ok(Ok((path, consistency))) => Ok(Response::new(BackupVmResponse {
                ok: true,
                path,
                message: format!("backup complete ({consistency}, verified)"),
            })),
            Ok(Err(e)) => Ok(Response::new(BackupVmResponse {
                ok: false,
                path: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn verify_backup(
        &self,
        request: Request<VerifyBackupRequest>,
    ) -> Result<Response<VerifyBackupResponse>, Status> {
        let path = request.into_inner().backup_path;
        let backup_root = std::env::var("MACHINA_BACKUP_DIR")
            .unwrap_or_else(|_| "/var/lib/machina/backups".into());
        if !crate::backup::within_root(&path, &backup_root) {
            return Ok(Response::new(VerifyBackupResponse {
                ok: false,
                message: format!("refused: backup path must be inside {backup_root}"),
                images_checked: 0,
            }));
        }
        let result = tokio::task::spawn_blocking(move || {
            let images = crate::backup::images_in(std::path::Path::new(&path))?;
            for img in &images {
                let out = std::process::Command::new("qemu-img")
                    .args(["check", "-f", "qcow2"])
                    .arg(img)
                    .output()
                    .map_err(|e| format!("qemu-img check: {e}"))?;
                if !crate::backup::check_accepts(out.status.code()) {
                    return Err(format!(
                        "{} failed its integrity check: {}",
                        img.display(),
                        String::from_utf8_lossy(&out.stderr).trim()
                    ));
                }
            }
            Ok::<usize, String>(images.len())
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(match result {
            Ok(n) => VerifyBackupResponse {
                ok: true,
                message: format!("{n} image(s) intact"),
                images_checked: n as i32,
            },
            Err(m) => VerifyBackupResponse {
                ok: false,
                message: m,
                images_checked: 0,
            },
        }))
    }

    async fn delete_backup(
        &self,
        request: Request<DeleteBackupRequest>,
    ) -> Result<Response<DeleteBackupResponse>, Status> {
        let path = request.into_inner().backup_path;
        // Safety: only ever delete a regular file under the managed backup directory, and
        // never a path containing traversal. This is invoked by retention (fleet_backup
        // scheduler) to reclaim on-disk backups, so it must not be a general file-delete.
        let backup_root = std::env::var("MACHINA_BACKUP_DIR")
            .unwrap_or_else(|_| "/var/lib/machina/backups".into());
        let msg = if path.contains("..") || !path.starts_with('/') {
            Some("refused: backup_path must be an absolute path without '..'".to_string())
        } else if !std::path::Path::new(&path).starts_with(&backup_root) {
            Some(format!("refused: backup_path is not under {backup_root}"))
        } else if std::path::Path::new(&path).is_dir() {
            // A multi-disk backup is a directory. Only remove one that really is ours (has a manifest) and is
            // strictly inside the backup root, never the root itself.
            let p = std::path::Path::new(&path);
            if p == std::path::Path::new(&backup_root) || !p.join(crate::backup::MANIFEST).is_file()
            {
                Some("refused: not a Machina multi-disk backup directory".to_string())
            } else {
                match std::fs::remove_dir_all(p) {
                    Ok(()) => None,
                    Err(e) => Some(format!("delete failed: {e}")),
                }
            }
        } else if !std::path::Path::new(&path).is_file() {
            // Already gone (or never a file) — treat as success so retention converges.
            None
        } else {
            match std::fs::remove_file(&path) {
                Ok(()) => None,
                Err(e) => Some(format!("delete failed: {e}")),
            }
        };
        match msg {
            None => Ok(Response::new(DeleteBackupResponse {
                ok: true,
                message: "deleted".into(),
            })),
            Some(m) => Ok(Response::new(DeleteBackupResponse {
                ok: false,
                message: m,
            })),
        }
    }

    async fn revert_snapshot(
        &self,
        request: Request<RevertSnapshotRequest>,
    ) -> Result<Response<RevertSnapshotResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        let snap_name = req.snapshot_name.clone();
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.revert_snapshot(&vm_name, &snap_name)
        })
        .await
        {
            Ok(Ok(())) => Ok(Response::new(RevertSnapshotResponse {
                ok: true,
                message: "reverted".into(),
            })),
            Ok(Err(e)) => Ok(Response::new(RevertSnapshotResponse {
                ok: false,
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn restore_vm_backup(
        &self,
        request: Request<RestoreVmBackupRequest>,
    ) -> Result<Response<RestoreVmBackupResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        let backup_path = req.backup_path.clone();
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.restore_vm_backup(&vm_name, &backup_path)
        })
        .await
        {
            Ok(Ok(())) => Ok(Response::new(RestoreVmBackupResponse {
                ok: true,
                message: "restored".into(),
            })),
            Ok(Err(e)) => Ok(Response::new(RestoreVmBackupResponse {
                ok: false,
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn clone_from_snapshot(
        &self,
        request: Request<CloneFromSnapshotRequest>,
    ) -> Result<Response<CloneFromSnapshotResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        let snap_name = req.snapshot_name.clone();
        let new_name = req.new_name.clone();
        let new_disk_path = req.new_disk_path.clone();
        let revert_source = req.revert_source;
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.clone_from_snapshot(
                &vm_name,
                &snap_name,
                &new_name,
                &new_disk_path,
                revert_source,
            )
        })
        .await
        {
            Ok(Ok((vm_name, uuid))) => Ok(Response::new(CloneFromSnapshotResponse {
                ok: true,
                vm_name,
                uuid,
                message: "cloned".into(),
            })),
            Ok(Err(e)) => Ok(Response::new(CloneFromSnapshotResponse {
                ok: false,
                vm_name: String::new(),
                uuid: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn provision_storage_pool(
        &self,
        request: Request<ProvisionStoragePoolRequest>,
    ) -> Result<Response<ProvisionStoragePoolResponse>, Status> {
        let req = request.into_inner();
        match tokio::task::spawn_blocking(move || {
            crate::provision_ops::provision_storage_pool(&req.pool_name, &req.backend, &req.path)
        })
        .await
        {
            Ok(Ok(())) => Ok(Response::new(ProvisionStoragePoolResponse {
                ok: true,
                message: "provisioned".into(),
            })),
            Ok(Err(e)) => Ok(Response::new(ProvisionStoragePoolResponse {
                ok: false,
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn provision_network(
        &self,
        request: Request<ProvisionNetworkRequest>,
    ) -> Result<Response<ProvisionNetworkResponse>, Status> {
        let req = request.into_inner();
        match tokio::task::spawn_blocking(move || {
            if req.backend == "cloud-isolated" {
                anyhow::ensure!(
                    req.network_name == format!("mc-{}", req.subnet_uuid),
                    "cloud network name must match subnet UUID"
                );
                return crate::provision_ops::provision_cloud_subnet(
                    &req.subnet_uuid,
                    &req.subnet_cidr,
                );
            }
            anyhow::ensure!(
                req.subnet_cidr.is_empty() && req.subnet_uuid.is_empty(),
                "cloud fields require cloud-isolated backend"
            );
            crate::provision_ops::provision_network(
                &req.network_name,
                &req.backend,
                req.vlan_id,
                &req.bridge,
            )
        })
        .await
        {
            Ok(Ok(())) => Ok(Response::new(ProvisionNetworkResponse {
                ok: true,
                message: "provisioned".into(),
            })),
            Ok(Err(e)) => Ok(Response::new(ProvisionNetworkResponse {
                ok: false,
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn attach_disk(
        &self,
        request: Request<AttachDiskRequest>,
    ) -> Result<Response<AttachDiskResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        let disk_path = req.disk_path.clone();
        let target = req.target_dev.clone();
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.attach_disk(&vm_name, &disk_path, &target)
        })
        .await
        {
            Ok(Ok(())) => Ok(Response::new(AttachDiskResponse {
                ok: true,
                message: "attached".into(),
            })),
            Ok(Err(e)) => Ok(Response::new(AttachDiskResponse {
                ok: false,
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn detach_disk(
        &self,
        request: Request<DetachDiskRequest>,
    ) -> Result<Response<DetachDiskResponse>, Status> {
        match run_vm_op(request, self.libvirt.clone(), |ctx, req| {
            ctx.detach_disk(&req.vm_name, &req.target_dev)
        })
        .await
        {
            Ok(()) => Ok(Response::new(DetachDiskResponse {
                ok: true,
                message: "detached".into(),
            })),
            Err(e) => Ok(Response::new(DetachDiskResponse {
                ok: false,
                message: e,
            })),
        }
    }

    async fn resize_disk(
        &self,
        request: Request<ResizeDiskRequest>,
    ) -> Result<Response<ResizeDiskResponse>, Status> {
        match run_vm_op(request, self.libvirt.clone(), |ctx, req| {
            ctx.resize_disk(&req.vm_name, &req.target_dev, req.size_gb)
        })
        .await
        {
            Ok(()) => Ok(Response::new(ResizeDiskResponse {
                ok: true,
                message: "resized".into(),
            })),
            Err(e) => Ok(Response::new(ResizeDiskResponse {
                ok: false,
                message: e,
            })),
        }
    }

    async fn attach_nic(
        &self,
        request: Request<AttachNicRequest>,
    ) -> Result<Response<AttachNicResponse>, Status> {
        match run_vm_op(request, self.libvirt.clone(), |ctx, req| {
            ctx.attach_nic(&req.vm_name, &req.network, &req.model)
        })
        .await
        {
            Ok(()) => Ok(Response::new(AttachNicResponse {
                ok: true,
                message: "attached".into(),
            })),
            Err(e) => Ok(Response::new(AttachNicResponse {
                ok: false,
                message: e,
            })),
        }
    }

    async fn detach_nic(
        &self,
        request: Request<DetachNicRequest>,
    ) -> Result<Response<DetachNicResponse>, Status> {
        match run_vm_op(request, self.libvirt.clone(), |ctx, req| {
            ctx.detach_nic(&req.vm_name, &req.mac_address)
        })
        .await
        {
            Ok(()) => Ok(Response::new(DetachNicResponse {
                ok: true,
                message: "detached".into(),
            })),
            Err(e) => Ok(Response::new(DetachNicResponse {
                ok: false,
                message: e,
            })),
        }
    }

    async fn set_autostart(
        &self,
        request: Request<SetAutostartRequest>,
    ) -> Result<Response<SetAutostartResponse>, Status> {
        match run_vm_op(request, self.libvirt.clone(), |ctx, req| {
            ctx.set_autostart(&req.vm_name, req.enabled)
        })
        .await
        {
            Ok(()) => Ok(Response::new(SetAutostartResponse {
                ok: true,
                message: "updated".into(),
            })),
            Err(e) => Ok(Response::new(SetAutostartResponse {
                ok: false,
                message: e,
            })),
        }
    }

    async fn set_vcpus(
        &self,
        request: Request<SetVcpusRequest>,
    ) -> Result<Response<SetVcpusResponse>, Status> {
        match run_vm_op(request, self.libvirt.clone(), |ctx, req| {
            ctx.set_vcpus(&req.vm_name, req.count)
        })
        .await
        {
            Ok(()) => Ok(Response::new(SetVcpusResponse {
                ok: true,
                message: "updated".into(),
            })),
            Err(e) => Ok(Response::new(SetVcpusResponse {
                ok: false,
                message: e,
            })),
        }
    }

    async fn set_memory(
        &self,
        request: Request<SetMemoryRequest>,
    ) -> Result<Response<SetMemoryResponse>, Status> {
        match run_vm_op(request, self.libvirt.clone(), |ctx, req| {
            ctx.set_memory(&req.vm_name, req.memory_mb)
        })
        .await
        {
            Ok(()) => Ok(Response::new(SetMemoryResponse {
                ok: true,
                message: "updated".into(),
            })),
            Err(e) => Ok(Response::new(SetMemoryResponse {
                ok: false,
                message: e,
            })),
        }
    }

    async fn get_vm_details(
        &self,
        request: Request<GetVmDetailsRequest>,
    ) -> Result<Response<GetVmDetailsResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.get_vm_details(&vm_name)
        })
        .await
        {
            Ok(Ok(details)) => {
                let details_json = serde_json::to_string(&details).unwrap_or_default();
                Ok(Response::new(GetVmDetailsResponse {
                    ok: true,
                    details_json,
                    message: String::new(),
                }))
            }
            Ok(Err(e)) => Ok(Response::new(GetVmDetailsResponse {
                ok: false,
                details_json: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn vm_libvirt_query(
        &self,
        request: Request<VmLibvirtQueryRequest>,
    ) -> Result<Response<VmLibvirtQueryResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        let action = req.action.clone();
        let payload: serde_json::Value =
            serde_json::from_str(&req.payload_json).unwrap_or(serde_json::json!({}));
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            crate::libvirt_invoke::vm_query(&ctx.conn, &vm_name, &action, &payload)
        })
        .await
        {
            Ok(Ok(result)) => Ok(Response::new(VmLibvirtQueryResponse {
                ok: true,
                result_json: serde_json::to_string(&result).unwrap_or_else(|_| "{}".into()),
                message: String::new(),
            })),
            Ok(Err(e)) => Ok(Response::new(VmLibvirtQueryResponse {
                ok: false,
                result_json: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn vm_libvirt_invoke(
        &self,
        request: Request<VmLibvirtInvokeRequest>,
    ) -> Result<Response<VmLibvirtInvokeResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        let action = req.action.clone();
        let payload: serde_json::Value =
            serde_json::from_str(&req.payload_json).unwrap_or(serde_json::json!({}));
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            if action == "domain.install" {
                let vm: machina_spec::VirtualMachine = payload
                    .get("spec")
                    .ok_or_else(|| {
                        machina_core::LibvirtError::Invalid("domain.install requires spec".into())
                    })
                    .and_then(|v| {
                        serde_json::from_value(v.clone())
                            .map_err(|e| machina_core::LibvirtError::Invalid(format!("spec: {e}")))
                    })?;
                ctx.install_defined_from_spec(&vm)?;
                Ok(serde_json::json!({ "status": "install_started" }))
            } else {
                crate::libvirt_invoke::vm_invoke(&ctx.conn, &vm_name, &action, &payload)
            }
        })
        .await
        {
            Ok(Ok(result)) => Ok(Response::new(VmLibvirtInvokeResponse {
                ok: true,
                result_json: serde_json::to_string(&result).unwrap_or_else(|_| "{}".into()),
                message: String::new(),
            })),
            Ok(Err(e)) => Ok(Response::new(VmLibvirtInvokeResponse {
                ok: false,
                result_json: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn host_libvirt_query(
        &self,
        request: Request<HostLibvirtQueryRequest>,
    ) -> Result<Response<HostLibvirtQueryResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let action = req.action.clone();
        let payload: serde_json::Value =
            serde_json::from_str(&req.payload_json).unwrap_or(serde_json::json!({}));
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            crate::libvirt_invoke::host_query(&ctx.conn, &action, &payload)
        })
        .await
        {
            Ok(Ok(result)) => Ok(Response::new(HostLibvirtQueryResponse {
                ok: true,
                result_json: serde_json::to_string(&result).unwrap_or_else(|_| "{}".into()),
                message: String::new(),
            })),
            Ok(Err(e)) => Ok(Response::new(HostLibvirtQueryResponse {
                ok: false,
                result_json: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn host_libvirt_invoke(
        &self,
        request: Request<HostLibvirtInvokeRequest>,
    ) -> Result<Response<HostLibvirtInvokeResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let action = req.action.clone();
        let payload: serde_json::Value =
            serde_json::from_str(&req.payload_json).unwrap_or(serde_json::json!({}));
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            crate::libvirt_invoke::host_invoke(&ctx.conn, &action, &payload)
        })
        .await
        {
            Ok(Ok(result)) => Ok(Response::new(HostLibvirtInvokeResponse {
                ok: true,
                result_json: serde_json::to_string(&result).unwrap_or_else(|_| "{}".into()),
                message: String::new(),
            })),
            Ok(Err(e)) => Ok(Response::new(HostLibvirtInvokeResponse {
                ok: false,
                result_json: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn get_guest_health(
        &self,
        request: Request<GetGuestHealthRequest>,
    ) -> Result<Response<GetGuestHealthResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.guest_health(&vm_name)
        })
        .await
        {
            Ok(Ok(summary)) => Ok(Response::new(GetGuestHealthResponse {
                ok: true,
                state: summary.state,
                agent_reachable: summary.agent_reachable,
                healthy: summary.healthy,
                os_pretty_name: summary.os_pretty_name,
                guest_ip: summary.guest_ip,
                guest_hostname: summary.guest_hostname,
                issues: summary.issues,
                install_state: summary.install_state,
                channel_attached: summary.channel_attached,
                channel_connected: summary.channel_connected,
                agent_ping: summary.agent_ping,
                agent_version: summary.agent_version,
                diagnostics_json: summary.diagnostics_json,
            })),
            Ok(Err(e)) => Ok(Response::new(GetGuestHealthResponse {
                ok: false,
                state: String::new(),
                agent_reachable: false,
                healthy: false,
                os_pretty_name: String::new(),
                guest_ip: String::new(),
                guest_hostname: String::new(),
                issues: vec![e.to_string()],
                install_state: "unknown".into(),
                channel_attached: false,
                channel_connected: false,
                agent_ping: false,
                agent_version: String::new(),
                diagnostics_json: String::new(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn get_guest_observability(
        &self,
        request: Request<GetGuestObservabilityRequest>,
    ) -> Result<Response<GetGuestObservabilityResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.guest_observability(&vm_name)
        })
        .await
        {
            Ok(Ok(json)) => Ok(Response::new(GetGuestObservabilityResponse {
                ok: true,
                guest_json: json,
                message: String::new(),
            })),
            Ok(Err(e)) => Ok(Response::new(GetGuestObservabilityResponse {
                ok: false,
                guest_json: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn guest_agent_action(
        &self,
        request: Request<GuestAgentActionRequest>,
    ) -> Result<Response<GuestAgentActionResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        let action = req.action.clone();
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.guest_agent_action(&vm_name, &action)
        })
        .await
        {
            Ok(Ok(json)) => Ok(Response::new(GuestAgentActionResponse {
                ok: true,
                result_json: json,
                message: String::new(),
            })),
            Ok(Err(e)) => Ok(Response::new(GuestAgentActionResponse {
                ok: false,
                result_json: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn install_guest_tools(
        &self,
        request: Request<InstallGuestToolsRequest>,
    ) -> Result<Response<InstallGuestToolsResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt.lock().map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.install_guest_tools(&vm_name)
        })
        .await
        {
            Ok(Ok(())) => Ok(Response::new(InstallGuestToolsResponse {
                ok: true,
                message: "Guest agent channel attached — install and enable guestkit-agent (QGA-compatible)".into(),
            })),
            Ok(Err(e)) => Ok(Response::new(InstallGuestToolsResponse {
                ok: false,
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn get_firewall_inventory(
        &self,
        _request: Request<GetFirewallInventoryRequest>,
    ) -> Result<Response<GetFirewallInventoryResponse>, Status> {
        let hostname = self.state.read().await.hostname.clone();
        match tokio::task::spawn_blocking(move || {
            machina_core::gather_firewall_inventory(&hostname)
        })
        .await
        {
            Ok(Ok(inv)) => {
                let json = serde_json::to_string(&inv).unwrap_or_else(|_| "{}".into());
                Ok(Response::new(GetFirewallInventoryResponse {
                    ok: true,
                    inventory_json: json,
                    message: String::new(),
                }))
            }
            Ok(Err(e)) => Ok(Response::new(GetFirewallInventoryResponse {
                ok: false,
                inventory_json: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn apply_firewall_plan(
        &self,
        request: Request<ApplyFirewallPlanRequest>,
    ) -> Result<Response<ApplyFirewallPlanResponse>, Status> {
        let req = request.into_inner();
        let hostname = self.state.read().await.hostname.clone();
        let plan: machina_core::FirewallPlanRequest = serde_json::from_str(&req.plan_json)
            .unwrap_or(machina_core::FirewallPlanRequest {
                profile: None,
                enable: None,
                stealth_level: None,
                preset: None,
                dry_run: req.dry_run,
                zone_cidrs: Default::default(),
            });
        let mut plan_req = plan;
        plan_req.dry_run = req.dry_run;
        match tokio::task::spawn_blocking(move || machina_core::apply_plan(&hostname, &plan_req))
            .await
        {
            Ok(Ok(result)) => {
                let json = serde_json::to_string(&result).unwrap_or_else(|_| "{}".into());
                Ok(Response::new(ApplyFirewallPlanResponse {
                    ok: true,
                    result_json: json,
                    message: String::new(),
                }))
            }
            Ok(Err(e)) => Ok(Response::new(ApplyFirewallPlanResponse {
                ok: false,
                result_json: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn get_firewall_activity(
        &self,
        _request: Request<GetFirewallActivityRequest>,
    ) -> Result<Response<GetFirewallActivityResponse>, Status> {
        let activity = crate::bpf_ops::firewall_activity().await;
        Ok(Response::new(GetFirewallActivityResponse {
            ok: true,
            activity_json: activity.to_string(),
            message: String::new(),
        }))
    }

    async fn get_guest_firewall_ports(
        &self,
        request: Request<GetGuestFirewallPortsRequest>,
    ) -> Result<Response<GetGuestFirewallPortsResponse>, Status> {
        let req = request.into_inner();
        let libvirt = self.libvirt.clone();
        let vm_name = req.vm_name.clone();
        match tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.guest_firewall_ports(&vm_name)
        })
        .await
        {
            Ok(Ok((ports, agent_reachable))) => Ok(Response::new(GetGuestFirewallPortsResponse {
                ok: true,
                agent_reachable,
                ports: ports
                    .into_iter()
                    .map(|p| GuestFirewallPort {
                        port: p.port as u32,
                        protocol: p.protocol,
                        bind_address: p.bind_address,
                        process: p.process.unwrap_or_default(),
                    })
                    .collect(),
                message: String::new(),
            })),
            Ok(Err(e)) => Ok(Response::new(GetGuestFirewallPortsResponse {
                ok: false,
                agent_reachable: false,
                ports: vec![],
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn get_linux_observability(
        &self,
        _request: Request<GetLinuxObservabilityRequest>,
    ) -> Result<Response<GetLinuxObservabilityResponse>, Status> {
        match tokio::task::spawn_blocking(machina_core::host_linux_obs::gather_linux_observability)
            .await
        {
            Ok(Ok(obs)) => match serde_json::to_string(&obs) {
                Ok(json) => Ok(Response::new(GetLinuxObservabilityResponse {
                    ok: true,
                    json,
                    message: String::new(),
                })),
                Err(e) => Ok(Response::new(GetLinuxObservabilityResponse {
                    ok: false,
                    json: String::new(),
                    message: e.to_string(),
                })),
            },
            Ok(Err(e)) => Ok(Response::new(GetLinuxObservabilityResponse {
                ok: false,
                json: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn get_systemd_network_diagnostics(
        &self,
        _request: Request<GetSystemdNetworkDiagnosticsRequest>,
    ) -> Result<Response<GetSystemdNetworkDiagnosticsResponse>, Status> {
        match tokio::task::spawn_blocking(
            machina_core::libvirt::host_network::get_systemd_network_diagnostics,
        )
        .await
        {
            Ok(Ok(diag)) => match serde_json::to_string(&diag) {
                Ok(json) => Ok(Response::new(GetSystemdNetworkDiagnosticsResponse {
                    ok: true,
                    json,
                    message: String::new(),
                })),
                Err(e) => Ok(Response::new(GetSystemdNetworkDiagnosticsResponse {
                    ok: false,
                    json: String::new(),
                    message: e.to_string(),
                })),
            },
            Ok(Err(e)) => Ok(Response::new(GetSystemdNetworkDiagnosticsResponse {
                ok: false,
                json: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn get_linux_audit(
        &self,
        _request: Request<GetLinuxAuditRequest>,
    ) -> Result<Response<GetLinuxAuditResponse>, Status> {
        match tokio::task::spawn_blocking(machina_core::linux_audit::gather_linux_audit_configured)
            .await
        {
            Ok(Ok(report)) => match serde_json::to_string(&report) {
                Ok(json) => Ok(Response::new(GetLinuxAuditResponse {
                    ok: true,
                    json,
                    message: String::new(),
                })),
                Err(e) => Ok(Response::new(GetLinuxAuditResponse {
                    ok: false,
                    json: String::new(),
                    message: e.to_string(),
                })),
            },
            Ok(Err(e)) => Ok(Response::new(GetLinuxAuditResponse {
                ok: false,
                json: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn get_linux_package_updates(
        &self,
        _request: Request<GetLinuxPackageUpdatesRequest>,
    ) -> Result<Response<GetLinuxPackageUpdatesResponse>, Status> {
        match tokio::task::spawn_blocking(machina_core::host_platform::check_package_updates).await
        {
            Ok(Ok(check)) => match serde_json::to_string(&check) {
                Ok(json) => Ok(Response::new(GetLinuxPackageUpdatesResponse {
                    ok: true,
                    json,
                    message: String::new(),
                })),
                Err(e) => Ok(Response::new(GetLinuxPackageUpdatesResponse {
                    ok: false,
                    json: String::new(),
                    message: e.to_string(),
                })),
            },
            Ok(Err(e)) => Ok(Response::new(GetLinuxPackageUpdatesResponse {
                ok: false,
                json: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn apply_linux_package_upgrade(
        &self,
        request: Request<ApplyLinuxPackageUpgradeRequest>,
    ) -> Result<Response<ApplyLinuxPackageUpgradeResponse>, Status> {
        let dry_run = request.into_inner().dry_run;
        match tokio::task::spawn_blocking(move || {
            if dry_run {
                machina_core::host_platform::package_upgrade_preview()
            } else {
                machina_core::host_platform::package_upgrade()
            }
        })
        .await
        {
            Ok(Ok(result)) => match serde_json::to_string(&result) {
                Ok(json) => Ok(Response::new(ApplyLinuxPackageUpgradeResponse {
                    // Preview is a successful probe even when apt/dnf simulate exits non-zero.
                    ok: dry_run || result.ok,
                    json,
                    message: if result.stderr.is_empty() {
                        result.stdout.clone()
                    } else {
                        result.stderr.clone()
                    },
                })),
                Err(e) => Ok(Response::new(ApplyLinuxPackageUpgradeResponse {
                    ok: false,
                    json: String::new(),
                    message: e.to_string(),
                })),
            },
            Ok(Err(e)) => Ok(Response::new(ApplyLinuxPackageUpgradeResponse {
                ok: false,
                json: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn host_linux_reboot(
        &self,
        _request: Request<HostLinuxRebootRequest>,
    ) -> Result<Response<HostLinuxRebootResponse>, Status> {
        match tokio::task::spawn_blocking(machina_core::libvirt::extras::host_reboot).await {
            Ok(Ok(())) => Ok(Response::new(HostLinuxRebootResponse {
                ok: true,
                message: "Host reboot initiated".into(),
            })),
            Ok(Err(e)) => Ok(Response::new(HostLinuxRebootResponse {
                ok: false,
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn get_linux_filesystems(
        &self,
        _request: Request<GetLinuxFilesystemsRequest>,
    ) -> Result<Response<GetLinuxFilesystemsResponse>, Status> {
        match tokio::task::spawn_blocking(machina_core::libvirt::extras::list_host_filesystems)
            .await
        {
            Ok(Ok(rows)) => match serde_json::to_string(&rows) {
                Ok(json) => Ok(Response::new(GetLinuxFilesystemsResponse {
                    ok: true,
                    json,
                    message: String::new(),
                })),
                Err(e) => Ok(Response::new(GetLinuxFilesystemsResponse {
                    ok: false,
                    json: String::new(),
                    message: e.to_string(),
                })),
            },
            Ok(Err(e)) => Ok(Response::new(GetLinuxFilesystemsResponse {
                ok: false,
                json: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn get_linux_top_processes(
        &self,
        request: Request<GetLinuxTopProcessesRequest>,
    ) -> Result<Response<GetLinuxTopProcessesResponse>, Status> {
        let req = request.into_inner();
        let limit = req.limit.clamp(1, 64);
        let order = if req.order.eq_ignore_ascii_case("cpu") {
            machina_core::libvirt::extras::HostTopProcessOrder::Cpu
        } else {
            machina_core::libvirt::extras::HostTopProcessOrder::Rss
        };
        match tokio::task::spawn_blocking(move || {
            machina_core::libvirt::extras::list_host_top_processes(limit, order)
        })
        .await
        {
            Ok(Ok(rows)) => match serde_json::to_string(&rows) {
                Ok(json) => Ok(Response::new(GetLinuxTopProcessesResponse {
                    ok: true,
                    json,
                    message: String::new(),
                })),
                Err(e) => Ok(Response::new(GetLinuxTopProcessesResponse {
                    ok: false,
                    json: String::new(),
                    message: e.to_string(),
                })),
            },
            Ok(Err(e)) => Ok(Response::new(GetLinuxTopProcessesResponse {
                ok: false,
                json: String::new(),
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn bpf_call(
        &self,
        request: Request<BpfCallRequest>,
    ) -> Result<Response<BpfCallResponse>, Status> {
        let req = request.into_inner();
        Ok(Response::new(
            match crate::bpf_ops::call(&req.request_json).await {
                Ok(data) => BpfCallResponse {
                    ok: true,
                    data_json: data.to_string(),
                    error: String::new(),
                },
                Err(e) => BpfCallResponse {
                    ok: false,
                    data_json: String::new(),
                    error: format!("{e:#}"),
                },
            },
        ))
    }

    type BpfSubscribeStream = std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<BpfEvent, Status>> + Send + 'static>,
    >;

    async fn bpf_subscribe(
        &self,
        request: Request<BpfSubscribeRequest>,
    ) -> Result<Response<Self::BpfSubscribeStream>, Status> {
        let topics = request.into_inner().topics;
        let refs: Vec<&str> = topics.iter().map(String::as_str).collect();
        let mut rx = machina_bpf::BpfdClient::from_env()
            .subscribe(&refs)
            .await
            .map_err(|e| Status::unavailable(format!("{e:#}")))?;
        let (tx, out) = tokio::sync::mpsc::channel(256);
        tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                let msg = BpfEvent {
                    topic: ev.topic,
                    event_json: ev.event.to_string(),
                };
                if tx.send(Ok(msg)).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(Box::pin(
            tokio_stream::wrappers::ReceiverStream::new(out),
        )))
    }

    async fn bpf_sync_policies(
        &self,
        request: Request<BpfSyncPoliciesRequest>,
    ) -> Result<Response<BpfSyncPoliciesResponse>, Status> {
        let req = request.into_inner();
        Ok(Response::new(
            match crate::bpf_ops::sync_policies(&req.policies_json, &req.owned_prefix).await {
                Ok(result) => BpfSyncPoliciesResponse {
                    ok: result["errors"].as_array().is_none_or(|e| e.is_empty()),
                    result_json: result.to_string(),
                    message: String::new(),
                },
                Err(e) => BpfSyncPoliciesResponse {
                    ok: false,
                    result_json: String::new(),
                    message: format!("{e:#}"),
                },
            },
        ))
    }

    async fn list_port_forwards(
        &self,
        _request: Request<ListPortForwardsRequest>,
    ) -> Result<Response<ListPortForwardsResponse>, Status> {
        match tokio::task::spawn_blocking(machina_core::libvirt::host_network::list_port_forwards)
            .await
        {
            Ok(Ok(rules)) => {
                let rules = rules
                    .into_iter()
                    .map(|r| PortForwardRuleMsg {
                        id: r.id,
                        protocol: r.protocol,
                        host_port: r.host_port as u32,
                        vm_ip: r.vm_ip,
                        vm_port: r.vm_port as u32,
                        description: r.description,
                    })
                    .collect();
                Ok(Response::new(ListPortForwardsResponse {
                    rules,
                    message: String::new(),
                }))
            }
            Ok(Err(e)) => Ok(Response::new(ListPortForwardsResponse {
                rules: vec![],
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn create_port_forward(
        &self,
        request: Request<CreatePortForwardRequest>,
    ) -> Result<Response<CreatePortForwardResponse>, Status> {
        let req = request.into_inner();
        let host_port = grpc_port_u16("host_port", req.host_port)?;
        let vm_port = grpc_port_u16("vm_port", req.vm_port)?;
        let core_req = machina_core::libvirt::host_network::CreatePortForwardRequest {
            protocol: req.protocol,
            host_port,
            vm_ip: req.vm_ip,
            vm_port,
            description: req.description,
        };
        match tokio::task::spawn_blocking(move || {
            machina_core::libvirt::host_network::create_port_forward(&core_req)
        })
        .await
        {
            Ok(Ok(())) => Ok(Response::new(CreatePortForwardResponse {
                ok: true,
                message: String::new(),
            })),
            Ok(Err(e)) => Ok(Response::new(CreatePortForwardResponse {
                ok: false,
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn delete_port_forward(
        &self,
        request: Request<DeletePortForwardRequest>,
    ) -> Result<Response<DeletePortForwardResponse>, Status> {
        let req = request.into_inner();
        let host_port = grpc_port_u16("host_port", req.host_port)?;
        let vm_port = grpc_port_u16("vm_port", req.vm_port)?;
        match tokio::task::spawn_blocking(move || {
            machina_core::libvirt::host_network::delete_port_forward(
                &req.protocol,
                host_port,
                &req.vm_ip,
                vm_port,
            )
        })
        .await
        {
            Ok(Ok(())) => Ok(Response::new(DeletePortForwardResponse {
                ok: true,
                message: String::new(),
            })),
            Ok(Err(e)) => Ok(Response::new(DeletePortForwardResponse {
                ok: false,
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn set_load_balancer(
        &self,
        request: Request<SetLoadBalancerRequest>,
    ) -> Result<Response<SetLoadBalancerResponse>, Status> {
        let req = request.into_inner();
        let host_port = grpc_port_u16("host_port", req.host_port)?;
        let mut members = Vec::with_capacity(req.members.len());
        for m in req.members {
            members.push(machina_core::libvirt::host_network::LbMember {
                vm_ip: m.vm_ip,
                port: grpc_port_u16("member port", m.port)?,
                weight: m.weight,
            });
        }
        let lb_id = req.lb_id;
        let protocol = req.protocol;
        match tokio::task::spawn_blocking(move || {
            machina_core::libvirt::host_network::set_load_balancer_rules(
                &lb_id, &protocol, host_port, &members,
            )
        })
        .await
        {
            Ok(Ok(())) => Ok(Response::new(SetLoadBalancerResponse {
                ok: true,
                message: String::new(),
            })),
            Ok(Err(e)) => Ok(Response::new(SetLoadBalancerResponse {
                ok: false,
                message: e.to_string(),
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn k3s_node(
        &self,
        request: Request<K3sNodeRequest>,
    ) -> Result<Response<K3sNodeResponse>, Status> {
        Ok(Response::new(crate::k3s::handle(request.into_inner()).await))
    }

    async fn delete_load_balancer(
        &self,
        request: Request<DeleteLoadBalancerRequest>,
    ) -> Result<Response<DeleteLoadBalancerResponse>, Status> {
        let req = request.into_inner();
        let host_port = grpc_port_u16("host_port", req.host_port)?;
        tokio::task::spawn_blocking(move || {
            machina_core::libvirt::host_network::delete_load_balancer_rules(
                &req.lb_id,
                &req.protocol,
                host_port,
            )
        })
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(DeleteLoadBalancerResponse {
            ok: true,
            message: String::new(),
        }))
    }

    async fn fork_vm(
        &self,
        request: Request<ForkVmRequest>,
    ) -> Result<Response<ForkVmResponse>, Status> {
        let req = request.into_inner();
        let opts = machina_core::libvirt::fork::ForkOptions {
            layers: to_layers(req.layers),
            memory: req.memory,
            isolate: req.isolate || req.memory,
            reseed: req.reseed,
            start: req.start,
        };
        let r = self
            .own_conn_call(move |conn| {
                machina_core::libvirt::fork::fork(
                    conn,
                    &req.source_name,
                    &req.new_name,
                    &req.label,
                    &opts,
                )
            })
            .await?;
        Ok(Response::new(ForkVmResponse {
            uuid: r.uuid,
            frozen: from_layers(r.frozen),
            disks: from_layers(r.disks),
            quiesced: r.quiesced,
            reseeded: r.reseeded,
            running: r.running,
        }))
    }

    async fn restore_point_create(
        &self,
        request: Request<RestorePointCreateRequest>,
    ) -> Result<Response<RestorePointCreateResponse>, Status> {
        let req = request.into_inner();
        let (layers, quiesced) = self
            .own_conn_call(move |conn| {
                machina_core::libvirt::fork::create_restore_point(conn, &req.vm_name, &req.label)
            })
            .await?;
        Ok(Response::new(RestorePointCreateResponse {
            layers: from_layers(layers),
            quiesced,
        }))
    }

    async fn restore_point_rewind(
        &self,
        request: Request<RestorePointRewindRequest>,
    ) -> Result<Response<RestorePointRewindResponse>, Status> {
        let req = request.into_inner();
        let layers = to_layers(req.layers);
        let (disks, restarted) = self
            .own_conn_call(move |conn| {
                machina_core::libvirt::fork::rewind(
                    conn,
                    &req.vm_name,
                    &layers,
                    &req.label,
                    &req.discard,
                )
            })
            .await?;
        Ok(Response::new(RestorePointRewindResponse {
            disks: from_layers(disks),
            restarted,
        }))
    }

    async fn restore_point_merge(
        &self,
        request: Request<RestorePointMergeRequest>,
    ) -> Result<Response<RestorePointMergeResponse>, Status> {
        let req = request.into_inner();
        if req.top.len() != req.base.len() {
            return Err(Status::invalid_argument("top and base differ in length"));
        }
        let pairs: Vec<_> = to_layers(req.top)
            .into_iter()
            .zip(to_layers(req.base))
            .collect();
        self.own_conn_call(move |conn| {
            machina_core::libvirt::fork::merge(conn, &req.vm_name, &pairs)
        })
        .await?;
        Ok(Response::new(RestorePointMergeResponse {}))
    }

    async fn restore_point_discard(
        &self,
        request: Request<RestorePointDiscardRequest>,
    ) -> Result<Response<RestorePointDiscardResponse>, Status> {
        let req = request.into_inner();
        let removed = self
            .own_conn_call(move |conn| machina_core::libvirt::fork::discard(conn, &req.files))
            .await?;
        Ok(Response::new(RestorePointDiscardResponse {
            removed: removed as u32,
        }))
    }

    async fn detach_fork(
        &self,
        request: Request<DetachForkRequest>,
    ) -> Result<Response<DetachForkResponse>, Status> {
        let req = request.into_inner();
        self.own_conn_call(move |conn| machina_core::libvirt::fork::detach(conn, &req.vm_name))
            .await?;
        Ok(Response::new(DetachForkResponse {}))
    }

    async fn list_host_gpus(
        &self,
        _request: Request<ListHostGpusRequest>,
    ) -> Result<Response<ListHostGpusResponse>, Status> {
        let gpus = self.libvirt_call(|ctx| ctx.list_host_gpus()).await?;
        let nvidia_smi_summary = std::process::Command::new("nvidia-smi")
            .arg("-L")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        Ok(Response::new(ListHostGpusResponse {
            devices: gpus
                .into_iter()
                .map(|(pci, vendor, name, group, mig)| HostGpuDevice {
                    pci_address: pci,
                    vendor,
                    device_name: name,
                    iommu_group: group,
                    mig_profile: mig,
                })
                .collect(),
            nvidia_smi_summary,
        }))
    }
}

fn resolve_console_pty(xml: &str) -> Option<String> {
    machina_core::xml::extract_attr(xml, "console", "tty")
        .filter(|s| !s.is_empty())
        .or_else(|| {
            for block in machina_core::xml::split_blocks(xml, "console") {
                if let Some(p) = machina_core::xml::extract_attr(&block, "source", "path") {
                    if !p.is_empty() {
                        return Some(p);
                    }
                }
            }
            None
        })
}

fn linux_cloud_serial_preferred(xml_lower: &str, os_hint: &str, desktop_golden: bool) -> bool {
    if os_hint != "linux" || desktop_golden {
        return false;
    }
    xml_lower.contains("cloudimg")
        || xml_lower.contains("server-cloudimg")
        || xml_lower.contains("-server-")
        || xml_lower.contains("genericcloud")
        || xml_lower.contains("cloud-init")
        || xml_lower.contains("cloudinit")
        || (xml_lower.contains("ubuntu") && xml_lower.contains(".qcow2"))
}

fn cloud_init_iso_path_from_xml(xml: &str) -> Option<String> {
    for block in machina_core::xml::split_blocks(xml, "disk") {
        let lower = block.to_lowercase();
        if !lower.contains("cloud-init")
            && !lower.contains("cloudinit")
            && !lower.contains("cidata")
        {
            continue;
        }
        if let Some(path) = machina_core::xml::extract_attr(&block, "source", "file") {
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    None
}

/// Upper bound on a cloud-init/cidata ISO we'll fully read into memory.
/// Real cloud-init seed ISOs are tiny (user-data/meta-data/network-config),
/// typically well under a few MiB — a caller with disk-attach/volume-create
/// access could otherwise name/attach an arbitrarily large file whose path
/// merely contains "cloud-init"/"cloudinit"/"cidata" and repeatedly trigger
/// this via the console-access-plan RPC, OOM-killing the whole agent process
/// (and every VM on the host) on each read.
const MAX_CLOUD_INIT_ISO_BYTES: u64 = 16 * 1024 * 1024;

fn sniff_cloud_config_from_iso(iso_path: &str) -> Option<String> {
    match std::fs::metadata(iso_path) {
        Ok(meta) if meta.len() > MAX_CLOUD_INIT_ISO_BYTES => {
            // Oversized for a legitimate cloud-init seed ISO — treat as "no
            // cloud-init config found" rather than reading it (or erroring the
            // whole RPC).
            return None;
        }
        Ok(_) => {}
        Err(_) => return None,
    }
    let data = std::fs::read(iso_path).ok()?;
    let text = String::from_utf8_lossy(&data);
    let start = text.find("#cloud-config")?;
    let tail = &text[start..];
    // `tail.find('\0')` yields a char boundary, but `tail.len().min(16_384)` is a
    // raw byte cap that may land inside a multi-byte UTF-8 sequence. Slicing on a
    // non-boundary panics; because this runs while the libvirt `Mutex` guard is
    // held, that panic would poison the mutex and break all later libvirt ops.
    // Snap the cap down to the nearest char boundary so the slice can never panic.
    let mut end = tail.find('\0').unwrap_or_else(|| tail.len().min(16_384));
    while end < tail.len() && !tail.is_char_boundary(end) {
        end -= 1;
    }
    Some(tail[..end].to_string())
}

fn infer_guest_auth_mode(user_data: &str) -> &'static str {
    let has_key = user_data.contains("ssh_authorized_keys");
    let has_pw = user_data.contains("chpasswd:")
        || user_data.contains("plain_text_passwd")
        || (user_data.contains("passwd:") && user_data.contains("lock_passwd: false"));
    match (has_key, has_pw) {
        (true, true) => "both",
        (true, false) => "ssh_key",
        (false, true) => "password",
        _ => "unknown",
    }
}

fn guest_auth_mode_from_domain_xml(xml: &str) -> String {
    let Some(iso) = cloud_init_iso_path_from_xml(xml) else {
        return "unknown".into();
    };
    let Some(user_data) = sniff_cloud_config_from_iso(&iso) else {
        return "unknown".into();
    };
    infer_guest_auth_mode(&user_data).into()
}

fn build_console_access_plan(
    libvirt: &Arc<std::sync::Mutex<libvirt_ops::LibvirtCtx>>,
    vm_name: &str,
) -> Result<GetConsoleAccessPlanResponse, String> {
    let ctx = libvirt.lock().map_err(|e| format!("libvirt lock: {e}"))?;

    let xml = ctx.get_domain_xml(vm_name).unwrap_or_default();
    let has_spice = machina_core::libvirt::graphics_convert::domain_has_spice_graphics(&xml);
    let (vnc_host, vnc_port) = ctx
        .resolve_vnc_from_xml(vm_name, &xml)
        .unwrap_or(("".into(), 0));
    let console_type = if vnc_port > 0 {
        "vnc".to_string()
    } else if has_spice {
        "spice".to_string()
    } else {
        "unknown".to_string()
    };
    let mut guest_ip = String::new();
    let mut guest_ip_host_observed = false;
    let ssh_user = std::env::var("MACHINA_DEFAULT_SSH_USER").unwrap_or_else(|_| "ubuntu".into());
    let mut os_hint = machina_core::guest_os::detect_os_hint(&xml, vm_name);

    if let Ok(health) = ctx.guest_health(vm_name) {
        if !health.guest_ip.is_empty() {
            guest_ip = health.guest_ip;
            guest_ip_host_observed = health.guest_ip_host_observed;
        }
        if !health.os_pretty_name.is_empty() {
            os_hint = machina_core::guest_os::refine_os_hint(&os_hint, &health.os_pretty_name);
        }
    }

    let xml_lower = xml.to_lowercase();
    let desktop_golden = xml_lower.contains("ubuntu-24.04-desktop")
        || xml_lower.contains("-desktop.qcow2")
        || xml_lower.contains("ubuntu-desktop");
    let _server_cloud_linux = linux_cloud_serial_preferred(&xml_lower, &os_hint, desktop_golden);
    let serial_available = resolve_console_pty(&xml).is_some();

    // Nothing below this point touches `ctx`; released before the blocking
    // network probe so a slow/unresponsive guest doesn't serialize every other
    // libvirt operation on this agent behind it (this mutex guards *all*
    // libvirt calls, not just this VM's).
    drop(ctx);

    // Serial is always last resort — only when no graphical display and no SSH/RDP alternative.
    // Native RDP: `rdp_port` is non-zero only when the
    // guest is actually listening, and that is what the controller advertises the
    // native "rdp" protocol from. Only probed when `guest_ip` is host-observed
    // (DHCP lease / ARP), not a bare guest-agent self-report: dialing a
    // guest-controlled address turns this into a port-scan oracle against
    // whatever network the host can reach.
    let rdp_up = os_hint == "windows"
        && guest_ip_host_observed
        && machina_core::guest_os::rdp_reachable(&guest_ip);

    let recommended = if rdp_up {
        "rdp".into()
    } else if console_type == "spice" {
        "spice".into()
    } else if (console_type == "vnc" && vnc_port > 0) || desktop_golden {
        "novnc".into()
    } else if serial_available {
        "serial".into()
    } else {
        "novnc".into()
    };

    Ok(GetConsoleAccessPlanResponse {
        vm_name: vm_name.to_string(),
        recommended,
        console_type,
        vnc_host,
        vnc_port: i32::from(vnc_port),
        guest_ip,
        ssh_user,
        // 0 = no native RDP path; the controller keys the "rdp" protocol off this.
        rdp_port: if rdp_up { 3389 } else { 0 },
        os_hint,
        guest_auth_mode: guest_auth_mode_from_domain_xml(&xml),
        has_spice,
    })
}

#[cfg(test)]
mod console_plan_tests {
    use super::{infer_guest_auth_mode, linux_cloud_serial_preferred};

    #[test]
    fn ubuntu_cloud_init_iso_prefers_serial() {
        let xml = r#"
            <source file='/var/lib/libvirt/images/ubuntu-cloud-init.iso'/>
            <source file='/var/lib/libvirt/images/ubuntu.qcow2'/>
        "#;
        assert!(linux_cloud_serial_preferred(
            &xml.to_lowercase(),
            "linux",
            false
        ));
    }

    #[test]
    fn desktop_golden_does_not_prefer_serial() {
        let xml = r#"<source file='/var/lib/libvirt/images/ubuntu-24.04-desktop-amd64.qcow2'/>"#;
        assert!(!linux_cloud_serial_preferred(
            &xml.to_lowercase(),
            "linux",
            true
        ));
    }

    #[test]
    fn cloud_init_user_data_ssh_key_only() {
        let user_data = r#"#cloud-config
users:
  - name: ubuntu
    ssh_authorized_keys:
      - ssh-ed25519 AAA test
"#;
        assert_eq!(infer_guest_auth_mode(user_data), "ssh_key");
    }

    #[test]
    fn cloud_init_user_data_password_only() {
        let user_data = r#"#cloud-config
chpasswd:
  list: |
    ubuntu:secret
  expire: false
"#;
        assert_eq!(infer_guest_auth_mode(user_data), "password");
    }
}
