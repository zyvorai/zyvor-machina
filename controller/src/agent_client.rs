// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use machina_agent::pb::host_agent_client::HostAgentClient;
use machina_agent::pb::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use tonic::service::interceptor::InterceptedService;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};

/// Attaches the shared agent bearer token (MACHINA_AGENT_TOKEN) to every request
/// so the agent can authenticate the controller. If the token is unset the
/// header is omitted (backward-compatible with an agent that isn't enforcing).
#[derive(Clone)]
pub struct AgentAuth {
    token: Option<String>,
}

impl tonic::service::Interceptor for AgentAuth {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        if let Some(t) = &self.token {
            if let Ok(val) = format!("Bearer {t}").parse() {
                req.metadata_mut().insert("authorization", val);
            }
        }
        // Fencing: agents refuse a controller whose leadership epoch is older than one they have already seen.
        let epoch = crate::leader::current_epoch();
        if epoch > 0 {
            if let Ok(val) = epoch.to_string().parse() {
                req.metadata_mut().insert("x-machina-epoch", val);
            }
        }
        Ok(req)
    }
}

/// Authenticated agent client type (channel wrapped with the auth interceptor).
pub type AgentClient = HostAgentClient<InterceptedService<Channel, AgentAuth>>;

pub fn normalize_agent_addr(addr: &str) -> String {
    let s = addr.trim();
    let s = s.strip_prefix("http://").unwrap_or(s);
    let s = s.strip_prefix("https://").unwrap_or(s);
    s.to_string()
}

/// One gRPC channel per agent address, reused for the life of the process. Tonic
/// channels handle reconnection transparently and are designed to be cloned/shared
/// (cheap, no I/O), so caching them avoids re-paying a TCP/TLS handshake on every
/// single RPC. Without this, a handler that awaits N hosts serially (e.g. the
/// fleet/desktop dashboard overview) paid a full dial per host per request — on a
/// single-host dev deployment this alone made that endpoint consistently take
/// ~2s. The auth token is still read fresh per call below, so token rotation
/// keeps working even though the underlying channel is cached.
fn channel_cache() -> &'static Mutex<HashMap<String, Channel>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Channel>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Per-host agent tokens (normalized agent address -> token), loaded from `hosts.agent_token` at
/// startup and updated when a host joins. A host without an entry uses the shared
/// `MACHINA_AGENT_TOKEN`.
fn host_tokens() -> &'static std::sync::RwLock<HashMap<String, String>> {
    static TOKENS: OnceLock<std::sync::RwLock<HashMap<String, String>>> = OnceLock::new();
    TOKENS.get_or_init(Default::default)
}

/// Agents (normalized addresses) that serve gRPC over the fleet's mutual TLS.
fn tls_hosts() -> &'static std::sync::RwLock<std::collections::HashSet<String>> {
    static TLS: OnceLock<std::sync::RwLock<std::collections::HashSet<String>>> = OnceLock::new();
    TLS.get_or_init(Default::default)
}

pub fn set_host_tls(addr: &str) {
    tls_hosts()
        .write()
        .unwrap()
        .insert(normalize_agent_addr(addr));
}

fn is_tls_host(addr: &str) -> bool {
    tls_hosts()
        .read()
        .unwrap()
        .contains(&normalize_agent_addr(addr))
}

pub fn set_host_token(addr: &str, token: &str) {
    host_tokens()
        .write()
        .unwrap()
        .insert(normalize_agent_addr(addr), token.to_string());
}

/// A host's agent and console addresses both authenticate with its own token.
pub fn forget_host_addrs(addrs: &[&str]) {
    for a in addrs {
        forget_host_token(a);
    }
}

pub fn forget_host_token(addr: &str) {
    tls_hosts()
        .write()
        .unwrap()
        .remove(&normalize_agent_addr(addr));
    host_tokens()
        .write()
        .unwrap()
        .remove(&normalize_agent_addr(addr));
}

/// The token to present to the agent at `addr`: its own if it joined with one, else the shared one.
pub fn token_for(addr: &str) -> Option<String> {
    host_tokens()
        .read()
        .unwrap()
        .get(&normalize_agent_addr(addr))
        .cloned()
        .or_else(|| {
            std::env::var("MACHINA_AGENT_TOKEN")
                .ok()
                .filter(|s| !s.is_empty())
        })
}

/// Loads every host's own token from the database (call once at startup).
pub async fn load_host_tokens(pool: &crate::db::DbPool) -> Result<usize, sqlx::Error> {
    let rows: Vec<(String, Option<String>, String)> = crate::db::query_as(
        "SELECT agent_grpc_addr, agent_console_addr, COALESCE(agent_token, '') FROM hosts WHERE agent_token IS NOT NULL AND agent_token <> ''",
    )
    .fetch_all(pool)
    .await?;
    let tls: Vec<(String,)> =
        crate::db::query_as("SELECT agent_grpc_addr FROM hosts WHERE agent_tls = 1")
            .fetch_all(pool)
            .await?;
    for (addr,) in &tls {
        set_host_tls(addr);
    }
    for (addr, console, token) in &rows {
        set_host_token(addr, token);
        if let Some(c) = console.as_deref().filter(|c| !c.is_empty()) {
            set_host_token(c, token);
        }
    }
    Ok(rows.len())
}

pub async fn connect(addr: &str) -> anyhow::Result<AgentClient> {
    let normalized = normalize_agent_addr(addr);
    let use_tls = std::env::var("MACHINA_AGENT_CA")
        .ok()
        .filter(|p| Path::new(p).exists());
    let fleet_tls = is_tls_host(&normalized);
    let endpoint_url = if use_tls.is_some() || fleet_tls {
        format!("https://{normalized}")
    } else {
        format!("http://{normalized}")
    };

    let cached = channel_cache().lock().unwrap().get(&endpoint_url).cloned();
    let channel = match cached {
        Some(ch) => ch,
        None => {
            // Bound the TCP/TLS dial so an unreachable or blackholed host can't
            // hang the caller forever waiting to connect. Per-call timeouts for
            // read/query RPCs are applied by `timed`/`read_rpc` below;
            // mutating/long-running ops rely on a caller-side bound where one is
            // needed (e.g. host.inventory in worker.rs).
            let mut endpoint = Endpoint::from_shared(endpoint_url.clone())?
                .connect_timeout(std::time::Duration::from_secs(10));
            if fleet_tls {
                // Mutual TLS: trust the fleet CA only, present the controller's certificate,
                // and expect the agent to be named agent.machina (agents are dialled by address).
                let id = crate::pki::controller_identity()?;
                let tls = ClientTlsConfig::new()
                    .ca_certificate(Certificate::from_pem(id.ca_pem))
                    .identity(Identity::from_pem(id.cert_pem, id.key_pem))
                    .domain_name(crate::pki::AGENT_DOMAIN);
                endpoint = endpoint.tls_config(tls)?;
            } else if let Some(ca_path) = use_tls {
                let ca = tokio::fs::read_to_string(&ca_path).await?;
                let mut tls = ClientTlsConfig::new().ca_certificate(Certificate::from_pem(ca));
                if let (Ok(cert_path), Ok(key_path)) = (
                    std::env::var("MACHINA_AGENT_CLIENT_CERT"),
                    std::env::var("MACHINA_AGENT_CLIENT_KEY"),
                ) {
                    if Path::new(&cert_path).exists() && Path::new(&key_path).exists() {
                        let cert = tokio::fs::read_to_string(&cert_path).await?;
                        let key = tokio::fs::read_to_string(&key_path).await?;
                        tls = tls.identity(Identity::from_pem(cert, key));
                    }
                }
                endpoint = endpoint.tls_config(tls)?;
            }
            let ch = endpoint.connect().await?;
            channel_cache()
                .lock()
                .unwrap()
                .insert(endpoint_url, ch.clone());
            ch
        }
    };

    let token = token_for(addr);
    Ok(HostAgentClient::with_interceptor(
        channel,
        AgentAuth { token },
    ))
}

/// Bound read/query RPCs (inventory, status, diagnostics, config lookups).
/// These calls are expected to return in well under a second, so a wedged agent
/// (TCP accepted, then stuck in libvirt) must not block the caller forever —
/// whether that's the single serial task worker, an Axum request handler, or a
/// background engine loop. Mutating/long-running ops (apply/migrate/backup/
/// snapshot/package-upgrade/reboot) are intentionally NOT bounded here — they
/// legitimately run for minutes.
const READ_RPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

// Bug fix: this timeout previously covered only list_vms/list_networks/
// list_storage_pools even though the comment above claims "per-call timeouts
// for hot-path RPCs are applied at the call sites" — in reality most read/query
// RPCs below (get_host_info, heartbeat, get_vm_details, vm_libvirt_query,
// host_libvirt_query, get_guest_health, list_snapshots, get_console, the linux/
// firewall observability calls, etc.) are invoked directly from Axum HTTP
// handlers and engine loops with no timeout at any call site, so a wedged agent
// (TCP connected, stuck in libvirt) hangs that request/loop forever. `timed`
// generalizes the same bound to any RPC future, including ones whose caller
// needs the raw Response (to inspect an ok/message pair) rather than just the
// inner value.
async fn timed<T>(
    label: &str,
    fut: impl std::future::Future<Output = Result<tonic::Response<T>, tonic::Status>>,
) -> anyhow::Result<tonic::Response<T>> {
    match tokio::time::timeout(READ_RPC_TIMEOUT, fut).await {
        Ok(res) => Ok(res?),
        Err(_) => Err(anyhow::anyhow!(
            "agent {label} timed out after {}s",
            READ_RPC_TIMEOUT.as_secs()
        )),
    }
}

async fn read_rpc<T>(
    label: &str,
    fut: impl std::future::Future<Output = Result<tonic::Response<T>, tonic::Status>>,
) -> anyhow::Result<T> {
    Ok(timed(label, fut).await?.into_inner())
}

pub async fn list_vms(client: &mut AgentClient) -> anyhow::Result<ListVmsResponse> {
    read_rpc("list_vms", client.list_vms(ListVmsRequest {})).await
}

pub async fn list_sprites(client: &mut AgentClient) -> anyhow::Result<ListSpritesResponse> {
    read_rpc("list_sprites", client.list_sprites(ListSpritesRequest {})).await
}

pub async fn list_networks(client: &mut AgentClient) -> anyhow::Result<ListNetworksResponse> {
    read_rpc(
        "list_networks",
        client.list_networks(ListNetworksRequest {}),
    )
    .await
}

pub async fn list_storage_pools(
    client: &mut AgentClient,
) -> anyhow::Result<ListStoragePoolsResponse> {
    read_rpc(
        "list_storage_pools",
        client.list_storage_pools(ListStoragePoolsRequest {}),
    )
    .await
}

pub async fn apply_vm(
    client: &mut AgentClient,
    spec_json: &str,
    disk_path: &str,
    template_source: Option<&str>,
    cloud_init_user: &str,
    cloud_init_password: &str,
    cloud_init_ssh_pubkey: &str,
) -> anyhow::Result<ApplyVmResponse> {
    Ok(client
        .apply_vm(ApplyVmRequest {
            spec_json: spec_json.to_string(),
            disk_path: disk_path.to_string(),
            template_source: template_source.unwrap_or("").to_string(),
            cloud_init_user: cloud_init_user.to_string(),
            cloud_init_password: cloud_init_password.to_string(),
            cloud_init_ssh_pubkey: cloud_init_ssh_pubkey.to_string(),
            ..Default::default()
        })
        .await?
        .into_inner())
}

pub async fn vm_power(
    client: &mut AgentClient,
    vm_name: &str,
    action: &str,
    mode: Option<&str>,
) -> anyhow::Result<VmPowerResponse> {
    Ok(client
        .vm_power(VmPowerRequest {
            vm_name: vm_name.to_string(),
            action: action.to_string(),
            mode: mode.unwrap_or("").to_string(),
            ..Default::default()
        })
        .await?
        .into_inner())
}

pub async fn guest_agent_action(
    client: &mut AgentClient,
    vm_name: &str,
    action: &str,
) -> anyhow::Result<serde_json::Value> {
    let resp = client
        .guest_agent_action(GuestAgentActionRequest {
            vm_name: vm_name.to_string(),
            action: action.to_string(),
        })
        .await?
        .into_inner();
    if !resp.ok {
        anyhow::bail!("{}", resp.message);
    }
    serde_json::from_str(&resp.result_json).map_err(|e| anyhow::anyhow!("guest action JSON: {e}"))
}

pub async fn get_domain_xml(client: &mut AgentClient, vm_name: &str) -> anyhow::Result<String> {
    Ok(timed(
        "get_domain_xml",
        client.get_domain_xml(GetDomainXmlRequest {
            vm_name: vm_name.to_string(),
        }),
    )
    .await?
    .into_inner()
    .xml)
}

pub async fn delete_vm(client: &mut AgentClient, vm_name: &str) -> anyhow::Result<()> {
    client
        .delete_vm(DeleteVmRequest {
            vm_name: vm_name.to_string(),
            ..Default::default()
        })
        .await?;
    Ok(())
}

pub async fn heartbeat(
    client: &mut AgentClient,
    host_id: &str,
) -> anyhow::Result<HeartbeatResponse> {
    read_rpc(
        "heartbeat",
        client.heartbeat(HeartbeatRequest {
            host_id: host_id.to_string(),
        }),
    )
    .await
}

pub async fn get_console(
    client: &mut AgentClient,
    vm_name: &str,
) -> anyhow::Result<GetConsoleResponse> {
    read_rpc(
        "get_console",
        client.get_console(GetConsoleRequest {
            vm_name: vm_name.to_string(),
        }),
    )
    .await
}

pub async fn get_console_access_plan(
    client: &mut AgentClient,
    vm_name: &str,
) -> anyhow::Result<GetConsoleAccessPlanResponse> {
    read_rpc(
        "get_console_access_plan",
        client.get_console_access_plan(GetConsoleAccessPlanRequest {
            vm_name: vm_name.to_string(),
        }),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn migrate_vm(
    client: &mut AgentClient,
    vm_name: &str,
    dest_uri: &str,
    live: bool,
    bandwidth_mib: u64,
    postcopy: bool,
    undefine_source: bool,
    tunnelled: bool,
    migrate_disks: Vec<String>,
    disks_uri: Option<String>,
    copy_storage: bool,
) -> anyhow::Result<MigrateVmResponse> {
    let resp = client
        .migrate_vm(MigrateVmRequest {
            vm_name: vm_name.to_string(),
            dest_uri: dest_uri.to_string(),
            live,
            bandwidth_mib,
            postcopy,
            undefine_source,
            tunnelled,
            migrate_disks,
            disks_uri: disks_uri.unwrap_or_default(),
            copy_storage,
        })
        .await?
        .into_inner();
    // Unlike most agent RPCs (which signal failure via a gRPC error status),
    // MigrateVmResponse carries its own ok/message pair. The caller (vm_migrate)
    // discards this response and unconditionally commits the VM's new host_id +
    // a 'completed' migration_jobs row — without this check, an agent that
    // returns ok=false (migration didn't actually happen) would still be
    // recorded as a successful migration, leaving the VM's DB record pointing
    // at a host it never moved to while the domain keeps running on the source.
    if !resp.ok {
        anyhow::bail!("{}", resp.message);
    }
    Ok(resp)
}

pub async fn get_migration_status(
    client: &mut AgentClient,
    vm: &str,
) -> anyhow::Result<GetMigrationStatusResponse> {
    read_rpc(
        "get_migration_status",
        client.get_migration_status(GetMigrationStatusRequest { vm_name: vm.into() }),
    )
    .await
}

pub async fn control_migration(
    client: &mut AgentClient,
    vm: &str,
    action: &str,
    value: u64,
    postcopy: bool,
) -> anyhow::Result<ControlMigrationResponse> {
    let r = client
        .control_migration(ControlMigrationRequest {
            vm_name: vm.into(),
            action: action.into(),
            value,
            postcopy,
        })
        .await?
        .into_inner();
    if !r.ok {
        anyhow::bail!("{}", r.message);
    }
    Ok(r)
}

pub async fn clone_vm(
    client: &mut AgentClient,
    source: &str,
    new_name: &str,
    clone_mode: &str,
) -> anyhow::Result<CloneVmResponse> {
    Ok(client
        .clone_vm(CloneVmRequest {
            source_name: source.to_string(),
            new_name: new_name.to_string(),
            spec_json: String::new(),
            clone_mode: clone_mode.to_string(),
        })
        .await?
        .into_inner())
}

pub async fn maintenance(
    client: &mut AgentClient,
    action: &str,
    evacuate: bool,
) -> anyhow::Result<MaintenanceResponse> {
    Ok(client
        .maintenance(MaintenanceRequest {
            action: action.to_string(),
            evacuate,
        })
        .await?
        .into_inner())
}

pub async fn get_host_info(client: &mut AgentClient) -> anyhow::Result<GetHostInfoResponse> {
    read_rpc("get_host_info", client.get_host_info(GetHostInfoRequest {})).await
}

pub async fn precheck_migrate(
    client: &mut AgentClient,
    vm_name: &str,
    dest_cpu_model: &str,
    dest_libvirt_version: &str,
) -> anyhow::Result<PrecheckMigrateResponse> {
    Ok(client
        .precheck_migrate(PrecheckMigrateRequest {
            vm_name: vm_name.to_string(),
            dest_cpu_model: dest_cpu_model.to_string(),
            dest_libvirt_version: dest_libvirt_version.to_string(),
        })
        .await?
        .into_inner())
}

pub async fn fence_host(
    client: &mut AgentClient,
    hostname: &str,
    method: &str,
    ipmi_address: &str,
    ipmi_username: &str,
    ipmi_password: &str,
    shell_command: &str,
) -> anyhow::Result<FenceHostResponse> {
    Ok(client
        .fence_host(FenceHostRequest {
            action: "fence".into(),
            method: method.to_string(),
            ipmi_address: ipmi_address.to_string(),
            ipmi_username: ipmi_username.to_string(),
            ipmi_password: ipmi_password.to_string(),
            shell_command: shell_command.to_string(),
            hostname: hostname.to_string(),
        })
        .await?
        .into_inner())
}

pub async fn create_snapshot(
    client: &mut AgentClient,
    vm_name: &str,
    snapshot_name: &str,
    description: &str,
    disk_only: bool,
    quiesce: bool,
    storage_mode: &str,
) -> anyhow::Result<CreateSnapshotResponse> {
    Ok(client
        .create_snapshot(CreateSnapshotRequest {
            vm_name: vm_name.to_string(),
            snapshot_name: snapshot_name.to_string(),
            description: description.to_string(),
            disk_only,
            quiesce,
            storage_mode: storage_mode.to_string(),
        })
        .await?
        .into_inner())
}

pub async fn delete_snapshot(
    client: &mut AgentClient,
    vm_name: &str,
    snapshot_name: &str,
) -> anyhow::Result<DeleteSnapshotResponse> {
    Ok(client
        .delete_snapshot(DeleteSnapshotRequest {
            vm_name: vm_name.to_string(),
            snapshot_name: snapshot_name.to_string(),
        })
        .await?
        .into_inner())
}

pub async fn list_snapshots(
    client: &mut AgentClient,
    vm_name: &str,
) -> anyhow::Result<ListSnapshotsResponse> {
    read_rpc(
        "list_snapshots",
        client.list_snapshots(ListSnapshotsRequest {
            vm_name: vm_name.to_string(),
        }),
    )
    .await
}

pub async fn backup_vm(
    client: &mut AgentClient,
    vm_name: &str,
    dest_path: &str,
) -> anyhow::Result<BackupVmResponse> {
    Ok(client
        .backup_vm(BackupVmRequest {
            vm_name: vm_name.to_string(),
            dest_path: dest_path.to_string(),
        })
        .await?
        .into_inner())
}

pub async fn revert_snapshot(
    client: &mut AgentClient,
    vm_name: &str,
    snapshot_name: &str,
) -> anyhow::Result<RevertSnapshotResponse> {
    Ok(client
        .revert_snapshot(RevertSnapshotRequest {
            vm_name: vm_name.to_string(),
            snapshot_name: snapshot_name.to_string(),
        })
        .await?
        .into_inner())
}

pub async fn restore_vm_backup(
    client: &mut AgentClient,
    vm_name: &str,
    backup_path: &str,
) -> anyhow::Result<RestoreVmBackupResponse> {
    Ok(client
        .restore_vm_backup(RestoreVmBackupRequest {
            vm_name: vm_name.to_string(),
            backup_path: backup_path.to_string(),
        })
        .await?
        .into_inner())
}

pub async fn delete_backup(
    client: &mut AgentClient,
    backup_path: &str,
) -> anyhow::Result<DeleteBackupResponse> {
    Ok(client
        .delete_backup(DeleteBackupRequest {
            backup_path: backup_path.to_string(),
        })
        .await?
        .into_inner())
}

pub async fn verify_backup(
    client: &mut AgentClient,
    backup_path: &str,
) -> anyhow::Result<VerifyBackupResponse> {
    Ok(client
        .verify_backup(VerifyBackupRequest {
            backup_path: backup_path.to_string(),
        })
        .await?
        .into_inner())
}

pub async fn clone_from_snapshot(
    client: &mut AgentClient,
    vm_name: &str,
    snapshot_name: &str,
    new_name: &str,
    new_disk_path: &str,
    revert_source: bool,
) -> anyhow::Result<CloneFromSnapshotResponse> {
    Ok(client
        .clone_from_snapshot(CloneFromSnapshotRequest {
            vm_name: vm_name.to_string(),
            snapshot_name: snapshot_name.to_string(),
            new_name: new_name.to_string(),
            new_disk_path: new_disk_path.to_string(),
            revert_source,
        })
        .await?
        .into_inner())
}

pub async fn provision_storage_pool(
    client: &mut AgentClient,
    pool_name: &str,
    backend: &str,
    path: &str,
) -> anyhow::Result<()> {
    let resp = client
        .provision_storage_pool(ProvisionStoragePoolRequest {
            pool_name: pool_name.to_string(),
            backend: backend.to_string(),
            path: path.to_string(),
        })
        .await?
        .into_inner();
    if resp.ok {
        Ok(())
    } else {
        anyhow::bail!("{}", resp.message)
    }
}

pub async fn provision_network(
    client: &mut AgentClient,
    network_name: &str,
    backend: &str,
    vlan_id: i32,
    bridge: &str,
) -> anyhow::Result<()> {
    let resp = client
        .provision_network(ProvisionNetworkRequest {
            network_name: network_name.to_string(),
            backend: backend.to_string(),
            vlan_id,
            bridge: bridge.to_string(),
            subnet_cidr: String::new(),
            subnet_uuid: String::new(),
        })
        .await?
        .into_inner();
    if resp.ok {
        Ok(())
    } else {
        anyhow::bail!("{}", resp.message)
    }
}

pub async fn attach_disk(
    client: &mut AgentClient,
    vm_name: &str,
    disk_path: &str,
    target_dev: &str,
) -> anyhow::Result<()> {
    let resp = client
        .attach_disk(AttachDiskRequest {
            vm_name: vm_name.to_string(),
            disk_path: disk_path.to_string(),
            target_dev: target_dev.to_string(),
        })
        .await?
        .into_inner();
    vm_op_response(resp.ok, &resp.message)
}

fn vm_op_response(ok: bool, message: &str) -> anyhow::Result<()> {
    if ok {
        Ok(())
    } else {
        anyhow::bail!("{message}")
    }
}

pub async fn detach_disk(
    client: &mut AgentClient,
    vm_name: &str,
    target_dev: &str,
) -> anyhow::Result<()> {
    let resp = client
        .detach_disk(DetachDiskRequest {
            vm_name: vm_name.to_string(),
            target_dev: target_dev.to_string(),
        })
        .await?
        .into_inner();
    vm_op_response(resp.ok, &resp.message)
}

pub async fn resize_disk(
    client: &mut AgentClient,
    vm_name: &str,
    target_dev: &str,
    size_gb: u64,
) -> anyhow::Result<()> {
    let resp = client
        .resize_disk(ResizeDiskRequest {
            vm_name: vm_name.to_string(),
            target_dev: target_dev.to_string(),
            size_gb,
        })
        .await?
        .into_inner();
    vm_op_response(resp.ok, &resp.message)
}

pub async fn attach_nic(
    client: &mut AgentClient,
    vm_name: &str,
    network: &str,
    model: &str,
) -> anyhow::Result<()> {
    let resp = client
        .attach_nic(AttachNicRequest {
            vm_name: vm_name.to_string(),
            network: network.to_string(),
            model: model.to_string(),
        })
        .await?
        .into_inner();
    vm_op_response(resp.ok, &resp.message)
}

pub async fn detach_nic(
    client: &mut AgentClient,
    vm_name: &str,
    mac_address: &str,
) -> anyhow::Result<()> {
    let resp = client
        .detach_nic(DetachNicRequest {
            vm_name: vm_name.to_string(),
            mac_address: mac_address.to_string(),
        })
        .await?
        .into_inner();
    vm_op_response(resp.ok, &resp.message)
}

pub async fn set_autostart(
    client: &mut AgentClient,
    vm_name: &str,
    enabled: bool,
) -> anyhow::Result<()> {
    let resp = client
        .set_autostart(SetAutostartRequest {
            vm_name: vm_name.to_string(),
            enabled,
        })
        .await?
        .into_inner();
    vm_op_response(resp.ok, &resp.message)
}

pub async fn set_vcpus(client: &mut AgentClient, vm_name: &str, count: u32) -> anyhow::Result<()> {
    let resp = client
        .set_vcpus(SetVcpusRequest {
            vm_name: vm_name.to_string(),
            count,
        })
        .await?
        .into_inner();
    vm_op_response(resp.ok, &resp.message)
}

pub async fn set_memory(
    client: &mut AgentClient,
    vm_name: &str,
    memory_mb: u64,
) -> anyhow::Result<()> {
    let resp = client
        .set_memory(SetMemoryRequest {
            vm_name: vm_name.to_string(),
            memory_mb,
        })
        .await?
        .into_inner();
    vm_op_response(resp.ok, &resp.message)
}

pub async fn vm_libvirt_query(
    client: &mut AgentClient,
    vm_name: &str,
    action: &str,
    payload: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let resp = timed(
        "vm_libvirt_query",
        client.vm_libvirt_query(VmLibvirtQueryRequest {
            vm_name: vm_name.to_string(),
            action: action.to_string(),
            payload_json: serde_json::to_string(payload)?,
        }),
    )
    .await?
    .into_inner();
    if resp.ok {
        serde_json::from_str(&resp.result_json).map_err(|e| anyhow::anyhow!("decode query: {e}"))
    } else {
        anyhow::bail!("{}", resp.message)
    }
}

pub async fn vm_libvirt_invoke(
    client: &mut AgentClient,
    vm_name: &str,
    action: &str,
    payload: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let resp = timed(
        "vm_libvirt_invoke",
        client.vm_libvirt_invoke(VmLibvirtInvokeRequest {
            vm_name: vm_name.to_string(),
            action: action.to_string(),
            payload_json: serde_json::to_string(payload)?,
        }),
    )
    .await?
    .into_inner();
    if resp.ok {
        serde_json::from_str(&resp.result_json).map_err(|e| anyhow::anyhow!("decode invoke: {e}"))
    } else {
        anyhow::bail!("{}", resp.message)
    }
}

pub async fn host_libvirt_query(
    client: &mut AgentClient,
    action: &str,
    payload: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let resp = timed(
        "host_libvirt_query",
        client.host_libvirt_query(HostLibvirtQueryRequest {
            action: action.to_string(),
            payload_json: serde_json::to_string(payload)?,
        }),
    )
    .await?
    .into_inner();
    if resp.ok {
        serde_json::from_str(&resp.result_json)
            .map_err(|e| anyhow::anyhow!("decode host query: {e}"))
    } else {
        anyhow::bail!("{}", resp.message)
    }
}

pub async fn host_libvirt_invoke(
    client: &mut AgentClient,
    action: &str,
    payload: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let resp = timed(
        "host_libvirt_invoke",
        client.host_libvirt_invoke(HostLibvirtInvokeRequest {
            action: action.to_string(),
            payload_json: serde_json::to_string(payload)?,
        }),
    )
    .await?
    .into_inner();
    if resp.ok {
        serde_json::from_str(&resp.result_json)
            .map_err(|e| anyhow::anyhow!("decode host invoke: {e}"))
    } else {
        anyhow::bail!("{}", resp.message)
    }
}

pub async fn get_vm_details(
    client: &mut AgentClient,
    vm_name: &str,
) -> anyhow::Result<machina_core::state::VmDetails> {
    let resp = timed(
        "get_vm_details",
        client.get_vm_details(GetVmDetailsRequest {
            vm_name: vm_name.to_string(),
        }),
    )
    .await?
    .into_inner();
    if resp.ok {
        serde_json::from_str(&resp.details_json)
            .map_err(|e| anyhow::anyhow!("decode vm details: {e}"))
    } else {
        anyhow::bail!("{}", resp.message)
    }
}

#[derive(Debug, Clone)]
pub struct GuestHealthResult {
    pub agent_reachable: bool,
    pub healthy: bool,
    pub os_pretty_name: String,
    pub guest_ip: String,
    pub guest_hostname: String,
    pub issues: Vec<String>,
    pub install_state: String,
    pub channel_attached: bool,
    pub channel_connected: bool,
    pub agent_ping: bool,
    pub agent_version: String,
    pub diagnostics_json: String,
}

pub async fn get_guest_health(
    client: &mut AgentClient,
    vm_name: &str,
) -> anyhow::Result<GuestHealthResult> {
    let resp = timed(
        "get_guest_health",
        client.get_guest_health(GetGuestHealthRequest {
            vm_name: vm_name.to_string(),
        }),
    )
    .await?
    .into_inner();
    if resp.ok {
        Ok(GuestHealthResult {
            agent_reachable: resp.agent_reachable,
            healthy: resp.healthy,
            os_pretty_name: resp.os_pretty_name,
            guest_ip: resp.guest_ip,
            guest_hostname: resp.guest_hostname,
            issues: resp.issues,
            install_state: resp.install_state,
            channel_attached: resp.channel_attached,
            channel_connected: resp.channel_connected,
            agent_ping: resp.agent_ping,
            agent_version: resp.agent_version,
            diagnostics_json: resp.diagnostics_json,
        })
    } else {
        Ok(GuestHealthResult {
            agent_reachable: false,
            healthy: false,
            os_pretty_name: String::new(),
            guest_ip: String::new(),
            guest_hostname: String::new(),
            issues: resp.issues,
            install_state: resp.install_state,
            channel_attached: resp.channel_attached,
            channel_connected: resp.channel_connected,
            agent_ping: resp.agent_ping,
            agent_version: resp.agent_version,
            diagnostics_json: resp.diagnostics_json,
        })
    }
}

pub async fn get_guest_observability(
    client: &mut AgentClient,
    vm_name: &str,
) -> anyhow::Result<serde_json::Value> {
    let resp = timed(
        "get_guest_observability",
        client.get_guest_observability(GetGuestObservabilityRequest {
            vm_name: vm_name.to_string(),
        }),
    )
    .await?
    .into_inner();
    if !resp.ok {
        anyhow::bail!("{}", resp.message);
    }
    let v: serde_json::Value =
        serde_json::from_str(&resp.guest_json).unwrap_or(serde_json::json!({}));
    Ok(v)
}

pub async fn install_guest_tools(client: &mut AgentClient, vm_name: &str) -> anyhow::Result<()> {
    let resp = client
        .install_guest_tools(InstallGuestToolsRequest {
            vm_name: vm_name.to_string(),
        })
        .await?
        .into_inner();
    if resp.ok {
        Ok(())
    } else {
        anyhow::bail!(resp.message)
    }
}

pub async fn get_firewall_inventory(addr: &str) -> anyhow::Result<machina_core::FirewallInventory> {
    let mut client = connect(addr).await?;
    let resp = timed(
        "get_firewall_inventory",
        client.get_firewall_inventory(GetFirewallInventoryRequest {}),
    )
    .await?
    .into_inner();
    if resp.ok {
        serde_json::from_str(&resp.inventory_json)
            .map_err(|e| anyhow::anyhow!("inventory json: {e}"))
    } else {
        anyhow::bail!(resp.message)
    }
}

pub async fn apply_firewall_plan(
    addr: &str,
    req: &machina_core::FirewallPlanRequest,
    dry_run: bool,
) -> anyhow::Result<machina_core::FirewallPlanResult> {
    let mut client = connect(addr).await?;
    let plan_json = serde_json::to_string(req)?;
    let resp = client
        .apply_firewall_plan(ApplyFirewallPlanRequest { plan_json, dry_run })
        .await?
        .into_inner();
    if resp.ok {
        serde_json::from_str(&resp.result_json).map_err(|e| anyhow::anyhow!("plan json: {e}"))
    } else {
        anyhow::bail!(resp.message)
    }
}

/// One machina-bpfd request on the host behind `addr`; returns bpfd's `data`.
pub async fn bpf_call(
    addr: &str,
    req: &machina_bpf::api::Request,
) -> anyhow::Result<serde_json::Value> {
    let mut client = connect(addr).await?;
    let resp = timed(
        "bpf_call",
        client.bpf_call(BpfCallRequest {
            request_json: serde_json::to_string(req)?,
        }),
    )
    .await?
    .into_inner();
    if resp.ok {
        serde_json::from_str(&resp.data_json).map_err(|e| anyhow::anyhow!("bpf data json: {e}"))
    } else {
        anyhow::bail!(resp.error)
    }
}

/// Reconcile the controller-owned (`owned_prefix`) policy set on one host.
pub async fn bpf_sync_policies(
    addr: &str,
    policies: &[machina_bpf::api::Policy],
    owned_prefix: &str,
) -> anyhow::Result<serde_json::Value> {
    let mut client = connect(addr).await?;
    let resp = timed(
        "bpf_sync_policies",
        client.bpf_sync_policies(BpfSyncPoliciesRequest {
            policies_json: serde_json::to_string(policies)?,
            owned_prefix: owned_prefix.to_string(),
        }),
    )
    .await?
    .into_inner();
    let result: serde_json::Value = serde_json::from_str(&resp.result_json).unwrap_or_default();
    if resp.ok {
        Ok(result)
    } else if resp.message.is_empty() {
        anyhow::bail!("policy sync reported errors: {}", result["errors"])
    } else {
        anyhow::bail!(resp.message)
    }
}

pub struct GuestFirewallPortsResponse {
    pub agent_reachable: bool,
    pub ports: Vec<machina_core::GuestListeningPort>,
}

pub async fn get_guest_firewall_ports(
    addr: &str,
    vm_name: &str,
) -> anyhow::Result<GuestFirewallPortsResponse> {
    let mut client = connect(addr).await?;
    let resp = timed(
        "get_guest_firewall_ports",
        client.get_guest_firewall_ports(GetGuestFirewallPortsRequest {
            vm_name: vm_name.into(),
        }),
    )
    .await?
    .into_inner();
    if resp.ok {
        Ok(GuestFirewallPortsResponse {
            agent_reachable: resp.agent_reachable,
            ports: resp
                .ports
                .into_iter()
                .map(|p| machina_core::GuestListeningPort {
                    // Don't silently truncate an out-of-range agent-reported port
                    // (e.g. 65537 → 1, which would mis-key/mis-match rules). Map
                    // invalid values to 0 as an explicit "invalid port" sentinel.
                    port: u16::try_from(p.port).unwrap_or(0),
                    protocol: p.protocol,
                    bind_address: p.bind_address,
                    process: if p.process.is_empty() {
                        None
                    } else {
                        Some(p.process)
                    },
                })
                .collect(),
        })
    } else {
        anyhow::bail!(resp.message)
    }
}

pub async fn get_firewall_activity(addr: &str, hours: u32) -> anyhow::Result<serde_json::Value> {
    let mut client = connect(addr).await?;
    let resp = timed(
        "get_firewall_activity",
        client.get_firewall_activity(GetFirewallActivityRequest { hours }),
    )
    .await?
    .into_inner();
    if resp.ok {
        serde_json::from_str(&resp.activity_json).map_err(|e| anyhow::anyhow!("activity json: {e}"))
    } else {
        anyhow::bail!(resp.message)
    }
}

pub async fn get_lldp(
    agent_console_addr: &str,
) -> anyhow::Result<machina_core::libvirt::host_network::LldpInventory> {
    let normalized = normalize_agent_addr(agent_console_addr);
    let url = format!("http://{normalized}/api/v1/host/lldp");
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()?
        .get(&url)
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("agent LLDP HTTP {}", resp.status());
    }
    Ok(resp.json().await?)
}

pub async fn get_linux_observability(addr: &str) -> anyhow::Result<serde_json::Value> {
    let mut client = connect(addr).await?;
    let resp = timed(
        "get_linux_observability",
        client.get_linux_observability(GetLinuxObservabilityRequest {}),
    )
    .await?
    .into_inner();
    if resp.ok {
        serde_json::from_str(&resp.json).map_err(|e| anyhow::anyhow!("linux obs json: {e}"))
    } else {
        anyhow::bail!(resp.message)
    }
}

pub async fn get_systemd_network_diagnostics(addr: &str) -> anyhow::Result<serde_json::Value> {
    let mut client = connect(addr).await?;
    let resp = timed(
        "get_systemd_network_diagnostics",
        client.get_systemd_network_diagnostics(GetSystemdNetworkDiagnosticsRequest {}),
    )
    .await?
    .into_inner();
    if resp.ok {
        serde_json::from_str(&resp.json).map_err(|e| anyhow::anyhow!("network diag json: {e}"))
    } else {
        anyhow::bail!(resp.message)
    }
}

pub async fn get_linux_audit(addr: &str) -> anyhow::Result<serde_json::Value> {
    let mut client = connect(addr).await?;
    let resp = timed(
        "get_linux_audit",
        client.get_linux_audit(GetLinuxAuditRequest {}),
    )
    .await?
    .into_inner();
    if resp.ok {
        serde_json::from_str(&resp.json).map_err(|e| anyhow::anyhow!("linux audit json: {e}"))
    } else {
        anyhow::bail!(resp.message)
    }
}

pub async fn get_linux_package_updates(addr: &str) -> anyhow::Result<serde_json::Value> {
    let mut client = connect(addr).await?;
    let resp = timed(
        "get_linux_package_updates",
        client.get_linux_package_updates(GetLinuxPackageUpdatesRequest {}),
    )
    .await?
    .into_inner();
    if resp.ok {
        serde_json::from_str(&resp.json)
            .map_err(|e| anyhow::anyhow!("linux package updates json: {e}"))
    } else {
        anyhow::bail!(resp.message)
    }
}

pub async fn apply_linux_package_upgrade(
    addr: &str,
    dry_run: bool,
) -> anyhow::Result<serde_json::Value> {
    let mut client = connect(addr).await?;
    let resp = client
        .apply_linux_package_upgrade(ApplyLinuxPackageUpgradeRequest { dry_run })
        .await?
        .into_inner();
    if resp.ok {
        let value = serde_json::from_str(&resp.json)
            .unwrap_or_else(|_| serde_json::json!({ "stdout": resp.message, "ok": true }));
        Ok(value)
    } else {
        anyhow::bail!(resp.message)
    }
}

pub async fn host_linux_reboot(addr: &str) -> anyhow::Result<()> {
    let mut client = connect(addr).await?;
    let resp = client
        .host_linux_reboot(HostLinuxRebootRequest {})
        .await?
        .into_inner();
    if resp.ok {
        Ok(())
    } else {
        anyhow::bail!(resp.message)
    }
}

pub async fn get_linux_filesystems(addr: &str) -> anyhow::Result<serde_json::Value> {
    let mut client = connect(addr).await?;
    let resp = timed(
        "get_linux_filesystems",
        client.get_linux_filesystems(GetLinuxFilesystemsRequest {}),
    )
    .await?
    .into_inner();
    if resp.ok {
        serde_json::from_str(&resp.json).map_err(|e| anyhow::anyhow!("linux filesystems json: {e}"))
    } else {
        anyhow::bail!(resp.message)
    }
}

pub async fn get_linux_top_processes(
    addr: &str,
    limit: u32,
    order: &str,
) -> anyhow::Result<serde_json::Value> {
    let mut client = connect(addr).await?;
    let resp = timed(
        "get_linux_top_processes",
        client.get_linux_top_processes(GetLinuxTopProcessesRequest {
            limit,
            order: order.to_string(),
        }),
    )
    .await?
    .into_inner();
    if resp.ok {
        serde_json::from_str(&resp.json).map_err(|e| anyhow::anyhow!("linux processes json: {e}"))
    } else {
        anyhow::bail!(resp.message)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PortForwardRuleDto {
    pub id: String,
    pub protocol: String,
    pub host_port: u16,
    pub vm_ip: String,
    pub vm_port: u16,
    pub description: String,
}

pub async fn list_port_forwards(addr: &str) -> anyhow::Result<Vec<PortForwardRuleDto>> {
    let mut client = connect(addr).await?;
    let resp = timed(
        "list_port_forwards",
        client.list_port_forwards(ListPortForwardsRequest {}),
    )
    .await?
    .into_inner();
    if !resp.message.is_empty() && resp.rules.is_empty() {
        anyhow::bail!(resp.message);
    }
    Ok(resp
        .rules
        .into_iter()
        .map(|r| PortForwardRuleDto {
            id: r.id,
            protocol: r.protocol,
            // Avoid silent u32→u16 truncation of out-of-range ports (would mis-key
            // the port-forward list and its vm_ip/port filtering); 0 = invalid.
            host_port: u16::try_from(r.host_port).unwrap_or(0),
            vm_ip: r.vm_ip,
            vm_port: u16::try_from(r.vm_port).unwrap_or(0),
            description: r.description,
        })
        .collect())
}

pub async fn create_port_forward(
    addr: &str,
    protocol: &str,
    host_port: u16,
    vm_ip: &str,
    vm_port: u16,
    description: &str,
) -> anyhow::Result<()> {
    let mut client = connect(addr).await?;
    let resp = client
        .create_port_forward(CreatePortForwardRequest {
            protocol: protocol.to_string(),
            host_port: host_port as u32,
            vm_ip: vm_ip.to_string(),
            vm_port: vm_port as u32,
            description: description.to_string(),
        })
        .await?
        .into_inner();
    if resp.ok {
        Ok(())
    } else {
        anyhow::bail!(resp.message)
    }
}

pub async fn delete_port_forward(
    addr: &str,
    protocol: &str,
    host_port: u16,
    vm_ip: &str,
    vm_port: u16,
) -> anyhow::Result<()> {
    let mut client = connect(addr).await?;
    let resp = client
        .delete_port_forward(DeletePortForwardRequest {
            protocol: protocol.to_string(),
            host_port: host_port as u32,
            vm_ip: vm_ip.to_string(),
            vm_port: vm_port as u32,
        })
        .await?
        .into_inner();
    if resp.ok {
        Ok(())
    } else {
        anyhow::bail!(resp.message)
    }
}

#[derive(Debug, Clone)]
pub struct LbMemberDto {
    pub vm_ip: String,
    pub port: u16,
    pub weight: u32,
}

pub async fn set_load_balancer(
    addr: &str,
    lb_id: &str,
    protocol: &str,
    host_port: u16,
    members: &[LbMemberDto],
) -> anyhow::Result<()> {
    let mut client = connect(addr).await?;
    let resp = client
        .set_load_balancer(SetLoadBalancerRequest {
            lb_id: lb_id.to_string(),
            protocol: protocol.to_string(),
            host_port: host_port as u32,
            members: members
                .iter()
                .map(|m| LoadBalancerMemberMsg {
                    vm_ip: m.vm_ip.clone(),
                    port: m.port as u32,
                    weight: m.weight,
                })
                .collect(),
        })
        .await?
        .into_inner();
    if resp.ok {
        Ok(())
    } else {
        anyhow::bail!(resp.message)
    }
}

pub async fn delete_load_balancer(
    addr: &str,
    lb_id: &str,
    protocol: &str,
    host_port: u16,
) -> anyhow::Result<()> {
    let mut client = connect(addr).await?;
    let resp = client
        .delete_load_balancer(DeleteLoadBalancerRequest {
            lb_id: lb_id.to_string(),
            protocol: protocol.to_string(),
            host_port: host_port as u32,
        })
        .await?
        .into_inner();
    if resp.ok {
        Ok(())
    } else {
        anyhow::bail!(resp.message)
    }
}

pub async fn list_host_gpus(client: &mut AgentClient) -> anyhow::Result<ListHostGpusResponse> {
    read_rpc(
        "list_host_gpus",
        client.list_host_gpus(ListHostGpusRequest {}),
    )
    .await
}

/// Cloud subnet requests are additive on the wire; old agents reject the new
/// backend, so mixed-version fleets cannot report false success.
pub async fn provision_cloud_subnet(
    client: &mut AgentClient,
    id: uuid::Uuid,
    cidr: &str,
) -> anyhow::Result<()> {
    let response = client
        .provision_network(ProvisionNetworkRequest {
            network_name: format!("mc-{id}"),
            backend: "cloud-isolated".into(),
            vlan_id: 0,
            bridge: String::new(),
            subnet_cidr: cidr.into(),
            subnet_uuid: id.to_string(),
        })
        .await?
        .into_inner();
    anyhow::ensure!(response.ok, "{}", response.message);
    Ok(())
}

// --- FluxVM (the host's fluxvm-api, reached through its agent) ---

pub async fn fluxvm_power(
    client: &mut AgentClient,
    vm_name: &str,
    action: &str,
) -> anyhow::Result<VmPowerResponse> {
    Ok(client
        .vm_power(VmPowerRequest {
            vm_name: vm_name.to_string(),
            action: action.to_string(),
            backend: machina_core::fluxvm::BACKEND_NAME.into(),
            ..Default::default()
        })
        .await?
        .into_inner())
}

pub async fn fluxvm_delete(client: &mut AgentClient, vm_name: &str) -> anyhow::Result<()> {
    client
        .delete_vm(DeleteVmRequest {
            vm_name: vm_name.to_string(),
            backend: machina_core::fluxvm::BACKEND_NAME.into(),
        })
        .await?;
    Ok(())
}

/// Create from a fluxvm-api create body (`{name, backend, image, …}`).
pub async fn fluxvm_apply(
    client: &mut AgentClient,
    create: &serde_json::Value,
    shared_takeover: bool,
) -> anyhow::Result<ApplyVmResponse> {
    Ok(client
        .apply_vm(ApplyVmRequest {
            backend: machina_core::fluxvm::BACKEND_NAME.into(),
            fluxvm_create_json: create.to_string(),
            shared_takeover,
            ..Default::default()
        })
        .await?
        .into_inner())
}

pub async fn fluxvm_export_record(
    client: &mut AgentClient,
    vm_name: &str,
) -> anyhow::Result<ExportFluxvmRecordResponse> {
    read_rpc(
        "export_fluxvm_record",
        client.export_fluxvm_record(ExportFluxvmRecordRequest {
            vm_name: vm_name.to_string(),
        }),
    )
    .await
}

pub async fn fluxvm_prepare_receiver(
    client: &mut AgentClient,
    record_json: &str,
    listen_host: &str,
    advertise_host: &str,
) -> anyhow::Result<PrepareFluxvmReceiverResponse> {
    Ok(client
        .prepare_fluxvm_receiver(PrepareFluxvmReceiverRequest {
            record_json: record_json.to_string(),
            listen_host: listen_host.to_string(),
            advertise_host: advertise_host.to_string(),
        })
        .await?
        .into_inner())
}

pub async fn fluxvm_start_migration(
    client: &mut AgentClient,
    vm_id: &str,
    destination: &str,
    bandwidth_mbps: u64,
    max_downtime_ms: u64,
) -> anyhow::Result<()> {
    client
        .start_fluxvm_migration(StartFluxvmMigrationRequest {
            vm_id: vm_id.to_string(),
            destination: destination.to_string(),
            bandwidth_mbps,
            max_downtime_ms,
        })
        .await?;
    Ok(())
}

pub async fn fluxvm_migration_status(
    client: &mut AgentClient,
    vm_id: &str,
) -> anyhow::Result<GetFluxvmMigrationStatusResponse> {
    read_rpc(
        "get_fluxvm_migration_status",
        client.get_fluxvm_migration_status(GetFluxvmMigrationStatusRequest {
            vm_id: vm_id.to_string(),
        }),
    )
    .await
}

pub async fn fluxvm_finish_migration(client: &mut AgentClient, vm_id: &str) -> anyhow::Result<()> {
    client
        .finish_fluxvm_migration(FinishFluxvmMigrationRequest {
            vm_id: vm_id.to_string(),
        })
        .await?;
    Ok(())
}

pub async fn fluxvm_adopt(
    client: &mut AgentClient,
    receiver_id: &str,
    token: &str,
) -> anyhow::Result<AdoptFluxvmVmResponse> {
    Ok(client
        .adopt_fluxvm_vm(AdoptFluxvmVmRequest {
            receiver_id: receiver_id.to_string(),
            token: token.to_string(),
        })
        .await?
        .into_inner())
}

pub async fn fluxvm_abort(
    client: &mut AgentClient,
    source_vm_id: &str,
    receiver_id: &str,
) -> anyhow::Result<()> {
    client
        .abort_fluxvm_migration(AbortFluxvmMigrationRequest {
            source_vm_id: source_vm_id.to_string(),
            receiver_id: receiver_id.to_string(),
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod host_token_tests {
    use super::*;

    #[tokio::test]
    async fn a_joined_host_uses_its_own_token_and_others_do_not() {
        let pool = crate::engine::test_support::test_pool().await;
        crate::db::query(
            "INSERT INTO hosts (id, hostname, state, agent_grpc_addr, agent_token)
             VALUES (?, 'h-own', 'online', '10.9.9.9:50051', 'mat-own')",
        )
        .bind(uuid::Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();
        assert!(load_host_tokens(&pool).await.unwrap() >= 1);
        assert_eq!(
            token_for("http://10.9.9.9:50051").as_deref(),
            Some("mat-own")
        );
        assert_ne!(token_for("10.8.8.8:50051").as_deref(), Some("mat-own"));
        forget_host_token("10.9.9.9:50051");
        assert_ne!(token_for("10.9.9.9:50051").as_deref(), Some("mat-own"));
    }
}

/// One k3s node operation on a host (see `machina_core::k3s_node`). The agent refuses unless the
/// host opted in with `MACHINA_AGENT_K3S_ENABLED=1`.
#[derive(Debug, Clone, Default)]
pub struct K3sNodeCall {
    pub action: String,
    pub version: String,
    pub server_url: String,
    pub token: String,
    pub node_name: String,
    pub advertise_address: String,
    pub install_url: String,
}

#[derive(Debug, Clone, Default)]
pub struct K3sNodeResult {
    pub node_token: String,
    pub kubeconfig: String,
    pub version: String,
    pub active: bool,
    pub message: String,
}

pub async fn k3s_node(addr: &str, call: K3sNodeCall) -> anyhow::Result<K3sNodeResult> {
    let mut client = connect(addr).await?;
    let resp = client
        .k3s_node(K3sNodeRequest {
            action: call.action,
            version: call.version,
            server_url: call.server_url,
            token: call.token,
            node_name: call.node_name,
            advertise_address: call.advertise_address,
            cluster_cidr: String::new(),
            service_cidr: String::new(),
            install_url: call.install_url,
        })
        .await?
        .into_inner();
    if resp.ok {
        Ok(K3sNodeResult {
            node_token: resp.node_token,
            kubeconfig: resp.kubeconfig,
            version: resp.version,
            active: resp.active,
            message: resp.message,
        })
    } else if resp.output_tail.is_empty() {
        anyhow::bail!(resp.message)
    } else {
        anyhow::bail!("{}: {}", resp.message, resp.output_tail)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReplicateResult {
    pub bytes: u64,
    pub files: u32,
    pub is_dir: bool,
    pub remote_uri: String,
    pub message: String,
    pub manifest_sha256: String,
    pub checksummed: bool,
}

/// Copy a backup to (`push`) or from (`fetch`) an S3-compatible store on the host that holds it, or
/// remove a copy from the store (`delete`, which ignores `local_path`).
pub async fn replicate_backup(
    addr: &str,
    action: &str,
    local_path: &str,
    bucket: &str,
    key: &str,
    endpoint_url: &str,
) -> anyhow::Result<ReplicateResult> {
    let mut client = connect(addr).await?;
    let resp = client
        .replicate_backup(ReplicateBackupRequest {
            action: action.to_string(),
            local_path: local_path.to_string(),
            bucket: bucket.to_string(),
            key: key.to_string(),
            endpoint_url: endpoint_url.to_string(),
        })
        .await?
        .into_inner();
    if resp.ok {
        Ok(ReplicateResult {
            bytes: resp.bytes,
            files: resp.files,
            is_dir: resp.is_dir,
            remote_uri: resp.remote_uri,
            message: resp.message,
            manifest_sha256: resp.manifest_sha256,
            checksummed: resp.checksummed,
        })
    } else {
        anyhow::bail!(resp.message)
    }
}
