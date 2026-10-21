// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Extension, Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};

use std::collections::HashMap;

use machina_core::libvirt::guest_agent::GuestIpAddress;
use machina_core::libvirt::{
    boot, capabilities, cdrom, domain, domain_job, emulator, extras, filesystem, guest_agent,
    guest_agent_provision, guest_health, guest_repair, host_cpu, hostdev_pci, iscsi, mdev,
    migrate, net_xml, network, node_device, numa_tune, nwfilter, save_restore, secret, storage,
};
use machina_core::{LibvirtError, LibvirtManager};

use crate::auth::{require_usb_pci, require_write, RequestActor};
use crate::conn_query::{spawn_libvirt_actor, ConnQuery};
use crate::error::{AppError, Xml};

/// Upper bound on synchronous PTR lookups performed per request. `addrs`
/// comes from the in-guest agent, which can be influenced by whatever is
/// running inside the VM (or a compromised guest) — without a cap, a guest
/// reporting hundreds of bogus IPv4 addresses could tie up a blocking-pool
/// thread for a long time (`dns_lookup::lookup_addr` has no timeout of its
/// own) for every `/interfaces` call.
const MAX_DNS_PTR_LOOKUPS: usize = 32;

fn enrich_dns_ptr(mut addrs: Vec<GuestIpAddress>) -> Vec<GuestIpAddress> {
    use std::net::IpAddr;
    let mut lookups = 0usize;
    for a in &mut addrs {
        if a.ip_type != "ipv4" {
            continue;
        }
        let Ok(ip) = a.address.parse::<IpAddr>() else {
            continue;
        };
        if ip.is_loopback() {
            continue;
        }
        if let IpAddr::V4(v4) = ip {
            if v4.is_link_local() || v4.is_broadcast() {
                continue;
            }
        }
        if lookups >= MAX_DNS_PTR_LOOKUPS {
            break;
        }
        lookups += 1;
        match dns_lookup::lookup_addr(&ip) {
            Ok(name) if name != a.address => a.dns_ptr = Some(name),
            _ => {}
        }
    }
    addrs
}

// ── Guest Agent ─────────────────────────────────────────────────────

async fn get_interfaces(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name2 = name.clone();
    let (addrs, net_gw): (Vec<GuestIpAddress>, HashMap<String, String>) =
        spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
            let mut addrs = guest_agent::get_guest_interfaces(conn, &name2)?;
            let leases = extras::list_dhcp_leases(conn).unwrap_or_default();
            addrs = guest_agent::enrich_with_dhcp_leases(addrs, &leases);
            let mut gateways: HashMap<String, String> = HashMap::new();
            if let Ok(dom_xml) = domain::get_vm_xml(conn, &name2) {
                for n in net_xml::network_names_from_domain_xml(&dom_xml) {
                    if gateways.contains_key(&n) {
                        continue;
                    }
                    if let Ok(nxml) = network::get_network_xml(conn, &n) {
                        if let Some(gw) = net_xml::ipv4_gateway_from_network_xml(&nxml) {
                            gateways.insert(n, gw);
                        }
                    }
                }
            }
            Ok((addrs, gateways))
        })
        .await?;

    let addrs = tokio::task::spawn_blocking(move || enrich_dns_ptr(addrs))
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))?;

    Ok(Json(serde_json::json!({
        "addresses": addrs,
        "queried_at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "network_gateways": net_gw,
    })))
}

async fn get_hostname(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name2 = name.clone();
    let h = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        guest_agent::get_guest_hostname(conn, &name2)
    })
    .await?;
    Ok(Json(serde_json::json!({ "hostname": h })))
}

async fn get_guest_observability(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name2 = name.clone();
    let info = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        guest_agent::get_guest_observability(conn, &name2)
    })
    .await?;
    Ok(Json(serde_json::json!(info)))
}

async fn get_guest_health(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name2 = name.clone();
    let report = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        guest_health::gather_guest_health(conn, &name2)
    })
    .await?;
    Ok(Json(serde_json::json!(report)))
}

// ── Guest agent offline install ─────────────────────────────────────

#[derive(serde::Deserialize, Default)]
struct AgentInjectRequest {
    /// Preview what `guestkit agent-inject` would change without writing.
    #[serde(default)]
    dry_run: bool,
}

/// Inject the GuestKit agent into a powered-off VM's disk. The VM must already be off; the web UI
/// shuts it down first and starts it again afterwards.
async fn inject_guest_agent_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<AgentInjectRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let libvirt_cfg = machina_core::MachinaConfig::load().libvirt;
    let name2 = name.clone();
    let dry_run = req.dry_run;
    let report = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        guest_agent_provision::inject_agent_offline(conn, &name2, Some(&libvirt_cfg), dry_run)
    })
    .await?;
    Ok(Json(serde_json::json!(report)))
}

// ── Boot Doctor: offline guest repair ───────────────────────────────

#[derive(serde::Deserialize, Default)]
struct GuestRepairRequest {
    /// Preview the repair without writing.
    #[serde(default)]
    dry_run: bool,
    /// Back the disk up first (default on; ignored for dry runs).
    #[serde(default = "default_true")]
    backup: bool,
}

fn default_true() -> bool {
    true
}

/// Is GuestKit installed on this hypervisor? The UI checks this before offering offline repair or
/// agent injection so nobody is shut down only to find the tool missing.
async fn guest_repair_capabilities_handler() -> Json<serde_json::Value> {
    let cfg = machina_core::MachinaConfig::load().libvirt;
    Json(serde_json::json!(guest_agent_provision::guestkit_status(
        Some(&cfg)
    )))
}

/// Diagnose a powered-off VM's disk with GuestKit (`doctor --explain`).
async fn diagnose_guest_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let report = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        guest_repair::diagnose_offline(conn, &name2)
    })
    .await?;
    Ok(Json(serde_json::json!(report)))
}

/// Repair a powered-off VM's boot problems with GuestKit (`repair --fix boot`), with a dry-run preview.
async fn repair_guest_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<GuestRepairRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let (dry_run, backup) = (req.dry_run, req.backup);
    let report = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        guest_repair::repair_offline(conn, &name2, dry_run, backup)
    })
    .await?;
    Ok(Json(serde_json::json!(report)))
}

#[derive(serde::Deserialize)]
struct GuestDriftRequest {
    /// The machine to compare against (a golden image VM). Both must be powered off.
    baseline: String,
}

/// How far a powered-off VM's disk has drifted from another powered-off VM's (GuestKit `drift`, read-only).
async fn drift_guest_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<GuestDriftRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let (name2, baseline) = (name.clone(), req.baseline.clone());
    let report = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        guest_repair::drift_offline(conn, &name2, &baseline)
    })
    .await?;
    Ok(Json(serde_json::json!(report)))
}

// ── CD-ROM ──────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct CdromRequest {
    iso_path: String,
    /// Omit to let the daemon pick a free target. The old fixed default of "sda"
    /// collided with the root disk on every SATA guest.
    #[serde(default)]
    target: String,
}

async fn insert_cdrom_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<CdromRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    if conn_q.is_fluxvm() {
        return Err(machina_core::LibvirtError::Invalid(
            "FluxVM attaches install ISOs only at create (fluxvm_isos); an ejected drive can't be refilled".into(),
        )
        .into());
    }
    // Held across the whole read-XML → pick-target → attach/update sequence:
    // two concurrent inserts on the same VM would otherwise both read the same
    // starting XML, pick the same "free" target, and race on the attach.
    let _vm_guard = manager.lock_vm(&name).await;
    let name2 = name.clone();
    let outcome = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        cdrom::insert_cdrom(conn, &name2, &req.iso_path, &req.target)
    })
    .await?;
    // Say plainly when the media is only staged: a SATA drive attached to a
    // running guest is invisible until reboot, and reporting a bare "inserted"
    // sent operators hunting for a CD that was never going to appear.
    let message = if outcome.requires_restart {
        "CD-ROM staged — restart the VM for the guest to see it"
    } else {
        "CD-ROM inserted"
    };
    Ok(Json(serde_json::json!({
        "status": "inserted",
        "name": name,
        "target": outcome.target,
        "bus": outcome.bus,
        "live": outcome.live,
        "requires_restart": outcome.requires_restart,
        "message": message,
    })))
}

async fn detach_cdrom_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path((name, target)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let _vm_guard = manager.lock_vm(&name).await;
    let name2 = name.clone();
    let target2 = target.clone();
    let outcome = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        cdrom::detach_cdrom(conn, &name2, &target2)
    })
    .await?;
    Ok(Json(serde_json::json!({
        "status": "detached",
        "name": name,
        "target": target,
        "live_removed": outcome.live_removed,
    })))
}

async fn eject_cdrom_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path((name, target)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    if conn_q.is_fluxvm() {
        super::fluxvm::client()?.eject_cdrom(&name, &target).await?;
        return Ok(Json(serde_json::json!({
            "status": "ejected",
            "name": name,
            "target": target,
            "backend": machina_core::fluxvm::BACKEND_NAME,
        })));
    }
    let _vm_guard = manager.lock_vm(&name).await;
    let name2 = name.clone();
    let target2 = target.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        cdrom::eject_cdrom(conn, &name2, &target2)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "ejected", "name": name, "target": target }),
    ))
}

// ── Shared directories (virtiofs) ───────────────────────────────────

#[derive(serde::Deserialize)]
struct ShareRequest {
    source_dir: String,
    mount_tag: String,
    #[serde(default)]
    xattr: bool,
}

async fn add_share_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<ShareRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    // add_virtiofs_share/remove_share both do a read-XML → modify → define_xml
    // full-document replace (same shape as the CD-ROM race this lock was
    // introduced for): two concurrent share edits on the same VM would read
    // the same starting XML and the second `define_xml` silently clobbers the
    // first's change instead of erroring.
    let _vm_guard = manager.lock_vm(&name).await;
    let name2 = name.clone();
    let tag = req.mount_tag.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        filesystem::add_virtiofs_share(conn, &name2, &req.source_dir, &req.mount_tag, req.xattr)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "shared", "name": name, "mount_tag": tag }),
    ))
}

async fn remove_share_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path((name, mount_tag)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    // See add_share_handler: same define_xml race.
    let _vm_guard = manager.lock_vm(&name).await;
    let name2 = name.clone();
    let tag2 = mount_tag.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        filesystem::remove_share(conn, &name2, &tag2)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "removed", "name": name, "mount_tag": mount_tag }),
    ))
}

// ── Save/Restore ────────────────────────────────────────────────────

async fn managed_save_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        save_restore::managed_save(conn, &name2)
    })
    .await?;
    Ok(Json(serde_json::json!({ "status": "saved", "name": name })))
}

async fn managed_save_remove_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        save_restore::managed_save_remove(conn, &name2)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "removed", "name": name }),
    ))
}

async fn has_managed_save_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name2 = name.clone();
    let result = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        save_restore::has_managed_save(conn, &name2)
    })
    .await;
    let has_save = result?;
    Ok(Json(
        serde_json::json!({ "name": name, "has_managed_save": has_save }),
    ))
}

// ── Boot ────────────────────────────────────────────────────────────

async fn get_boot_config_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        boot::get_boot_config(conn, &name)
    })
    .await;
    Ok(Json(serde_json::json!(result?)))
}

#[derive(serde::Deserialize)]
struct BootOrderRequest {
    devices: Vec<String>,
}

async fn set_boot_order_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<BootOrderRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    // set_boot_order also does read-XML → modify → define_xml; same lost-update
    // race as the shared-directory handlers above.
    let _vm_guard = manager.lock_vm(&name).await;
    let name2 = name.clone();
    let devices = req.devices.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        boot::set_boot_order(conn, &name2, &devices)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "name": name, "boot_devices": req.devices }),
    ))
}

// ── Migration ───────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct MigrateRequest {
    dest_uri: String,
    #[serde(default)]
    live: bool,
    #[serde(default)]
    parameters: Option<migrate::MigrateParametersApi>,
    /// OR with libvirt flags, e.g. unsafe (512), postcopy (32768), undefine source (16).
    #[serde(default)]
    extra_flags: u32,
    #[serde(default)]
    unsafe_migrate: bool,
    #[serde(default)]
    postcopy: bool,
    #[serde(default)]
    undefine_source: bool,
    #[serde(default)]
    tunnelled: bool,
    #[serde(default)]
    paused: bool,
    /// FluxVM (`?backend=fluxvm`): bearer token for a remote `dest_uri` FluxVM API.
    #[serde(default)]
    dest_token: Option<String>,
    /// FluxVM: target listener bind address (default 127.0.0.1 locally, 0.0.0.0 remote).
    #[serde(default)]
    listen_host: Option<String>,
    /// FluxVM: address the source dials when it differs from `listen_host`.
    #[serde(default)]
    advertise_host: Option<String>,
    #[serde(default)]
    bandwidth_mbps: Option<u64>,
    #[serde(default)]
    max_downtime_ms: Option<u64>,
}

/// FluxVM live migration. `dest_uri` is `local` (host to itself, the same
/// `fluxvm-api`) or another host's FluxVM API URL (`https://host:7788`).
async fn fluxvm_migrate(name: &str, req: &MigrateRequest) -> Result<serde_json::Value, AppError> {
    use machina_core::fluxvm::migrate::{live_migrate, MigrateOptions};
    use machina_core::fluxvm::FluxvmClient;
    let cfg = machina_core::MachinaConfig::load().fluxvm;
    let src = super::fluxvm::client()?;
    let dest = req.dest_uri.trim();
    let local = matches!(dest, "" | "local" | "loopback" | "fluxvm://local")
        || dest.trim_end_matches('/') == cfg.base_url.trim_end_matches('/');
    let dst = if local {
        src.clone()
    } else if dest.starts_with("http://") || dest.starts_with("https://") {
        let mut c = cfg.clone();
        c.base_url = dest.to_string();
        c.token_file.clear();
        c.token = req.dest_token.clone().unwrap_or_default();
        FluxvmClient::from_config(&c)?
    } else {
        return Err(machina_core::LibvirtError::Invalid(
            "FluxVM dest_uri must be `local` or the target's FluxVM API URL (https://host:7788)"
                .into(),
        )
        .into());
    };
    let listen_default = if local { "127.0.0.1" } else { "0.0.0.0" };
    let opts = MigrateOptions {
        listen_host: req
            .listen_host
            .clone()
            .unwrap_or_else(|| listen_default.into()),
        advertise_host: req.advertise_host.clone().unwrap_or_default(),
        bandwidth_mbps: req.bandwidth_mbps,
        max_downtime_ms: req.max_downtime_ms,
        timeout: std::time::Duration::from_secs(900),
    };
    let adopted = live_migrate(&src, &dst, name, &opts).await?;
    Ok(serde_json::json!({
        "status": "migrated",
        "name": name,
        "destination": if local { "local" } else { dest },
        "backend": machina_core::fluxvm::BACKEND_NAME,
        "id": adopted.id,
        "state": machina_core::fluxvm::map_status(&adopted.status),
    }))
}

async fn migrate_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<MigrateRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    // undefine_source destroys the source domain after migrating — that is a
    // destructive/undefine operation, so require admin (matching VM delete/undefine).
    // The dest URI itself is scheme-allowlisted + shell-metachar-rejected in
    // migrate::validate_migrate_uri (called by migrate_vm_uri below).
    if req.undefine_source {
        crate::auth::require_destroy_vm(&actor)?;
    }
    if conn_q.is_fluxvm() {
        return Ok(Json(fluxvm_migrate(&name, &req).await?));
    }
    let name2 = name.clone();
    let dest_uri = req.dest_uri.clone();
    let destination = dest_uri.clone();
    let live = req.live;
    let params = req.parameters.clone();
    let mut xf = req.extra_flags;
    if req.unsafe_migrate {
        xf |= virt::sys::VIR_MIGRATE_UNSAFE;
    }
    if req.postcopy {
        xf |= virt::sys::VIR_MIGRATE_POSTCOPY;
    }
    if req.undefine_source {
        xf |= virt::sys::VIR_MIGRATE_UNDEFINE_SOURCE;
    }
    if req.tunnelled {
        xf |= virt::sys::VIR_MIGRATE_TUNNELLED;
    }
    if req.paused {
        xf |= virt::sys::VIR_MIGRATE_PAUSED;
    }
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        migrate::migrate_vm_uri(conn, &name2, &dest_uri, live, params.as_ref(), xf)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "migrated", "name": name, "destination": destination }),
    ))
}

async fn migrate_get_max_speed_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name2 = name.clone();
    let mib = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        migrate::migrate_get_max_speed(conn, &name2)
    })
    .await;
    Ok(Json(
        serde_json::json!({ "name": name, "mib_per_sec": mib? }),
    ))
}

#[derive(serde::Deserialize)]
struct MigrateBandwidthBody {
    mib_per_sec: u64,
}

async fn migrate_set_max_speed_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<MigrateBandwidthBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let mib = req.mib_per_sec;
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        migrate::migrate_set_max_speed(conn, &name2, mib)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "name": name, "mib_per_sec": mib }),
    ))
}

#[derive(serde::Deserialize)]
struct MigrateDowntimeBody {
    downtime_ns: u64,
}

async fn migrate_set_max_downtime_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<MigrateDowntimeBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let ns = req.downtime_ns;
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        migrate::migrate_set_max_downtime(conn, &name2, ns)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "name": name, "downtime_ns": ns }),
    ))
}

async fn get_numa_tune_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name2 = name.clone();
    let result = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        numa_tune::get_numa_tune(conn, &name2)
    })
    .await;
    Ok(Json(serde_json::json!(result?)))
}

async fn set_numa_tune_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<numa_tune::SetNumaTuneRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let body = req;
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        numa_tune::set_numa_tune(conn, &name2, &body)
    })
    .await?;
    Ok(Json(serde_json::json!({ "status": "ok", "name": name })))
}

#[derive(serde::Deserialize)]
struct EmulatorPinBody {
    cpus: Vec<bool>,
}

async fn pin_emulator_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<EmulatorPinBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let cpus = req.cpus.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        emulator::pin_emulator(conn, &name2, &cpus)
    })
    .await?;
    Ok(Json(serde_json::json!({ "status": "ok", "name": name })))
}

#[derive(serde::Deserialize)]
struct CpuCompareBody {
    cpu_xml: String,
    #[serde(default)]
    flags: u32,
}

async fn compare_cpu_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<CpuCompareBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let xml = req.cpu_xml.clone();
    let flags = req.flags;
    let result = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        host_cpu::compare_cpu(conn, &xml, flags)
    })
    .await;
    Ok(Json(serde_json::json!(result?)))
}

async fn job_info_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name2 = name.clone();
    let result = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        domain_job::job_info(conn, &name2)
    })
    .await;
    Ok(Json(result?))
}

#[derive(serde::Deserialize)]
struct JobStatsQuery {
    #[serde(default)]
    flags: u32,
}

async fn job_stats_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Query(query): Query<JobStatsQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name2 = name.clone();
    let flags = query.flags;
    let result = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        domain_job::job_stats_u32(conn, &name2, flags)
    })
    .await;
    Ok(Json(result?))
}

// ── Capabilities ────────────────────────────────────────────────────

async fn get_capabilities_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = spawn_libvirt_actor(
        manager,
        Some(&actor),
        conn_q,
        capabilities::get_capabilities,
    )
    .await?;
    Ok(Json(serde_json::json!(result)))
}

async fn get_sysinfo_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Xml, AppError> {
    let result =
        spawn_libvirt_actor(manager, Some(&actor), conn_q, capabilities::get_sysinfo).await?;
    Ok(Xml(result))
}

// ── Node Devices ────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct DeviceQuery {
    #[serde(default)]
    capability: Option<String>,
}

async fn list_node_devices_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Query(query): Query<DeviceQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let cap = query.capability.clone();
    let result = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        node_device::list_node_devices(conn, cap.as_deref())
    })
    .await?;
    Ok(Json(serde_json::json!(result)))
}

async fn get_node_device_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Xml, AppError> {
    let result = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        node_device::get_node_device_xml(conn, &name)
    })
    .await;
    Ok(Xml(result?))
}

// ── Network Filters ─────────────────────────────────────────────────

async fn list_nwfilters_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result =
        spawn_libvirt_actor(manager, Some(&actor), conn_q, nwfilter::list_nwfilters).await?;
    Ok(Json(serde_json::json!(result)))
}

async fn get_nwfilter_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Xml, AppError> {
    let result = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        nwfilter::get_nwfilter_xml(conn, &name)
    })
    .await;
    Ok(Xml(result?))
}

async fn delete_nwfilter_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "networks:write")?;
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        nwfilter::delete_nwfilter(conn, &name2)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "deleted", "name": name }),
    ))
}

#[derive(serde::Deserialize)]
struct DefineNwfilterRequest {
    xml: String,
}

async fn define_nwfilter_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<DefineNwfilterRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "networks:write")?;
    let xml = req.xml;
    let name = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        nwfilter::define_nwfilter(conn, &xml)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "defined", "name": name }),
    ))
}

// ── Secrets ─────────────────────────────────────────────────────────

async fn list_secrets_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = spawn_libvirt_actor(manager, Some(&actor), conn_q, secret::list_secrets).await?;
    Ok(Json(serde_json::json!(result)))
}

async fn delete_secret_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(uuid): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "storage:write")?;
    let uuid2 = uuid.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        secret::delete_secret(conn, &uuid2)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "deleted", "uuid": uuid }),
    ))
}

#[derive(serde::Deserialize)]
struct DefineSecretRequest {
    xml: String,
    #[serde(default)]
    value_base64: Option<String>,
    #[serde(default)]
    validate_xml: bool,
    #[serde(default)]
    set_value_flags: u32,
}

async fn define_secret_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<DefineSecretRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "storage:write")?;
    use base64::Engine;
    let value_bytes: Option<Vec<u8>> = if let Some(b64) = &req.value_base64 {
        Some(
            base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .map_err(|e| LibvirtError::Invalid(format!("value_base64: {e}")))?,
        )
    } else {
        None
    };
    let xml = req.xml.clone();
    let validate = req.validate_xml;
    let svf = req.set_value_flags;
    let uuid = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        secret::define_secret_with_value(conn, &xml, value_bytes.as_deref(), validate, svf)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "defined", "uuid": uuid }),
    ))
}

#[derive(serde::Deserialize)]
struct PciHostdevBody {
    pci: String,
}

async fn attach_pci_hostdev_handler(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<PciHostdevBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    require_usb_pci(&actor)?;
    let pci = req.pci.clone();
    let pci_for_task = pci.clone();
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        hostdev_pci::attach_pci_hostdev(conn, &name2, &pci_for_task)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "pci_attached", "name": name, "pci": pci }),
    ))
}

async fn detach_pci_hostdev_handler(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<PciHostdevBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    require_usb_pci(&actor)?;
    let pci = req.pci.clone();
    let pci_for_task = pci.clone();
    let name2 = name.clone();
    let outcome = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        hostdev_pci::detach_pci_hostdev(conn, &name2, &pci_for_task)
    })
    .await?;
    Ok(Json(serde_json::json!({
        "status": "pci_detached",
        "name": name,
        "pci": pci,
        "live_removed": outcome.live_removed,
    })))
}

// ── Mediated devices (vGPU partitions) ──────────────────────────────

#[derive(serde::Deserialize)]
struct MdevCreateBody {
    parent: String,
    #[serde(rename = "type")]
    type_id: String,
    #[serde(default)]
    uuid: Option<String>,
}

#[derive(serde::Deserialize)]
struct MdevUuidBody {
    uuid: String,
}

/// Partition types each mdev-capable GPU offers, and the partitions that exist.
async fn list_mdev_handler(
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_usb_pci(&actor)?;
    let root = std::path::Path::new(mdev::SYSFS_ROOT);
    Ok(Json(serde_json::json!({
        "types": mdev::list_types(root),
        "instances": mdev::list_instances(root),
    })))
}

async fn create_mdev_handler(
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<MdevCreateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    require_usb_pci(&actor)?;
    let uuid = mdev::create(
        std::path::Path::new(mdev::SYSFS_ROOT),
        &req.parent,
        &req.type_id,
        req.uuid.as_deref(),
    )?;
    Ok(Json(serde_json::json!({
        "status": "mdev_created", "uuid": uuid, "parent": req.parent, "type": req.type_id,
    })))
}

async fn delete_mdev_handler(
    Extension(actor): Extension<RequestActor>,
    Path(uuid): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    require_usb_pci(&actor)?;
    mdev::remove(std::path::Path::new(mdev::SYSFS_ROOT), &uuid)?;
    Ok(Json(serde_json::json!({ "status": "mdev_removed", "uuid": uuid })))
}

async fn attach_mdev_handler(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<MdevUuidBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    require_usb_pci(&actor)?;
    let uuid = req.uuid.clone();
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        mdev::attach_mdev(conn, &name2, &uuid)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "mdev_attached", "name": name, "uuid": req.uuid }),
    ))
}

async fn detach_mdev_handler(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<MdevUuidBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    require_usb_pci(&actor)?;
    let uuid = req.uuid.clone();
    let name2 = name.clone();
    let outcome = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        mdev::detach_mdev(conn, &name2, &uuid)
    })
    .await?;
    Ok(Json(serde_json::json!({
        "status": "mdev_detached", "name": name, "uuid": req.uuid,
        "live_removed": outcome.live_removed,
    })))
}

// ── iSCSI storage ───────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct IscsiDiscoverBody {
    host: String,
    #[serde(default)]
    port: Option<u16>,
}

/// Ask an iSCSI portal which targets it offers (runs `iscsiadm` on this host).
async fn iscsi_discover_handler(
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<IscsiDiscoverBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "storage:write")?;
    let port = req.port.unwrap_or(iscsi::DEFAULT_PORT);
    let host = req.host.clone();
    let targets = tokio::task::spawn_blocking(move || iscsi::discover(&host, port))
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(serde_json::json!({
        "portal": format!("{}:{port}", req.host),
        "targets": targets,
    })))
}

#[derive(serde::Deserialize)]
struct CreateIscsiPoolRequest {
    name: String,
    host: String,
    #[serde(default)]
    port: Option<u16>,
    target_iqn: String,
    #[serde(default)]
    initiator_iqn: Option<String>,
    #[serde(default)]
    chap_username: Option<String>,
    #[serde(default)]
    chap_secret: Option<String>,
}

/// Log in to an iSCSI target and expose its LUNs as a storage pool. The CHAP secret becomes a
/// libvirt secret and is never returned.
async fn create_iscsi_pool_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<CreateIscsiPoolRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "storage:write")?;
    let chap = match (req.chap_username.clone(), req.chap_secret.clone()) {
        (Some(username), Some(secret)) => Some(iscsi::Chap { username, secret }),
        (None, None) => None,
        _ => {
            return Err(LibvirtError::Invalid(
                "give both chap_username and chap_secret, or neither".into(),
            )
            .into())
        }
    };
    let spec = iscsi::IscsiSpec {
        name: req.name.clone(),
        host: req.host.clone(),
        port: req.port.unwrap_or(iscsi::DEFAULT_PORT),
        target_iqn: req.target_iqn.clone(),
        initiator_iqn: req.initiator_iqn.clone(),
        chap,
    };
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        iscsi::create_iscsi_pool(conn, &spec)
    })
    .await?;
    Ok(Json(serde_json::json!({
        "status": "created",
        "name": req.name,
        "target_iqn": req.target_iqn,
        "chap": req.chap_username.is_some(),
    })))
}

#[derive(serde::Deserialize)]
struct AttachLunBody {
    pool: String,
    volume: String,
    #[serde(default = "default_lun_target")]
    target: String,
    #[serde(default)]
    bus: String,
    #[serde(default)]
    readonly: bool,
    #[serde(default)]
    shareable: bool,
}

fn default_lun_target() -> String {
    "vdb".to_string()
}

/// Attach one LUN of an iSCSI pool to a VM as a raw block disk. Detach it with the normal disk
/// detach, using the same `target`.
async fn attach_lun_handler(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<AttachLunBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    require_write(&actor, "storage:write")?;
    let (vm, pool, volume, target, bus) = (
        name.clone(),
        req.pool.clone(),
        req.volume.clone(),
        req.target.clone(),
        req.bus.clone(),
    );
    let (readonly, shareable) = (req.readonly, req.shareable);
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        iscsi::attach_lun(conn, &vm, &pool, &volume, &target, &bus, readonly, shareable)
    })
    .await?;
    Ok(Json(serde_json::json!({
        "status": "lun_attached",
        "name": name,
        "pool": req.pool,
        "volume": req.volume,
        "target": req.target,
    })))
}

async fn detach_nodedev_handler(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(conn_q): Query<ConnQuery>,
    Path(devname): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    require_usb_pci(&actor)?;
    let dev = devname.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        node_device::detach_node_device(conn, &dev)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "nodedev_detached", "name": devname }),
    ))
}

async fn reattach_nodedev_handler(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Query(conn_q): Query<ConnQuery>,
    Path(devname): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    require_usb_pci(&actor)?;
    let dev = devname.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        node_device::reattach_node_device(conn, &dev)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "nodedev_reattached", "name": devname }),
    ))
}

// ── Storage Pool Create/Delete ──────────────────────────────────────

#[derive(serde::Deserialize)]
struct CreatePoolRequest {
    name: String,
    #[serde(default = "default_pool_type")]
    pool_type: String,
    target_path: String,
}

fn default_pool_type() -> String {
    "dir".to_string()
}

async fn create_pool_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<CreatePoolRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "storage:write")?;
    let req_name = req.name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        storage::create_pool(conn, &req.name, &req.pool_type, &req.target_path)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "created", "name": req_name }),
    ))
}

async fn delete_pool_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "storage:write")?;
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        storage::delete_pool(conn, &name2)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "deleted", "name": name }),
    ))
}

async fn get_pool_xml_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Xml, AppError> {
    let result = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        storage::get_pool_xml(conn, &name)
    })
    .await;
    Ok(Xml(result?))
}

// ── Volume Resize/Clone ─────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct ResizeVolumeRequest {
    capacity_gb: f64,
}

async fn resize_volume_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path((pool, vol)): Path<(String, String)>,
    Json(req): Json<ResizeVolumeRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "storage:write")?;
    if req.capacity_gb <= 0.0 || req.capacity_gb > 10_240.0 {
        return Err(machina_core::LibvirtError::Operation(
            "capacity_gb must be between 0 and 10240 (10 TB)".to_string(),
        )
        .into());
    }
    let capacity = req.capacity_gb.ceil() as u64;
    let pool2 = pool.clone();
    let vol2 = vol.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        storage::resize_volume(conn, &pool2, &vol2, capacity)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "resized", "pool": pool, "volume": vol, "capacity_gb": capacity }),
    ))
}

#[derive(serde::Deserialize)]
struct CloneVolumeRequest {
    new_name: String,
}

async fn clone_volume_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path((pool, vol)): Path<(String, String)>,
    Json(req): Json<CloneVolumeRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "storage:write")?;
    let pool2 = pool.clone();
    let vol2 = vol.clone();
    let new_name = req.new_name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        storage::clone_volume(conn, &pool2, &vol2, &new_name)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "cloned", "pool": pool, "source": vol, "clone": req.new_name }),
    ))
}

// ── Memory Balloon ──────────────────────────────────────────────────

async fn set_memory_balloon_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path((name, mb)): Path<(String, u64)>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    machina_core::validate::validate_memory_mb(mb)?;
    let name2 = name.clone();
    let outcome = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        machina_core::libvirt::resize::set_memory_balloon(conn, &name2, mb)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "name": name, "memory_mb": mb, "live_applied": outcome.live_applied }),
    ))
}

// ── Routes ──────────────────────────────────────────────────────────

pub fn advanced_routes() -> Router<LibvirtManager> {
    Router::new()
        // Guest agent
        .route("/vms/{name}/interfaces", get(get_interfaces))
        .route("/vms/{name}/hostname", get(get_hostname))
        .route(
            "/vms/{name}/guest-observability",
            get(get_guest_observability),
        )
        .route("/vms/{name}/guest-health", get(get_guest_health))
        .route(
            "/vms/{name}/guest-agent/inject",
            post(inject_guest_agent_handler),
        )
        .route(
            "/vms/{name}/guest-repair/diagnose",
            post(diagnose_guest_handler),
        )
        .route("/vms/{name}/guest-repair/apply", post(repair_guest_handler))
        .route("/vms/{name}/guest-drift", post(drift_guest_handler))
        .route(
            "/guest-repair/capabilities",
            get(guest_repair_capabilities_handler),
        )
        // CD-ROM
        .route("/vms/{name}/cdrom/insert", post(insert_cdrom_handler))
        .route(
            "/vms/{name}/cdrom/eject/{target}",
            post(eject_cdrom_handler),
        )
        .route(
            "/vms/{name}/cdrom/detach/{target}",
            post(detach_cdrom_handler),
        )
        // Shared directories (virtiofs)
        .route("/vms/{name}/share", post(add_share_handler))
        .route(
            "/vms/{name}/share/{mount_tag}",
            delete(remove_share_handler),
        )
        // Save/Restore
        .route("/vms/{name}/managed-save", post(managed_save_handler))
        .route(
            "/vms/{name}/managed-save",
            delete(managed_save_remove_handler),
        )
        .route(
            "/vms/{name}/managed-save/status",
            get(has_managed_save_handler),
        )
        // Boot
        .route("/vms/{name}/boot", get(get_boot_config_handler))
        .route("/vms/{name}/boot", post(set_boot_order_handler))
        // Migration + tuning
        .route("/vms/{name}/migrate", post(migrate_handler))
        .route(
            "/vms/{name}/migrate/max-bandwidth",
            get(migrate_get_max_speed_handler).post(migrate_set_max_speed_handler),
        )
        .route(
            "/vms/{name}/migrate/max-downtime",
            post(migrate_set_max_downtime_handler),
        )
        .route(
            "/vms/{name}/numa",
            get(get_numa_tune_handler).post(set_numa_tune_handler),
        )
        .route("/vms/{name}/emulator/pin", post(pin_emulator_handler))
        .route("/vms/{name}/job", get(job_info_handler))
        .route("/vms/{name}/job/stats", get(job_stats_handler))
        .route("/cpu/compare", post(compare_cpu_handler))
        // Memory balloon
        .route("/vms/{name}/balloon/{mb}", post(set_memory_balloon_handler))
        // Capabilities
        .route("/capabilities", get(get_capabilities_handler))
        .route("/sysinfo", get(get_sysinfo_handler))
        // Node devices
        .route("/devices", get(list_node_devices_handler))
        .route("/devices/{name}", get(get_node_device_handler))
        // Network filters
        .route(
            "/nwfilters",
            get(list_nwfilters_handler).post(define_nwfilter_handler),
        )
        .route("/nwfilters/{name}", get(get_nwfilter_handler))
        .route("/nwfilters/{name}", delete(delete_nwfilter_handler))
        // Secrets
        .route(
            "/secrets",
            get(list_secrets_handler).post(define_secret_handler),
        )
        .route("/secrets/{uuid}", delete(delete_secret_handler))
        // PCI hostdev + node device detach (VFIO prep)
        .route(
            "/vms/{name}/hostdev/pci/attach",
            post(attach_pci_hostdev_handler),
        )
        .route(
            "/vms/{name}/hostdev/pci/detach",
            post(detach_pci_hostdev_handler),
        )
        .route("/host/iscsi/discover", post(iscsi_discover_handler))
        .route("/storage/iscsi-pools", post(create_iscsi_pool_handler))
        .route("/vms/{name}/disks/lun", post(attach_lun_handler))
        .route("/host/mdev", get(list_mdev_handler).post(create_mdev_handler))
        .route("/host/mdev/{uuid}", delete(delete_mdev_handler))
        .route("/vms/{name}/hostdev/mdev/attach", post(attach_mdev_handler))
        .route("/vms/{name}/hostdev/mdev/detach", post(detach_mdev_handler))
        .route("/host/nodedev/{name}/detach", post(detach_nodedev_handler))
        .route(
            "/host/nodedev/{name}/reattach",
            post(reattach_nodedev_handler),
        )
        // Storage pool management
        .route("/storage/pools", post(create_pool_handler))
        .route("/storage/pools/{name}", delete(delete_pool_handler))
        .route("/storage/pools/{name}/xml", get(get_pool_xml_handler))
        // Volume resize/clone
        .route(
            "/storage/pools/{pool}/volumes/{vol}/resize",
            post(resize_volume_handler),
        )
        .route(
            "/storage/pools/{pool}/volumes/{vol}/clone",
            post(clone_volume_handler),
        )
}
