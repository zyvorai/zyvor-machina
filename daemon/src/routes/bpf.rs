// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Native eBPF datapath on this host, via the local machina-bpfd socket.

use std::convert::Infallible;

use axum::extract::{Extension, Path, Query};
use axum::http::header;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, put};
use axum::{Json, Router};
use base64::Engine as _;
use futures_util::Stream;
use machina_bpf::api::{
    AfxdpConfig, DirectConfig, GuardConfig, L7SampleConfig, Mode, NodeIsoConfig, Policy,
    QuicLbConfig, Request, RtnlConfig, ScxConfig, ShieldConfig, TelemetryConfig, TlsConfig,
    VmEdgeState, VmIntelConfig, VmSandboxConfig,
};
use machina_bpf::BpfdClient;
use machina_core::{LibvirtError, LibvirtManager};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequestActor;
use crate::error::AppError;

pub(super) fn require_admin(actor: &RequestActor, what: &str) -> Result<(), AppError> {
    if actor.role.is_admin() {
        Ok(())
    } else {
        Err(LibvirtError::Forbidden(format!("{what} requires the admin role.")).into())
    }
}

async fn bpfd(req: Request) -> Result<Json<Value>, AppError> {
    BpfdClient::from_env()
        .call(&req)
        .await
        .map(Json)
        .map_err(|e| LibvirtError::Operation(format!("{e:#}")).into())
}

async fn status() -> Json<Value> {
    let client = BpfdClient::from_env();
    match client.call(&Request::Status).await {
        Ok(v) => Json(v),
        Err(e) => Json(json!({
            "available": false,
            "socket": client.socket_path(),
            "error": format!("{e:#}"),
            "probe": machina_core::bpf_probe::probe_bpf_summary(),
        })),
    }
}

async fn list_policies() -> Result<Json<Value>, AppError> {
    bpfd(Request::ListPolicies).await
}

async fn apply_policy(
    Extension(actor): Extension<RequestActor>,
    Json(policy): Json<Policy>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing runtime enforcement policies")?;
    bpfd(Request::ApplyPolicy { policy }).await
}

async fn remove_policy(
    Extension(actor): Extension<RequestActor>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing runtime enforcement policies")?;
    bpfd(Request::RemovePolicy { id }).await
}

#[derive(Deserialize)]
struct ModeBody {
    mode: Mode,
    lease_secs: Option<u64>,
}

async fn set_mode(
    Extension(actor): Extension<RequestActor>,
    Json(b): Json<ModeBody>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Switching enforcement mode")?;
    bpfd(Request::SetMode {
        mode: b.mode,
        lease_secs: b.lease_secs,
    })
    .await
}

async fn list_interfaces() -> Result<Json<Value>, AppError> {
    bpfd(Request::ListInterfaces).await
}

#[derive(Deserialize)]
struct AttachBody {
    name: String,
    #[serde(default)]
    guest_side: bool,
    #[serde(default)]
    xdp: bool,
}

async fn attach_interface(
    Extension(actor): Extension<RequestActor>,
    Json(b): Json<AttachBody>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Attaching the eBPF datapath")?;
    bpfd(Request::AttachInterface {
        name: b.name,
        guest_side: b.guest_side,
        xdp: b.xdp,
    })
    .await
}

async fn detach_interface(
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Detaching the eBPF datapath")?;
    bpfd(Request::DetachInterface { name }).await
}

#[derive(Deserialize)]
struct ListQuery {
    limit: Option<usize>,
    kind: Option<String>,
    vm: Option<String>,
    protocol: Option<String>,
}

async fn l7(Query(q): Query<ListQuery>) -> Result<Json<Value>, AppError> {
    bpfd(Request::L7 {
        limit: q.limit,
        vm: q.vm,
        protocol: q.protocol,
    })
    .await
}

async fn accounting(Query(q): Query<ListQuery>) -> Result<Json<Value>, AppError> {
    bpfd(Request::Accounting { vm: q.vm }).await
}

#[derive(Deserialize, Default)]
struct ResetBody {
    vm: Option<String>,
}

async fn reset_accounting(
    Extension(actor): Extension<RequestActor>,
    body: Option<Json<ResetBody>>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Resetting traffic accounting")?;
    let vm = body.and_then(|Json(b)| b.vm);
    bpfd(Request::ResetAccounting { vm }).await
}

async fn flows(Query(q): Query<ListQuery>) -> Result<Json<Value>, AppError> {
    bpfd(Request::Flows {
        limit: q.limit,
        vm: q.vm,
    })
    .await
}

async fn events(Query(q): Query<ListQuery>) -> Result<Json<Value>, AppError> {
    bpfd(Request::Events {
        limit: q.limit,
        kind: q.kind,
    })
    .await
}

async fn dns(Query(q): Query<ListQuery>) -> Result<Json<Value>, AppError> {
    bpfd(Request::Dns { limit: q.limit }).await
}

async fn processes(Query(q): Query<ListQuery>) -> Result<Json<Value>, AppError> {
    bpfd(Request::ProcEvents {
        limit: q.limit,
        kind: q.kind,
    })
    .await
}

async fn anomalies(Query(q): Query<ListQuery>) -> Result<Json<Value>, AppError> {
    bpfd(Request::Anomalies { limit: q.limit }).await
}

async fn net_health() -> Result<Json<Value>, AppError> {
    bpfd(Request::NetHealth).await
}

#[derive(Deserialize)]
struct CaptureBody {
    iface: String,
    duration_secs: Option<u64>,
    sample: Option<u32>,
    snaplen: Option<u32>,
    max_packets: Option<usize>,
}

async fn capture_start(
    Extension(actor): Extension<RequestActor>,
    Json(b): Json<CaptureBody>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Starting a packet capture")?;
    bpfd(Request::CaptureStart {
        iface: b.iface,
        duration_secs: b.duration_secs,
        sample: b.sample,
        snaplen: b.snaplen,
        max_packets: b.max_packets,
    })
    .await
}

async fn capture_list() -> Result<Json<Value>, AppError> {
    bpfd(Request::CaptureList).await
}

/// pcapng download (opens in Wireshark).
async fn capture_get(
    Extension(actor): Extension<RequestActor>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    require_admin(&actor, "Downloading packet captures")?;
    let Json(v) = bpfd(Request::CaptureGet { id: id.clone() }).await?;
    let b64 = v["pcapng_base64"].as_str().unwrap_or("");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| LibvirtError::Internal(format!("invalid capture payload: {e}")))?;
    let safe: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    Ok((
        [
            (header::CONTENT_TYPE, "application/x-pcapng".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"machina-{safe}.pcapng\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Deserialize)]
struct QosBody {
    iface: Option<String>,
    vm: Option<String>,
    #[serde(default)]
    egress_bps: u64,
    #[serde(default)]
    ingress_bps: u64,
}

async fn set_qos(
    Extension(actor): Extension<RequestActor>,
    Json(b): Json<QosBody>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing VM bandwidth limits")?;
    bpfd(Request::SetQos {
        iface: b.iface,
        vm: b.vm,
        egress_bps: b.egress_bps,
        ingress_bps: b.ingress_bps,
    })
    .await
}

async fn get_telemetry() -> Result<Json<Value>, AppError> {
    bpfd(Request::GetTelemetry).await
}

async fn set_telemetry(
    Extension(actor): Extension<RequestActor>,
    Json(telemetry): Json<TelemetryConfig>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing eBPF telemetry")?;
    bpfd(Request::SetTelemetry { telemetry }).await
}

#[derive(Deserialize)]
struct StreamQuery {
    /// Comma-separated: net, dns, l7, proc, anomaly. Empty = all.
    topics: Option<String>,
}

/// Live events as Server-Sent Events.
async fn stream(
    Query(q): Query<StreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AppError> {
    let topics: Vec<String> = q
        .topics
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect();
    let refs: Vec<&str> = topics.iter().map(String::as_str).collect();
    let rx = BpfdClient::from_env()
        .subscribe(&refs)
        .await
        .map_err(|e| LibvirtError::Operation(format!("{e:#}")))?;
    let events =
        futures_util::StreamExt::map(tokio_stream::wrappers::ReceiverStream::new(rx), |ev| {
            Ok(Event::default().event(ev.topic).data(ev.event.to_string()))
        });
    Ok(Sse::new(events).keep_alive(KeepAlive::default()))
}

async fn vm_edge_status() -> Result<Json<Value>, AppError> {
    bpfd(Request::VmEdgeStatus).await
}

async fn vm_edge_sync(
    Extension(actor): Extension<RequestActor>,
    Json(state): Json<VmEdgeState>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing VM edge isolation")?;
    bpfd(Request::VmEdgeSync { state }).await
}

async fn vm_sandbox_status() -> Result<Json<Value>, AppError> {
    bpfd(Request::VmSandboxStatus).await
}

async fn vm_sandbox_configure(
    Extension(actor): Extension<RequestActor>,
    Json(config): Json<VmSandboxConfig>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing the QEMU sandbox")?;
    bpfd(Request::VmSandboxConfigure { config }).await
}

#[derive(Deserialize, Default)]
struct SandboxAttachBody {
    cgroup: Option<String>,
}

async fn vm_sandbox_attach(
    Extension(actor): Extension<RequestActor>,
    Path(vm): Path<String>,
    body: Option<Json<SandboxAttachBody>>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing the QEMU sandbox")?;
    let cgroup = body.and_then(|Json(b)| b.cgroup);
    bpfd(Request::VmSandboxAttach { vm, cgroup }).await
}

async fn vm_sandbox_detach(
    Extension(actor): Extension<RequestActor>,
    Path(vm): Path<String>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing the QEMU sandbox")?;
    bpfd(Request::VmSandboxDetach { vm }).await
}

async fn tls_status() -> Result<Json<Value>, AppError> {
    bpfd(Request::TlsStatus).await
}

async fn tls_configure(
    Extension(actor): Extension<RequestActor>,
    Json(config): Json<TlsConfig>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing TLS fingerprinting / OpenSSL capture")?;
    bpfd(Request::TlsConfigure { config }).await
}

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<usize>,
}

async fn tls_fingerprints(Query(q): Query<LimitQuery>) -> Result<Json<Value>, AppError> {
    bpfd(Request::TlsFingerprints { limit: q.limit }).await
}

async fn ssl_events(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<LimitQuery>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Reading OpenSSL HTTP metadata")?;
    bpfd(Request::SslEvents { limit: q.limit }).await
}

async fn icmp_errors() -> Result<Json<Value>, AppError> {
    bpfd(Request::IcmpErrors).await
}

async fn shield_status() -> Result<Json<Value>, AppError> {
    bpfd(Request::ShieldStatus).await
}

async fn shield_configure(
    Extension(actor): Extension<RequestActor>,
    Json(config): Json<ShieldConfig>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing the XDP DDoS shield")?;
    bpfd(Request::ShieldConfigure { config }).await
}

async fn node_iso_status() -> Result<Json<Value>, AppError> {
    bpfd(Request::NodeIsoStatus).await
}

async fn node_iso_configure(
    Extension(actor): Extension<RequestActor>,
    Json(config): Json<NodeIsoConfig>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Isolating the node")?;
    bpfd(Request::NodeIsoConfigure { config }).await
}

async fn cni_status() -> Result<Json<Value>, AppError> {
    bpfd(Request::CniStatus).await
}

async fn cni_services() -> Result<Json<Value>, AppError> {
    bpfd(Request::CniServices).await
}

#[derive(Deserialize)]
struct RtnlQuery {
    limit: Option<usize>,
    iface: Option<String>,
}

async fn rtnl_status() -> Result<Json<Value>, AppError> {
    bpfd(Request::RtnlStatus).await
}

async fn rtnl_configure(
    Extension(actor): Extension<RequestActor>,
    Json(config): Json<RtnlConfig>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing the network change audit")?;
    bpfd(Request::RtnlConfigure { config }).await
}

async fn rtnl_events(Query(q): Query<RtnlQuery>) -> Result<Json<Value>, AppError> {
    bpfd(Request::RtnlEvents {
        limit: q.limit,
        iface: q.iface,
    })
    .await
}

async fn l7_sample_status() -> Result<Json<Value>, AppError> {
    bpfd(Request::L7SampleStatus).await
}

async fn l7_sample_configure(
    Extension(actor): Extension<RequestActor>,
    Json(config): Json<L7SampleConfig>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing L7 sampling")?;
    bpfd(Request::L7SampleConfigure { config }).await
}

async fn vm_intel_status() -> Result<Json<Value>, AppError> {
    bpfd(Request::VmIntelStatus).await
}

async fn vm_intel_configure(
    Extension(actor): Extension<RequestActor>,
    Json(config): Json<VmIntelConfig>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing VM runtime intelligence")?;
    bpfd(Request::VmIntelConfigure { config }).await
}

async fn vm_intel_vm(Path(name): Path<String>) -> Result<Json<Value>, AppError> {
    bpfd(Request::VmIntelVm { name }).await
}

async fn guard_status() -> Result<Json<Value>, AppError> {
    bpfd(Request::GuardStatus).await
}

async fn guard_configure(
    Extension(actor): Extension<RequestActor>,
    Json(config): Json<GuardConfig>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing the VMM guard")?;
    bpfd(Request::GuardConfigure { config }).await
}

async fn guard_events(Query(q): Query<LimitQuery>) -> Result<Json<Value>, AppError> {
    bpfd(Request::GuardEvents { limit: q.limit }).await
}

async fn direct_status() -> Result<Json<Value>, AppError> {
    bpfd(Request::DirectStatus).await
}

async fn direct_configure(
    Extension(actor): Extension<RequestActor>,
    Json(config): Json<DirectConfig>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing direct tap redirects")?;
    bpfd(Request::DirectConfigure { config }).await
}

async fn quic_lb_status() -> Result<Json<Value>, AppError> {
    bpfd(Request::QuicLbStatus).await
}

async fn quic_lb_configure(
    Extension(actor): Extension<RequestActor>,
    Json(config): Json<QuicLbConfig>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing the QUIC load balancer")?;
    bpfd(Request::QuicLbConfigure { config }).await
}

async fn afxdp_status() -> Result<Json<Value>, AppError> {
    bpfd(Request::AfxdpStatus).await
}

async fn afxdp_configure(
    Extension(actor): Extension<RequestActor>,
    Json(config): Json<AfxdpConfig>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing the AF_XDP fast path")?;
    bpfd(Request::AfxdpConfigure { config }).await
}

async fn scx_status() -> Result<Json<Value>, AppError> {
    bpfd(Request::ScxStatus).await
}

async fn scx_configure(
    Extension(actor): Extension<RequestActor>,
    Json(config): Json<ScxConfig>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing the sched_ext scheduler")?;
    bpfd(Request::ScxConfigure { config }).await
}


#[derive(Deserialize, Default)]
struct BlackBoxTriggerBody {
    post_secs: Option<u64>,
    #[serde(default)]
    reason: String,
}

async fn blackbox_list() -> Result<Json<Value>, AppError> {
    bpfd(Request::BlackBoxList).await
}

async fn blackbox_get(Path(vm): Path<String>) -> Result<Json<Value>, AppError> {
    bpfd(Request::BlackBoxGet { vm }).await
}

async fn blackbox_trigger(
    Extension(actor): Extension<RequestActor>,
    Path(vm): Path<String>,
    body: Option<Json<BlackBoxTriggerBody>>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Triggering a VM Black Box capture")?;
    let body = body.map(|Json(v)| v).unwrap_or_default();
    bpfd(Request::BlackBoxTrigger {
        vm,
        post_secs: body.post_secs,
        reason: body.reason,
    })
    .await
}

async fn blackbox_clear(
    Extension(actor): Extension<RequestActor>,
    Path(vm): Path<String>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Clearing a VM Black Box capture")?;
    bpfd(Request::BlackBoxClear { vm }).await
}

/// After a VM lifecycle change, have bpfd re-follow VM taps and QEMU scopes
/// now instead of on its next rescan. Best-effort: bpfd may not be running.
pub fn notify_vm_lifecycle() {
    tokio::spawn(async {
        if let Err(e) = BpfdClient::from_env().call(&Request::VmRefresh).await {
            tracing::debug!("bpfd vm refresh: {e:#}");
        }
    });
    super::netpol::trigger_resync();
}

pub fn bpf_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/bpf/status", get(status))
        .route("/bpf/policies", get(list_policies).post(apply_policy))
        .route("/bpf/policies/{id}", delete(remove_policy))
        .route("/bpf/mode", put(set_mode))
        .route(
            "/bpf/interfaces",
            get(list_interfaces).post(attach_interface),
        )
        .route("/bpf/interfaces/{name}", delete(detach_interface))
        .route("/bpf/flows", get(flows))
        .route("/bpf/events", get(events))
        .route("/bpf/dns", get(dns))
        .route("/bpf/l7", get(l7))
        .route("/bpf/accounting", get(accounting))
        .route(
            "/bpf/accounting/reset",
            axum::routing::post(reset_accounting),
        )
        .route("/bpf/processes", get(processes))
        .route("/bpf/anomalies", get(anomalies))
        .route("/bpf/health", get(net_health))
        .route("/bpf/captures", get(capture_list).post(capture_start))
        .route("/bpf/captures/{id}", get(capture_get))
        .route("/bpf/qos", put(set_qos))
        .route("/bpf/telemetry", get(get_telemetry).put(set_telemetry))
        .route("/bpf/stream", get(stream))
        .route("/bpf/vm-edge", get(vm_edge_status).put(vm_edge_sync))
        .route("/bpf/shield", get(shield_status).put(shield_configure))
        .route("/bpf/icmp-errors", get(icmp_errors))
        .route(
            "/bpf/node-iso",
            get(node_iso_status).put(node_iso_configure),
        )
        .route("/bpf/cni", get(cni_status))
        .route("/bpf/cni/services", get(cni_services))
        .route("/bpf/rtnl", get(rtnl_status).put(rtnl_configure))
        .route("/bpf/rtnl/events", get(rtnl_events))
        .route(
            "/bpf/l7-sample",
            get(l7_sample_status).put(l7_sample_configure),
        )
        .route(
            "/bpf/vm-intel",
            get(vm_intel_status).put(vm_intel_configure),
        )
        .route("/bpf/vm-intel/vms/{name}", get(vm_intel_vm))
        .route("/bpf/guard", get(guard_status).put(guard_configure))
        .route("/bpf/guard/events", get(guard_events))
        .route("/bpf/direct", get(direct_status).put(direct_configure))
        .route("/bpf/quic-lb", get(quic_lb_status).put(quic_lb_configure))
        .route("/bpf/afxdp", get(afxdp_status).put(afxdp_configure))
        .route("/bpf/scx", get(scx_status).put(scx_configure))
        .route("/bpf/blackbox", get(blackbox_list))
        .route(
            "/bpf/blackbox/{vm}",
            get(blackbox_get).delete(blackbox_clear),
        )
        .route(
            "/bpf/blackbox/{vm}/trigger",
            axum::routing::post(blackbox_trigger),
        )
        .route("/bpf/tls", get(tls_status).put(tls_configure))
        .route("/bpf/tls/fingerprints", get(tls_fingerprints))
        .route("/bpf/tls/ssl", get(ssl_events))
        .route(
            "/bpf/vm-sandbox",
            get(vm_sandbox_status).put(vm_sandbox_configure),
        )
        .route(
            "/bpf/vm-sandbox/{vm}",
            axum::routing::post(vm_sandbox_attach).delete(vm_sandbox_detach),
        )
}
