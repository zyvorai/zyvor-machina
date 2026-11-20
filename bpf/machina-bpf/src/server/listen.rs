// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Unix-socket server, maintenance loop and state persistence.

use std::os::unix::fs::PermissionsExt;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use super::readers::{on_capture, on_dns, on_l7, on_net, on_proc, on_ssl, on_tlsfp, spawn_reader};
use super::*;

const FLOW_IDLE_NS: u64 = 120 * 1_000_000_000;
const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 5000;

#[derive(Debug, Clone)]
pub struct Config {
    pub socket: PathBuf,
    pub state_dir: PathBuf,
    /// Group allowed to use the socket (mode 0660); `None` = root only (0600).
    pub socket_group: Option<String>,
}

/// What survives a restart. Enforce mode is deliberately not persisted: a
/// restarted bpfd always comes back in observe (fail-open).
#[derive(Default, Serialize, Deserialize)]
struct Persisted {
    #[serde(default)]
    policies: Vec<Policy>,
    #[serde(default)]
    telemetry: Option<TelemetryConfig>,
    #[serde(default)]
    interfaces: Vec<(String, bool, bool)>,
    #[serde(default)]
    qos_by_vm: Vec<(String, u64, u64)>,
    /// Per-workload traffic totals: (key, totals, window start).
    #[serde(default)]
    accounting: Vec<(String, AcctTotals, String)>,
    /// machina-cni: node config, local pod endpoints, last synced state.
    /// `cni_node` is the pre-dual-stack (address, uplink) form.
    #[serde(default, skip_serializing)]
    cni_node: Option<(String, Option<String>)>,
    #[serde(default)]
    cni_config: Option<CniNodeConfig>,
    #[serde(default)]
    cni_endpoints: Vec<CniEndpoint>,
    #[serde(default)]
    cni_state: Option<CniState>,
    #[serde(default)]
    vm_edge: Option<VmEdgeState>,
    #[serde(default)]
    vm_sandbox: Option<VmSandboxConfig>,
    /// VMs sandboxed by request (re-attached when their scope reappears).
    #[serde(default)]
    vm_sandbox_pinned: Vec<String>,
    #[serde(default)]
    shield: Option<ShieldConfig>,
    #[serde(default)]
    tls: Option<TlsConfig>,
    #[serde(default)]
    rtnl: Option<RtnlConfig>,
    #[serde(default)]
    l7_sample: Option<L7SampleConfig>,
    #[serde(default)]
    vm_intel: Option<VmIntelConfig>,
    #[serde(default)]
    guard: Option<GuardConfig>,
    /// Re-held for the time they have left (wall clock `until`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    vm_quarantines: Vec<VmQuarantine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vm_egress: Option<VmEgressSnat>,
    /// Egress IPs bpfd added, so a restart can still remove them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    vm_egress_managed: Vec<(std::net::IpAddr, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vm_overlay: Option<VmOverlay>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vm_wake: Option<VmWake>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    vm_chaos: Vec<VmChaosActive>,
    /// Root qdisc kinds chaos replaced, by tap.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    vm_chaos_prior: std::collections::BTreeMap<String, String>,
}

struct Daemon {
    engine: Mutex<Engine>,
    shared: SharedState,
    bus: broadcast::Sender<StreamEvent>,
    state_path: PathBuf,
}

impl Daemon {
    fn save(&self, eng: &Engine) {
        let (vm_chaos, vm_chaos_prior) = eng.chaos.persisted();
        let p = Persisted {
            policies: eng.list_policies(),
            telemetry: Some(eng.telemetry.clone()),
            interfaces: eng
                .explicit
                .iter()
                .map(|(n, (g, x))| (n.clone(), *g, *x))
                .collect(),
            qos_by_vm: eng
                .qos_by_vm
                .iter()
                .map(|(v, (e, i))| (v.clone(), *e, *i))
                .collect(),
            accounting: eng
                .acct_base
                .iter()
                .map(|(k, t)| {
                    (
                        k.clone(),
                        *t,
                        eng.acct_since.get(k).cloned().unwrap_or_default(),
                    )
                })
                .collect(),
            cni_node: None,
            cni_config: eng.cni.config.clone(),
            cni_endpoints: eng.cni.endpoints.values().cloned().collect(),
            cni_state: eng.cni.last_sync.is_some().then(|| eng.cni.last.clone()),
            vm_edge: (!eng.vm_edge.state.vms.is_empty()).then(|| eng.vm_edge.state.clone()),
            vm_sandbox: Some(eng.sandbox.config.clone()),
            vm_sandbox_pinned: {
                let mut v: Vec<String> = eng.sandbox.pinned.iter().cloned().collect();
                v.sort();
                v
            },
            shield: (eng.shield.config != ShieldConfig::default())
                .then(|| eng.shield.config.clone()),
            tls: (eng.tls.config != TlsConfig::default()).then(|| eng.tls.config.clone()),
            rtnl: (eng.rtnl.config != RtnlConfig::default()).then(|| eng.rtnl.config.clone()),
            l7_sample: (eng.l7s.config != L7SampleConfig::default())
                .then(|| eng.l7s.config.clone()),
            vm_intel: (eng.vmi.config != VmIntelConfig::default()).then(|| eng.vmi.config.clone()),
            guard: eng.guard_persisted(),
            vm_quarantines: eng.vm_quarantines(),
            vm_egress: (!eng.egress.config.rules.is_empty()).then(|| eng.egress.config.clone()),
            vm_egress_managed: eng.egress.managed.clone(),
            vm_overlay: eng
                .overlay
                .config
                .enabled
                .then(|| eng.overlay.config.clone()),
            vm_wake: {
                let w = eng.wake.config();
                (!w.entries.is_empty()).then_some(w)
            },
            vm_chaos,
            vm_chaos_prior,
        };
        let tmp = self.state_path.with_extension("json.tmp");
        let res = serde_json::to_vec_pretty(&p)
            .map_err(anyhow::Error::from)
            .and_then(|b| std::fs::write(&tmp, b).map_err(Into::into))
            .and_then(|_| std::fs::rename(&tmp, &self.state_path).map_err(Into::into));
        if let Err(e) = res {
            tracing::warn!("persist {}: {e:#}", self.state_path.display());
        }
    }

    fn restore(&self, eng: &mut Engine) {
        let Ok(bytes) = std::fs::read(&self.state_path) else {
            return;
        };
        let p: Persisted = match serde_json::from_slice(&bytes) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("ignoring corrupt {}: {e}", self.state_path.display());
                return;
            }
        };
        for (k, t, since) in p.accounting {
            eng.acct_base.insert(k.clone(), t);
            eng.acct_since.insert(k, since);
        }
        if let Some(t) = p.telemetry {
            if let Err(e) = eng.set_telemetry(t) {
                tracing::warn!("restore telemetry: {e:#}");
            }
        }
        for (name, g, x) in p.interfaces {
            eng.explicit.insert(name.clone(), (g, x));
            if let Err(e) = eng.attach_interface(&name, g, x) {
                tracing::info!("restore interface {name}: {e:#}");
            }
        }
        for (vm, e, i) in p.qos_by_vm {
            eng.qos_by_vm.insert(vm.clone(), (e, i));
            let _ = eng.set_qos(None, Some(&vm), e, i);
        }
        for pol in p.policies {
            let id = pol.id.clone();
            if let Err(e) = eng.apply_policy(pol) {
                tracing::warn!("restore policy {id}: {e:#}");
            }
        }
        // Pods keep running across a bpfd restart: restore their datapath.
        let cfg = p.cni_config.or_else(|| {
            p.cni_node.map(|(node_addr, uplink)| CniNodeConfig {
                node_addr,
                uplink,
                ..Default::default()
            })
        });
        if let Some(cfg) = cfg {
            if let Err(e) = eng.cni_configure(cfg) {
                tracing::warn!("restore cni node: {e:#}");
            }
        }
        if let Some(st) = p.cni_state {
            if let Err(e) = eng.cni_sync(st) {
                tracing::warn!("restore cni state: {e:#}");
            }
        }
        for ep in p.cni_endpoints {
            let ip = ep.ip.clone();
            if let Err(e) = eng.cni_add_endpoint(ep) {
                tracing::info!("restore cni endpoint {ip}: {e:#}");
            }
        }
        // Sandbox mode is restored as configured, but enforcement still needs
        // a fresh lease, so a restarted bpfd only observes.
        if let Some(cfg) = p.vm_sandbox {
            eng.sandbox.config = cfg;
        }
        eng.sandbox.pinned = p.vm_sandbox_pinned.into_iter().collect();
        eng.egress.managed = p.vm_egress_managed;
        if p.vm_egress.is_some() || !eng.egress.managed.is_empty() {
            if let Err(e) = eng.vm_egress_set(p.vm_egress.unwrap_or_default()) {
                tracing::warn!("restore egress SNAT: {e:#}");
            }
        }
        if let Some(cfg) = p.vm_overlay {
            if let Err(e) = eng.vm_overlay_set(cfg) {
                tracing::warn!("restore overlay: {e:#}");
            }
        }
        if let Some(cfg) = p.vm_wake {
            if let Err(e) = eng.wake.set(cfg) {
                tracing::warn!("restore wake set: {e:#}");
            }
        }
        eng.chaos.restore(p.vm_chaos, p.vm_chaos_prior);
        for q in p.vm_quarantines {
            let vm = q.vm.clone();
            if let Err(e) = eng.quarantine_restore(q) {
                tracing::warn!("restore quarantine of {vm}: {e:#}");
            }
        }
        if let Some(st) = p.vm_edge {
            if let Err(e) = eng.vm_edge_sync(st) {
                tracing::warn!("restore vm edge: {e:#}");
            }
        }
        if let Some(cfg) = p.tls {
            if let Err(e) = eng.tls_configure(cfg) {
                tracing::warn!("restore tls: {e:#}");
            }
        }
        if let Some(cfg) = p.shield {
            if let Err(e) = eng.shield_configure(cfg) {
                tracing::warn!("restore shield: {e:#}");
            }
        }
        if let Some(cfg) = p.rtnl {
            if let Err(e) = eng.rtnl_configure(cfg) {
                tracing::warn!("restore rtnl: {e:#}");
            }
        }
        if let Some(cfg) = p.vm_intel {
            if let Err(e) = eng.vmi_configure(cfg) {
                tracing::warn!("restore vm intel: {e:#}");
            }
        }
        if let Some(cfg) = p.guard {
            if let Err(e) = eng.guard_configure(cfg) {
                tracing::warn!("restore vmm guard: {e:#}");
            }
        }
        if let Some(cfg) = p.l7_sample {
            if let Err(e) = eng.l7s_configure(cfg) {
                tracing::warn!("restore l7 sampling: {e:#}");
            }
        }
    }

    fn handle(&self, req: Request, fd: Option<std::os::fd::OwnedFd>) -> Response {
        let r = match req {
            Request::AfxdpRegister { iface, queue } => lock(&self.engine)
                .afxdp_register(&iface, queue, fd)
                .map(|s| v(&s)),
            req => self.dispatch(req),
        };
        match r {
            Ok(v) => Response::ok(v),
            Err(e) => Response::err(format!("{e:#}")),
        }
    }

    fn dispatch(&self, req: Request) -> Result<serde_json::Value> {
        let lim = |l: Option<usize>| l.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        Ok(match req {
            Request::Status => v(&lock(&self.engine).status()),
            Request::ListPolicies => v(&lock(&self.engine).list_policies()),
            Request::ApplyPolicy { policy } => {
                let mut eng = lock(&self.engine);
                let p = eng.apply_policy(policy)?;
                self.save(&eng);
                v(&p)
            }
            Request::RemovePolicy { id } => {
                let mut eng = lock(&self.engine);
                let removed = eng.remove_policy(&id)?;
                if !removed {
                    return Err(anyhow!("policy {id} not found"));
                }
                self.save(&eng);
                json!({ "removed": id })
            }
            Request::SetMode { mode, lease_secs } => {
                v(&lock(&self.engine).set_mode(mode, lease_secs)?)
            }
            Request::ListInterfaces => v(&lock(&self.engine).interfaces()),
            Request::AttachInterface {
                name,
                guest_side,
                xdp,
            } => {
                let mut eng = lock(&self.engine);
                eng.attach_interface(&name, guest_side, xdp)?;
                self.save(&eng);
                v(&eng.interfaces())
            }
            Request::DetachInterface { name } => {
                let mut eng = lock(&self.engine);
                eng.detach_interface(&name);
                self.save(&eng);
                json!({ "detached": name })
            }
            Request::Flows { limit, vm } => {
                v(&lock(&self.engine).flows(lim(limit), vm.as_deref())?)
            }
            Request::Events { limit, kind } => {
                let s = lock(&self.shared);
                let out: Vec<&NetEventRecord> = s
                    .net
                    .iter()
                    .rev()
                    .filter(|e| kind.as_deref().is_none_or(|k| e.kind == k))
                    .take(lim(limit))
                    .collect();
                v(&out)
            }
            Request::Dns { limit } => {
                let s = lock(&self.shared);
                let out: Vec<&DnsRecord> = s.dns.iter().rev().take(lim(limit)).collect();
                v(&out)
            }
            Request::L7 {
                limit,
                vm,
                protocol,
            } => {
                let s = lock(&self.shared);
                let out: Vec<&L7Record> =
                    s.l7.iter()
                        .rev()
                        .filter(|e| vm.is_none() || e.vm == vm)
                        .filter(|e| protocol.as_deref().is_none_or(|p| e.protocol == p))
                        .take(lim(limit))
                        .collect();
                v(&out)
            }
            Request::Accounting { vm } => v(&lock(&self.engine).accounting(vm.as_deref())?),
            Request::ResetAccounting { vm } => {
                let mut eng = lock(&self.engine);
                let n = eng.reset_accounting(vm.as_deref())?;
                self.save(&eng);
                json!({ "reset": n })
            }
            Request::ProcEvents { limit, kind } => {
                let s = lock(&self.shared);
                let out: Vec<&ProcRecord> = s
                    .procs
                    .iter()
                    .rev()
                    .filter(|e| kind.as_deref().is_none_or(|k| e.kind == k))
                    .take(lim(limit))
                    .collect();
                v(&out)
            }
            Request::Anomalies { limit } => {
                let s = lock(&self.shared);
                let out: Vec<&Anomaly> = s.anomalies.iter().rev().take(lim(limit)).collect();
                v(&out)
            }
            Request::NetHealth => v(&lock(&self.engine).net_health()?),
            Request::CaptureStart {
                iface,
                duration_secs,
                sample,
                snaplen,
                max_packets,
            } => v(&lock(&self.engine).capture_start(
                &iface,
                duration_secs,
                sample,
                snaplen,
                max_packets,
            )?),
            Request::CaptureList => {
                let s = lock(&self.shared);
                let mut out: Vec<&CaptureInfo> = s.captures.values().map(|c| &c.info).collect();
                out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
                v(&out)
            }
            Request::CaptureGet { id } => {
                let s = lock(&self.shared);
                let c = s
                    .captures
                    .get(&id)
                    .ok_or_else(|| anyhow!("capture {id} not found"))?;
                json!({
                    "info": c.info,
                    "pcapng_base64": base64::engine::general_purpose::STANDARD.encode(c.writer.as_bytes()),
                })
            }
            Request::SetQos {
                iface,
                vm,
                egress_bps,
                ingress_bps,
            } => {
                let mut eng = lock(&self.engine);
                let names =
                    eng.set_qos(iface.as_deref(), vm.as_deref(), egress_bps, ingress_bps)?;
                self.save(&eng);
                json!({ "interfaces": names, "egress_bps": egress_bps, "ingress_bps": ingress_bps })
            }
            Request::GetTelemetry => v(&lock(&self.engine).telemetry),
            Request::SetTelemetry { telemetry } => {
                let mut eng = lock(&self.engine);
                let t = eng.set_telemetry(telemetry)?;
                self.save(&eng);
                v(&t)
            }
            Request::CniConfigure { config } => {
                let mut eng = lock(&self.engine);
                let st = eng.cni_configure(config)?;
                self.save(&eng);
                v(&st)
            }
            Request::CniAddEndpoint { endpoint } => {
                let mut eng = lock(&self.engine);
                eng.cni_add_endpoint(endpoint)?;
                self.save(&eng);
                v(&eng.cni_status())
            }
            Request::CniDelEndpoint { ip } => {
                let mut eng = lock(&self.engine);
                let removed = eng.cni_del_endpoint(&ip)?;
                self.save(&eng);
                json!({ "removed": removed, "ip": ip })
            }
            Request::CniSync { state } => {
                let mut eng = lock(&self.engine);
                if eng.cni.last_sync.is_some() && eng.cni.last == state {
                    return Ok(v(&eng.cni_status()));
                }
                let st = eng.cni_sync(state)?;
                self.save(&eng);
                v(&st)
            }
            Request::CniStatus => v(&lock(&self.engine).cni_status()),
            Request::CniServices => v(&lock(&self.engine).cni_services()),
            Request::VmEdgeSync { state } => {
                let mut eng = lock(&self.engine);
                let st = eng.vm_edge_sync(state)?;
                self.save(&eng);
                v(&st)
            }
            Request::VmEdgeStatus => v(&lock(&self.engine).vm_edge_status()),
            Request::VmFqdnCache => v(&lock(&self.engine).vm_fqdn_cache()),
            Request::VmAuthTable => v(&lock(&self.engine).vm_auth_table()),
            Request::VmAuthIdentity => v(&lock(&self.engine).vmauth.identity()?),
            Request::VmAuthCert {
                host_id,
                ca_pem,
                cert_pem,
                not_after,
            } => v(&lock(&self.engine)
                .vmauth
                .install(host_id, ca_pem, cert_pem, not_after)?),
            Request::VmAuthProbe { host_id, address } => {
                let id = lock(&self.engine)
                    .vmauth
                    .probe_identity()
                    .ok_or_else(|| anyhow!("no host certificate"))?;
                json!({ "ok": true, "detail": super::vmauth::probe(&id, &host_id, &address)? })
            }
            Request::VmFlows { limit, vm, verdict } => {
                let s = lock(&self.shared);
                let out: Vec<&VmFlowRecord> = s
                    .vm_flows
                    .iter()
                    .rev()
                    .filter(|f| {
                        vm.as_deref().is_none_or(|v| {
                            f.vm == v
                                || f.src_vm.as_deref() == Some(v)
                                || f.dst_vm.as_deref() == Some(v)
                        })
                    })
                    .filter(|f| {
                        verdict
                            .as_deref()
                            .is_none_or(|x| f.verdict.eq_ignore_ascii_case(x))
                    })
                    .take(lim(limit))
                    .collect();
                v(&out)
            }
            Request::VmFlowEdges { vm } => v(&lock(&self.shared).flow_hist.edges(vm.as_deref())),
            Request::VmFlowEdgesReset {} => {
                let mut s = lock(&self.shared);
                s.flow_hist.reset();
                s.flow_hist.save();
                json!({ "ok": true })
            }
            Request::VmFlowAlerts { limit } => {
                let s = lock(&self.shared);
                let out: Vec<&VmFlowAlert> =
                    s.flow_hist.alerts.iter().rev().take(lim(limit)).collect();
                v(&out)
            }
            Request::VmQuarantine {
                vm,
                secs,
                allow,
                reason,
                by,
            } => {
                let mut eng = lock(&self.engine);
                let q = eng.vm_quarantine(&vm, secs, allow, reason, by)?;
                self.save(&eng);
                v(&q)
            }
            Request::VmQuarantineRelease { vm } => {
                let mut eng = lock(&self.engine);
                let released = eng.vm_quarantine_release(&vm)?;
                self.save(&eng);
                json!({ "released": released, "vm": vm })
            }
            Request::VmQuarantines => v(&lock(&self.engine).vm_quarantines()),
            Request::VmThreatFeedSet {
                name,
                source,
                block,
                domains,
            } => v(&lock(&self.engine).vm_threat_feed_set(&name, source, block, domains)?),
            Request::VmThreatFeedRemove { name } => {
                let removed = lock(&self.engine).vm_threat_feed_remove(&name)?;
                json!({ "removed": removed, "name": name })
            }
            Request::VmThreatFeeds => v(&lock(&self.engine).vm_threat_status()),
            Request::VmEgressSnatSet { config } => {
                let mut eng = lock(&self.engine);
                let st = eng.vm_egress_set(config)?;
                self.save(&eng);
                v(&st)
            }
            Request::VmEgressSnatStatus => v(&lock(&self.engine).vm_egress_status()),
            Request::VmOverlaySet { config } => {
                let mut eng = lock(&self.engine);
                let st = eng.vm_overlay_set(config)?;
                self.save(&eng);
                v(&st)
            }
            Request::VmOverlayStatus => v(&lock(&self.engine).vm_overlay_status()),
            Request::VmWakeSet { config } => {
                let eng = lock(&self.engine);
                let st = eng.wake.set(config)?;
                self.save(&eng);
                v(&st)
            }
            Request::VmWakeStatus => v(&lock(&self.engine).wake.status()),
            Request::VmChaosStart { fault } => {
                let eng = lock(&self.engine);
                let st = eng.chaos.start(fault)?;
                self.save(&eng);
                v(&st)
            }
            Request::VmChaosStop { id, prefix } => {
                let eng = lock(&self.engine);
                let stopped = eng.chaos.stop(&id, &prefix);
                self.save(&eng);
                json!({ "stopped": stopped })
            }
            Request::VmChaosStatus => v(&lock(&self.engine).chaos.status()),
            Request::VmSandboxConfigure { config } => {
                let mut eng = lock(&self.engine);
                let st = eng.vm_sandbox_configure(config)?;
                self.save(&eng);
                v(&st)
            }
            Request::VmSandboxAttach { vm, cgroup } => {
                let mut eng = lock(&self.engine);
                let st = eng.vm_sandbox_attach(&vm, cgroup.as_deref())?;
                self.save(&eng);
                v(&st)
            }
            Request::VmSandboxDetach { vm } => {
                let mut eng = lock(&self.engine);
                let detached = eng.vm_sandbox_detach(&vm);
                self.save(&eng);
                json!({ "detached": detached, "vm": vm })
            }
            Request::VmSandboxStatus => v(&lock(&self.engine).vm_sandbox_status()),
            Request::ShieldConfigure { config } => {
                let mut eng = lock(&self.engine);
                let st = eng.shield_configure(config)?;
                self.save(&eng);
                v(&st)
            }
            Request::ShieldStatus => v(&lock(&self.engine).shield_status()),
            Request::NodeIsoConfigure { config } => {
                v(&lock(&self.engine).nodeiso_configure(config)?)
            }
            Request::NodeIsoStatus => v(&lock(&self.engine).nodeiso_status()),
            Request::IcmpErrors => v(&lock(&self.engine).icmp_errors()),
            Request::TlsConfigure { config } => {
                let mut eng = lock(&self.engine);
                let st = eng.tls_configure(config)?;
                self.save(&eng);
                v(&st)
            }
            Request::TlsStatus => v(&lock(&self.engine).tls_status()),
            Request::TlsFingerprints { limit } => {
                v(&super::tls::recent(&lock(&self.shared).tls_fp, limit))
            }
            Request::SslEvents { limit } => v(&super::tls::recent(&lock(&self.shared).ssl, limit)),
            Request::VmRefresh => {
                let mut eng = lock(&self.engine);
                eng.vm_edge_refresh();
                eng.sandbox_refresh();
                eng.vmi_refresh_now();
                json!({ "refreshed": true })
            }
            Request::RtnlConfigure { config } => {
                let mut eng = lock(&self.engine);
                let st = eng.rtnl_configure(config)?;
                self.save(&eng);
                v(&st)
            }
            Request::RtnlStatus => v(&lock(&self.engine).rtnl_status()),
            Request::RtnlEvents { limit, iface } => {
                let s = lock(&self.shared);
                let out: Vec<&RtnlRecord> = s
                    .rtnl
                    .iter()
                    .rev()
                    .filter(|r| iface.is_none() || r.iface == iface)
                    .take(lim(limit))
                    .collect();
                v(&out)
            }
            Request::L7SampleConfigure { config } => {
                let mut eng = lock(&self.engine);
                let st = eng.l7s_configure(config)?;
                self.save(&eng);
                v(&st)
            }
            Request::L7SampleStatus => v(&lock(&self.engine).l7s_status()),
            Request::VmIntelConfigure { config } => {
                let mut eng = lock(&self.engine);
                let st = eng.vmi_configure(config)?;
                self.save(&eng);
                v(&st)
            }
            Request::VmIntelStatus => v(&lock(&self.engine).vmi_status()),
            Request::VmIntelVm { name } => v(&lock(&self.engine).vmi_vm(&name)?),
            Request::GuardConfigure { config } => {
                let mut eng = lock(&self.engine);
                let st = eng.guard_configure(config)?;
                self.save(&eng);
                v(&st)
            }
            Request::GuardStatus => v(&lock(&self.engine).guard_status()),
            Request::DirectConfigure { config } => v(&lock(&self.engine).direct_configure(config)?),
            Request::DirectStatus => v(&lock(&self.engine).direct_status()),
            Request::QuicLbConfigure { config } => v(&lock(&self.engine).quiclb_configure(config)?),
            Request::QuicLbStatus => v(&lock(&self.engine).quiclb_status()),
            Request::AfxdpConfigure { config } => v(&lock(&self.engine).afxdp_configure(config)?),
            Request::AfxdpRegister { .. } => {
                return Err(anyhow!("afxdp_register needs an SCM_RIGHTS socket"))
            }
            Request::AfxdpUnregister { iface, queue } => {
                v(&lock(&self.engine).afxdp_unregister(&iface, queue)?)
            }
            Request::AfxdpStatus => v(&lock(&self.engine).afxdp_status()),
            Request::ScxConfigure { config } => v(&lock(&self.engine).scx_configure(config)?),
            Request::ScxStatus => v(&lock(&self.engine).scx_status()),
            Request::BlackBoxList => v(&super::blackbox::list()),
            Request::BlackBoxGet { vm } => v(&super::blackbox::get(&vm)),
            Request::BlackBoxTrigger { vm, post_secs, reason } => {
                v(&super::blackbox::trigger(&vm, post_secs, reason))
            }
            Request::BlackBoxClear { vm } => {
                json!({ "vm": vm, "cleared": super::blackbox::clear(&vm) })
            }
            Request::GuardEvents { limit } => {
                let s = lock(&self.shared);
                let out: Vec<&GuardRecord> = s.guard.iter().rev().take(lim(limit)).collect();
                v(&out)
            }
            Request::Subscribe { .. } => {
                return Err(anyhow!("subscribe is handled per connection"))
            }
        })
    }
}

fn v<T: Serialize + ?Sized>(x: &T) -> serde_json::Value {
    serde_json::to_value(x).unwrap_or(serde_json::Value::Null)
}

async fn write_line<T: Serialize>(w: &mut UnixStream, v: &T) -> Result<()> {
    let mut b = serde_json::to_vec(v)?;
    b.push(b'\n');
    w.write_all(&b).await?;
    Ok(())
}

/// One `recvmsg`: bytes plus any SCM_RIGHTS fds (queued in arrival order).
fn recv_with_fds(
    fd: std::os::fd::RawFd,
    fds: &mut VecDeque<std::os::fd::OwnedFd>,
) -> std::io::Result<Vec<u8>> {
    use std::os::fd::FromRawFd;
    let mut buf = vec![0u8; 65536];
    let mut cbuf = [0u64; 16];
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    };
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cbuf.as_mut_ptr().cast();
    msg.msg_controllen = std::mem::size_of_val(&cbuf) as _;
    let n = unsafe { libc::recvmsg(fd, &mut msg, libc::MSG_CMSG_CLOEXEC | libc::MSG_DONTWAIT) };
    if n < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut c = unsafe { libc::CMSG_FIRSTHDR(&msg) };
    while !c.is_null() {
        let h = unsafe { &*c };
        if h.cmsg_level == libc::SOL_SOCKET && h.cmsg_type == libc::SCM_RIGHTS {
            let data = unsafe { libc::CMSG_DATA(c) } as *const libc::c_int;
            let count = (h.cmsg_len as usize - unsafe { libc::CMSG_LEN(0) } as usize)
                / std::mem::size_of::<libc::c_int>();
            for i in 0..count {
                fds.push_back(unsafe {
                    std::os::fd::OwnedFd::from_raw_fd(std::ptr::read_unaligned(data.add(i)))
                });
            }
        }
        c = unsafe { libc::CMSG_NXTHDR(&msg, c) };
    }
    buf.truncate(n as usize);
    Ok(buf)
}

async fn serve_conn(d: Arc<Daemon>, mut stream: UnixStream) -> Result<()> {
    use std::os::fd::AsRawFd;
    let mut pending: Vec<u8> = Vec::new();
    let mut fds: VecDeque<std::os::fd::OwnedFd> = VecDeque::new();
    loop {
        let Some(nl) = pending.iter().position(|b| *b == b'\n') else {
            stream.readable().await?;
            let raw = stream.as_raw_fd();
            match stream.try_io(tokio::io::Interest::READABLE, || {
                recv_with_fds(raw, &mut fds)
            }) {
                Ok(b) if b.is_empty() => return Ok(()),
                Ok(b) => pending.extend_from_slice(&b),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.into()),
            }
            continue;
        };
        let line: Vec<u8> = pending.drain(..=nl).collect();
        let line = String::from_utf8_lossy(&line);
        if line.trim().is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                write_line(&mut stream, &Response::err(format!("bad request: {e}"))).await?;
                continue;
            }
        };
        if let Request::Subscribe { topics } = req {
            let mut rx = d.bus.subscribe();
            write_line(&mut stream, &Response::ok(json!({ "subscribed": topics }))).await?;
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        if topics.is_empty() || topics.contains(&ev.topic) {
                            write_line(&mut stream, &ev).await?;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::debug!("subscriber lagged by {n} events");
                    }
                    Err(broadcast::error::RecvError::Closed) => return Ok(()),
                }
            }
        }
        let fd = matches!(req, Request::AfxdpRegister { .. })
            .then(|| fds.pop_front())
            .flatten();
        let d2 = d.clone();
        let resp = tokio::task::spawn_blocking(move || d2.handle(req, fd)).await?;
        write_line(&mut stream, &resp).await?;
    }
}

fn bind_socket(cfg: &Config) -> Result<UnixListener> {
    if let Some(dir) = cfg.socket.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let _ = std::fs::remove_file(&cfg.socket);
    let l = UnixListener::bind(&cfg.socket)
        .with_context(|| format!("bind {}", cfg.socket.display()))?;
    let mut mode = 0o600;
    if let Some(group) = &cfg.socket_group {
        let c = std::ffi::CString::new(group.as_str())?;
        let gr = unsafe { libc::getgrnam(c.as_ptr()) };
        if gr.is_null() {
            tracing::warn!("socket group {group} not found; socket stays root-only");
        } else {
            let gid = unsafe { (*gr).gr_gid };
            let p = std::ffi::CString::new(cfg.socket.as_os_str().as_encoded_bytes())?;
            if unsafe { libc::chown(p.as_ptr(), 0, gid) } == 0 {
                mode = 0o660;
            }
        }
    }
    std::fs::set_permissions(&cfg.socket, std::fs::Permissions::from_mode(mode))?;
    Ok(l)
}

fn spawn_maintenance(d: Arc<Daemon>, wake: Arc<Notify>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let mut n: u64 = 0;
        loop {
            tokio::select! {
                _ = tick.tick() => { n += 1; }
                _ = wake.notified() => {}
            }
            let d2 = d.clone();
            let n2 = n;
            let res = tokio::task::spawn_blocking(move || -> Result<()> {
                let mut eng = lock(&d2.engine);
                eng.drain_dns_blocks()?;
                eng.vm_fqdn_tick()?;
                eng.threat_tick()?;
                eng.vm_auth_tick()?;
                eng.expire_lease()?;
                eng.nodeiso_expire()?;
                eng.guard_expire()?;
                eng.scx_tick();
                eng.sweep_captures()?;
                if n2.is_multiple_of(5) {
                    eng.rescan_ifaces();
                }
                if n2.is_multiple_of(10) {
                    eng.refresh_workloads();
                    eng.sweep_flows(FLOW_IDLE_NS)?;
                }
                if n2.is_multiple_of(60) {
                    eng.fold_accounting()?;
                    d2.save(&eng);
                    lock(&d2.shared).flow_hist.save();
                }
                Ok(())
            })
            .await;
            match res {
                Ok(Err(e)) => tracing::warn!("maintenance: {e:#}"),
                Err(e) => tracing::error!("maintenance task: {e}"),
                _ => {}
            }
        }
    });
}

/// Run machina-bpfd until SIGTERM/SIGINT. Dropping the engine detaches every
/// program, so stopping the service always fails open.
pub async fn run(cfg: Config) -> Result<()> {
    std::fs::create_dir_all(&cfg.state_dir).ok();
    let shared: SharedState = Arc::new(Mutex::new(Shared::default()));
    lock(&shared)
        .flow_hist
        .load(&cfg.state_dir.join("flow-history.json"));
    let (bus, _) = broadcast::channel(4096);
    let wake = Arc::new(Notify::new());

    let mut eng = Engine::new(shared.clone(), bus.clone())?;
    eng.vmauth.set_dir(cfg.state_dir.join("auth"));
    eng.threat_set_path(cfg.state_dir.join("threat-feeds.json"));
    eng.overlay.dir = Some(cfg.state_dir.join("overlay"));
    eng.wake.spawn_poller(bus.clone());
    eng.chaos.spawn_ticker();
    {
        let (sh, b) = (shared.clone(), bus.clone());
        spawn_reader(eng.dp.take_ringbuf("NET_EVENTS")?, "net", move |x| {
            on_net(&sh, &b, x)
        });
        let (sh, b, w) = (shared.clone(), bus.clone(), wake.clone());
        spawn_reader(eng.dp.take_ringbuf("DNS_EVENTS")?, "dns", move |x| {
            on_dns(&sh, &b, &w, x)
        });
        let (sh, b) = (shared.clone(), bus.clone());
        spawn_reader(eng.dp.take_ringbuf("PROC_EVENTS")?, "proc", move |x| {
            on_proc(&sh, &b, x)
        });
        let (sh, b) = (shared.clone(), bus.clone());
        spawn_reader(eng.dp.take_ringbuf("L7_EVENTS")?, "l7", move |x| {
            on_l7(&sh, &b, x)
        });
        let sh = shared.clone();
        spawn_reader(
            eng.dp.take_ringbuf("CAPTURE_EVENTS")?,
            "capture",
            move |x| on_capture(&sh, x),
        );
        let (sh, b) = (shared.clone(), bus.clone());
        spawn_reader(eng.dp.take_ringbuf("TLSFP_EVENTS")?, "tlsfp", move |x| {
            on_tlsfp(&sh, &b, x)
        });
        let (sh, b) = (shared.clone(), bus.clone());
        spawn_reader(eng.dp.take_ringbuf("SSL_EVENTS")?, "ssl", move |x| {
            on_ssl(&sh, &b, x)
        });
        let (sh, b) = (shared.clone(), bus.clone());
        spawn_reader(eng.dp.take_ringbuf("RTNL_EVENTS")?, "rtnl", move |x| {
            super::rtnl::on_rtnl(&sh, &b, x)
        });
        let (sh, b) = (shared.clone(), bus.clone());
        spawn_reader(eng.dp.take_ringbuf("L7S_EVENTS")?, "l7s", move |x| {
            super::l7sample::on_l7s(&sh, &b, x)
        });
        let (sh, b) = (shared.clone(), bus.clone());
        spawn_reader(eng.dp.take_ringbuf("GUARD_EVENTS")?, "guard", move |x| {
            super::guard::on_guard(&sh, &b, x)
        });
        let (sh, b, w) = (shared.clone(), bus.clone(), wake.clone());
        spawn_reader(eng.dp.take_ringbuf("VM_FLOW_EVENTS")?, "vmflow", move |x| {
            super::vm::on_vm_flow(&sh, &b, &w, x)
        });
        let (sh, b) = (shared.clone(), bus.clone());
        let (flows, srcs) = (
            eng.dp.take_hash("VM_PROXY_FLOW")?,
            eng.dp.take_hash("VM_PROXY_SRC")?,
        );
        eng.vmproxy.init(shared.clone(), bus.clone(), flows, srcs);
        let mut l7 = super::vml7::L7Worker::new(eng.dp.take_hash("VM_L7_FLOW")?);
        spawn_reader(eng.dp.take_ringbuf("VM_L7_EVENTS")?, "vml7", move |x| {
            l7.on_event(&sh, &b, x)
        });
    }

    let d = Arc::new(Daemon {
        engine: Mutex::new(eng),
        shared,
        bus,
        state_path: cfg.state_dir.join("bpfd-state.json"),
    });
    {
        let mut eng = lock(&d.engine);
        d.restore(&mut eng);
        eng.threat_restore();
        eng.rescan_ifaces();
        let st = eng.status();
        tracing::info!(
            interfaces = st.interfaces.len(),
            tracepoints = st.tracepoints.len(),
            policies = st.policies_total,
            "machina-bpfd datapath ready"
        );
        for n in &st.notes {
            tracing::warn!("{n}");
        }
    }

    let listener = bind_socket(&cfg)?;
    tracing::info!("machina-bpfd listening on {}", cfg.socket.display());
    spawn_maintenance(d.clone(), wake);

    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        tokio::select! {
            r = listener.accept() => {
                let (stream, _) = r?;
                let d = d.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve_conn(d, stream).await {
                        tracing::debug!("connection: {e:#}");
                    }
                });
            }
            _ = term.recv() => break,
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    lock(&d.shared).flow_hist.save();
    tracing::info!("machina-bpfd stopping; detaching programs");
    let _ = std::fs::remove_file(&cfg.socket);
    Ok(())
}
