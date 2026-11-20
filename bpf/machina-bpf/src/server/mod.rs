// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! machina-bpfd: the single owner of the host eBPF datapath.
//!
//! Serves newline-delimited JSON ([`crate::api`]) on a Unix socket. One engine
//! task owns the [`Datapath`]; ring-buffer readers feed a shared event store,
//! the anomaly detectors and any live subscribers.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::sync::{Mutex, MutexGuard};
use tokio::io::{unix::AsyncFd, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;

use machina_bpf_common::*;

use crate::anomaly::{Ctx, Detector};
use crate::api::*;
use crate::attribution::{self, CgroupCache};
use crate::loader::{self, mono_to_epoch_us, mono_to_rfc3339, Datapath};
use crate::pcapng::PcapngWriter;
use crate::policy::{self, proto_name, Prefix, Rule};
use crate::{dns, fmt_addr};

mod afxdp;
mod blackbox;
mod chaos;
mod cni;
mod direct;
mod egress;
mod flowhist;
mod guard;
mod l7sample;
mod listen;
mod nodeiso;
mod ops;
mod overlay;
mod quiclb;
mod readers;
mod rtnl;
mod scx;
mod shield;
mod tcp;
mod tls;
mod uplink;
mod vm;
mod vmauth;
mod vmintel;
mod vml7;
mod vmproxy;
mod wake;

pub use listen::{run, Config};

const NET_STORE_CAP: usize = 5000;
const PROC_STORE_CAP: usize = 5000;
const DNS_STORE_CAP: usize = 2000;
const L7_STORE_CAP: usize = 5000;
const TLS_STORE_CAP: usize = 2000;
const SSL_STORE_CAP: usize = 2000;
const ANOMALY_STORE_CAP: usize = 1000;
const VM_FLOW_STORE_CAP: usize = 5000;
const SOCK_PROGS: &[&str] = &[
    "mn_cg_connect4",
    "mn_cg_connect6",
    "mn_cg_sendmsg4",
    "mn_cg_sendmsg6",
];

// ---------------------------------------------------------------------------
// Shared state (read by ring-buffer readers, engine and request handlers)
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct IfaceMeta {
    name: String,
    vm: Option<String>,
}

struct ActiveCapture {
    info: CaptureInfo,
    ifindex: u32,
    writer: PcapngWriter,
    deadline: Instant,
}

#[derive(Default)]
struct Shared {
    ifaces: HashMap<u32, IfaceMeta>,
    /// policy id → match label, for annotating events.
    policy_labels: HashMap<u32, String>,
    /// DNS deny policies: policy num → (suffix, scope) for resolve-and-block.
    dns_denies: HashMap<u32, (String, u32)>,
    net: VecDeque<NetEventRecord>,
    procs: VecDeque<ProcRecord>,
    dns: VecDeque<DnsRecord>,
    l7: VecDeque<L7Record>,
    tls_fp: VecDeque<TlsFingerprint>,
    ssl: VecDeque<SslRecord>,
    /// Fingerprint ClientHellos seen by the tap L7 path too.
    fp_from_l7: bool,
    anomalies: VecDeque<Anomaly>,
    captures: HashMap<String, ActiveCapture>,
    counters: Counters,
    detector: Detector,
    cgroups: CgroupCache,
    /// Resolved DNS-deny answers awaiting the engine: (scope, prefix, policy num).
    dns_block_queue: Vec<(u32, Prefix, u32)>,
    /// Pod UID → (namespace, name), from kubepods joined with CNI endpoints.
    pods: HashMap<String, (String, String)>,
    /// CNI host veth → (namespace, name).
    pod_ifaces: HashMap<String, (String, String)>,
    rtnl: VecDeque<RtnlRecord>,
    rtnl_host_netns: Option<u64>,
    l7s: l7sample::L7sCounts,
    guard: VecDeque<GuardRecord>,
    /// Guarded cgroup id → VM.
    guard_cgroups: HashMap<u64, String>,
    vm_flow_index: vm::FlowIndex,
    vm_flows: VecDeque<VmFlowRecord>,
    flow_hist: flowhist::FlowHistory,
    /// `toFQDNs` patterns of the VM edge state, for the DNS reader.
    vm_fqdn_patterns: Vec<String>,
    /// DNS answers awaiting the engine.
    vm_fqdn_queue: Vec<vm::FqdnLearn>,
    /// Threat feed domain → (feed, block), for the DNS reader.
    vm_threat: HashMap<String, (String, bool)>,
    /// Blocking threat answers awaiting the engine.
    vm_threat_queue: Vec<vm::ThreatHit>,
    /// L7 rules in force (static plus learned `toFQDNs`), for the L7 reader.
    vm_l7_rules: Arc<Vec<VmEdgeL7Rule>>,
    vm_l7_gen: u64,
    /// Send side of the L7 reinject veth (0 = none).
    vm_l7_inject: u32,
    /// Identity pairs (subject, peer) waiting for authentication.
    vm_auth_queue: Vec<(u32, u32)>,
    /// Source addresses seen on each VM's tap.
    vm_learned: vm::learned::Learned,
}

impl Shared {
    fn iface(&self, ifindex: u32) -> (Option<String>, Option<String>) {
        match self.ifaces.get(&ifindex) {
            Some(m) => (Some(m.name.clone()), m.vm.clone()),
            None => (None, None),
        }
    }

    /// Workload behind a datapath interface: the VM on a tap, the pod on a
    /// CNI host veth.
    fn iface_workload(&self, ifindex: u32) -> Option<Workload> {
        let m = self.ifaces.get(&ifindex)?;
        if let Some(vm) = &m.vm {
            return Some(Workload {
                kind: "vm".into(),
                ns: None,
                name: vm.clone(),
            });
        }
        self.pod_ifaces.get(&m.name).map(|(ns, name)| Workload {
            kind: "pod".into(),
            ns: Some(ns.clone()),
            name: name.clone(),
        })
    }

    fn cgroup_workload(&self, path: &str) -> Option<Workload> {
        attribution::cgroup_workload(path, &self.pods)
    }

    fn push_capped<T>(q: &mut VecDeque<T>, item: T, cap: usize) {
        q.push_back(item);
        while q.len() > cap {
            q.pop_front();
        }
    }

    /// Store a VM edge record, fold it into the history; returns raised alerts.
    fn record_vm_flow(&mut self, rec: &VmFlowRecord) -> Vec<VmFlowAlert> {
        let alerts = self.flow_hist.observe(rec);
        Self::push_capped(&mut self.vm_flows, rec.clone(), VM_FLOW_STORE_CAP);
        alerts
    }
}

fn publish_vm_flow(
    bus: &broadcast::Sender<StreamEvent>,
    rec: &VmFlowRecord,
    alerts: Vec<VmFlowAlert>,
) {
    publish(bus, "flow", rec);
    for a in alerts {
        tracing::warn!(kind = %a.kind, src = %a.src, "{}", a.detail);
        publish(bus, "alert", &a);
    }
}

type SharedState = Arc<Mutex<Shared>>;

fn publish<T: serde::Serialize>(bus: &broadcast::Sender<StreamEvent>, topic: &str, ev: &T) {
    // Record even without an SSE subscriber. This is best-effort and observe-only.
    blackbox::record(topic, ev);
    if bus.receiver_count() == 0 {
        return;
    }
    if let Ok(event) = serde_json::to_value(ev) {
        let _ = bus.send(StreamEvent {
            topic: topic.to_string(),
            event,
        });
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

struct ScopeInfo {
    id: u32,
    /// cgroup path when this scope is cgroup-backed.
    cgroup: Option<PathBuf>,
    cgroup_id: Option<u64>,
}

struct CompiledPolicy {
    policy: Policy,
    scope_id: u32,
    rules: Vec<Rule>,
}

struct Engine {
    dp: Datapath,
    features: KernelFeatures,
    programs_compiled: bool,
    policies: HashMap<String, CompiledPolicy>,
    policy_nums: HashMap<String, u32>,
    next_policy_num: u32,
    /// Resolved DNS-deny addresses: (scope, prefix) → policy num.
    dns_blocked: HashMap<(u32, Prefix), u32>,
    scopes: HashMap<String, ScopeInfo>,
    next_scope: u32,
    mode: Mode,
    lease_deadline_mono: u64,
    lease_wall: Option<chrono::DateTime<chrono::Utc>>,
    lease_lapsed: bool,
    /// Interfaces attached by request (survive rescans): name → (guest_side, xdp).
    explicit: HashMap<String, (bool, bool)>,
    qos_by_vm: HashMap<String, (u64, u64)>,
    drop_names: HashMap<u32, String>,
    telemetry: TelemetryConfig,
    /// ifindex → (name, guest_side, xdp, vm, mac, qos_egress, qos_ingress)
    ifaces: HashMap<u32, IfaceRuntime>,
    shared: SharedState,
    bus: broadcast::Sender<StreamEvent>,
    capture_seq: u64,
    /// Accounting key (VM name, or `iface:<name>`) → totals folded from the datapath.
    acct_base: HashMap<String, AcctTotals>,
    /// ifindex → datapath counters already folded into `acct_base`.
    acct_offset: HashMap<u32, IfaceStats>,
    /// Accounting key → RFC 3339 start of its window.
    acct_since: HashMap<String, String>,
    cni: cni::CniRuntime,
    uplink: uplink::UplinkRuntime,
    vm_edge: vm::VmEdgeRuntime,
    vmauth: vmauth::VmAuth,
    vmproxy: vmproxy::VmProxy,
    sandbox: vm::SandboxRuntime,
    shield: shield::ShieldRuntime,
    nodeiso: nodeiso::NodeIsoRuntime,
    tls: tls::TlsRuntime,
    rtnl: rtnl::RtnlRuntime,
    l7s: l7sample::L7sRuntime,
    vmi: vmintel::VmiRuntime,
    guard: guard::GuardRuntime,
    direct: direct::DirectRuntime,
    quiclb: quiclb::QuicLbRuntime,
    afxdp: afxdp::AfxdpRuntime,
    scx: scx::ScxRuntime,
    egress: egress::EgressRuntime,
    overlay: overlay::OverlayRuntime,
    wake: wake::WakeRuntime,
    chaos: chaos::ChaosRuntime,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
struct AcctTotals {
    tx_bytes: u64,
    rx_bytes: u64,
    tx_pkts: u64,
    rx_pkts: u64,
    drops: u64,
}

impl AcctTotals {
    fn add_delta(&mut self, now: &IfaceStats, before: &IfaceStats) {
        self.tx_bytes += now.tx_bytes.saturating_sub(before.tx_bytes);
        self.rx_bytes += now.rx_bytes.saturating_sub(before.rx_bytes);
        self.tx_pkts += now.tx_pkts.saturating_sub(before.tx_pkts);
        self.rx_pkts += now.rx_pkts.saturating_sub(before.rx_pkts);
        self.drops += now.drops.saturating_sub(before.drops);
    }
}

fn acct_key(r: &IfaceRuntime) -> String {
    r.vm.clone().unwrap_or_else(|| format!("iface:{}", r.name))
}

#[derive(Clone)]
struct IfaceRuntime {
    name: String,
    guest_side: bool,
    xdp: bool,
    vm: Option<String>,
    mac: Option<String>,
    qos_egress_bps: u64,
    qos_ingress_bps: u64,
}

fn if_nametoindex(name: &str) -> Option<u32> {
    let c = std::ffi::CString::new(name).ok()?;
    let idx = unsafe { libc::if_nametoindex(c.as_ptr()) };
    (idx != 0).then_some(idx)
}

fn list_links() -> Vec<String> {
    std::fs::read_dir("/sys/class/net")
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default()
}

impl Engine {
    fn new(shared: SharedState, bus: broadcast::Sender<StreamEvent>) -> Result<Self> {
        let features = loader::kernel_features();
        let dp = Datapath::load()?;
        let mut eng = Self {
            dp,
            features,
            programs_compiled: true,
            policies: HashMap::new(),
            policy_nums: HashMap::new(),
            next_policy_num: 1,
            dns_blocked: HashMap::new(),
            scopes: HashMap::new(),
            next_scope: 1,
            mode: Mode::Observe,
            lease_deadline_mono: 0,
            lease_wall: None,
            lease_lapsed: false,
            explicit: HashMap::new(),
            qos_by_vm: HashMap::new(),
            drop_names: HashMap::new(),
            telemetry: TelemetryConfig::default(),
            ifaces: HashMap::new(),
            shared,
            bus,
            capture_seq: 0,
            acct_base: HashMap::new(),
            acct_offset: HashMap::new(),
            acct_since: HashMap::new(),
            cni: cni::CniRuntime::default(),
            uplink: uplink::UplinkRuntime::default(),
            vm_edge: vm::VmEdgeRuntime::default(),
            vmauth: vmauth::VmAuth::default(),
            vmproxy: vmproxy::VmProxy::default(),
            sandbox: vm::SandboxRuntime::default(),
            shield: shield::ShieldRuntime::default(),
            nodeiso: nodeiso::NodeIsoRuntime::default(),
            tls: tls::TlsRuntime::default(),
            rtnl: rtnl::RtnlRuntime::default(),
            l7s: l7sample::L7sRuntime::default(),
            vmi: vmintel::VmiRuntime::default(),
            guard: guard::GuardRuntime::default(),
            direct: direct::DirectRuntime::default(),
            quiclb: quiclb::QuicLbRuntime::default(),
            afxdp: afxdp::AfxdpRuntime::default(),
            scx: scx::ScxRuntime::default(),
            egress: egress::EgressRuntime::default(),
            overlay: overlay::OverlayRuntime::default(),
            wake: wake::WakeRuntime::default(),
            chaos: chaos::ChaosRuntime::default(),
        };
        eng.init()?;
        Ok(eng)
    }

    fn init(&mut self) -> Result<()> {
        self.dp.set_global(GlobalCfg {
            mode: MODE_OBSERVE,
            flags: self.global_flags(),
            lease_deadline_ns: 0,
        })?;
        if let Some(root) = crate::tracefs::tracefs_root() {
            let offs = crate::tracefs::resolve_offsets(&root);
            self.dp.set_tp_offsets(&offs)?;
            self.drop_names = crate::tracefs::drop_reason_names(&root);
        }
        self.dp.attach_telemetry();
        self.push_file_watch()?;
        // Host-wide container enforcement: scope 0 at the cgroup root.
        if self.features.cgroup2 {
            let root = Path::new(attribution::CGROUP_ROOT);
            if let Err(e) = self.dp.attach_cgroup(root, SOCK_PROGS, false) {
                self.dp
                    .notes
                    .push(format!("host cgroup enforcement unavailable: {e:#}"));
            } else if let Some(id) = attribution::cgroup_id(root) {
                let _ = self.dp.set_cgroup_scope(id, 0);
            }
        }
        self.sync_sockops();
        if let Err(e) = self.rtnl_configure(RtnlConfig::default()) {
            self.dp.notes.push(format!("{e:#}"));
        }
        Ok(())
    }

    fn global_flags(&self) -> u32 {
        let mut f = 0;
        if self.telemetry.exec {
            f |= GLOBAL_EXEC_EVENTS;
        }
        if self.telemetry.fork {
            f |= GLOBAL_FORK_EVENTS;
        }
        if self.telemetry.connect {
            f |= GLOBAL_CONNECT_EVENTS;
        }
        // File-watch / exec-deny / cap-deny are enabled whenever matching
        // policies or watch prefixes exist.
        if !self.telemetry.file_watch.is_empty() || self.has_kind("deny_file") {
            f |= GLOBAL_FILE_WATCH;
        }
        if self.has_kind("deny_process") {
            f |= GLOBAL_EXEC_DENY;
        }
        if self.has_kind("deny_cap") {
            f |= GLOBAL_CAP_DENY;
        }
        f
    }

    fn has_kind(&self, kind: &str) -> bool {
        self.policies
            .values()
            .any(|p| p.policy.enabled && p.policy.kind == kind)
    }

    fn refresh_global(&mut self) -> Result<()> {
        let flags = self.global_flags();
        let mode = if self.mode == Mode::Enforce {
            MODE_ENFORCE
        } else {
            MODE_OBSERVE
        };
        self.dp.set_global(GlobalCfg {
            mode,
            flags,
            lease_deadline_ns: self.lease_deadline_mono,
        })
    }

    // ---- scopes ----------------------------------------------------------

    fn scope_id_for(&mut self, scope: &Scope) -> Result<u32> {
        let Some(key) = scope.key() else {
            return Ok(0);
        };
        if let Some(s) = self.scopes.get(&key) {
            return Ok(s.id);
        }
        let id = self.next_scope;
        self.next_scope += 1;
        let mut info = ScopeInfo {
            id,
            cgroup: None,
            cgroup_id: None,
        };
        match scope {
            Scope::Cgroup { path } => {
                let full = Path::new(attribution::CGROUP_ROOT).join(path.trim_matches('/'));
                let cid = attribution::cgroup_id(&full)
                    .ok_or_else(|| anyhow!("cgroup path {} not found", full.display()))?;
                self.dp.attach_cgroup(&full, SOCK_PROGS, true)?;
                self.dp.set_cgroup_scope(cid, id)?;
                info.cgroup = Some(full);
                info.cgroup_id = Some(cid);
            }
            Scope::Vm { name } => {
                // Assign this scope to the VM's current taps.
                let idxs: Vec<u32> = self
                    .ifaces
                    .iter()
                    .filter(|(_, r)| r.vm.as_deref() == Some(name.as_str()))
                    .map(|(i, _)| *i)
                    .collect();
                self.scopes.insert(key.clone(), info);
                for idx in idxs {
                    self.program_iface(idx)?;
                }
                return Ok(id);
            }
            Scope::Host => unreachable!(),
        }
        self.scopes.insert(key, info);
        Ok(id)
    }

    fn scope_of_iface(&self, r: &IfaceRuntime) -> u32 {
        r.vm.as_ref()
            .and_then(|vm| self.scopes.get(&format!("vm:{vm}")))
            .map(|s| s.id)
            .unwrap_or(0)
    }

    /// Does `scope_id` (or host scope 0) have any enabled policy of the given kinds?
    fn scope_has(&self, scope_id: u32, kinds: &[&str]) -> bool {
        self.policies.values().any(|p| {
            p.policy.enabled
                && (p.scope_id == scope_id || p.scope_id == 0)
                && kinds.contains(&p.policy.kind.as_str())
        })
    }

    // ---- interfaces ------------------------------------------------------

    fn add_iface(
        &mut self,
        name: &str,
        guest_side: bool,
        xdp: bool,
        vm: Option<String>,
        mac: Option<String>,
    ) -> Result<()> {
        let Some(idx) = if_nametoindex(name) else {
            return Err(anyhow!("interface {name} not found"));
        };
        self.dp.attach_tc(name)?;
        if xdp {
            if let Err(e) = self.dp.attach_xdp(name) {
                self.dp
                    .notes
                    .push(format!("XDP attach to {name} failed: {e:#}"));
            }
        }
        self.ifaces.insert(
            idx,
            IfaceRuntime {
                name: name.to_string(),
                guest_side,
                xdp,
                vm: vm.clone(),
                mac: mac.clone(),
                qos_egress_bps: 0,
                qos_ingress_bps: 0,
            },
        );
        {
            let mut sh = lock(&self.shared);
            sh.ifaces.insert(
                idx,
                IfaceMeta {
                    name: name.to_string(),
                    vm,
                },
            );
        }
        self.program_iface(idx)?;
        Ok(())
    }

    fn remove_iface(&mut self, name: &str) {
        let Some(idx) = if_nametoindex(name).or_else(|| {
            self.ifaces
                .iter()
                .find(|(_, r)| r.name == name)
                .map(|(i, _)| *i)
        }) else {
            return;
        };
        self.retire_iface_stats(idx);
        self.dp.detach_tc(name);
        self.dp.detach_xdp(name);
        self.dp.remove_iface_cfg(idx);
        self.ifaces.remove(&idx);
        lock(&self.shared).ifaces.remove(&idx);
    }

    /// Recompute and push an interface's IfaceCfg from current policy/telemetry.
    fn program_iface(&mut self, idx: u32) -> Result<()> {
        let Some(r) = self.ifaces.get(&idx).cloned() else {
            return Ok(());
        };
        let scope = self.scope_of_iface(&r);
        let mut flags = 0u32;
        if r.guest_side {
            flags |= IF_GUEST_SIDE;
        }
        if self.scope_has(scope, &["deny_ip", "deny_port"])
            || self.dns_blocked.keys().any(|(s, _)| *s == scope || *s == 0)
        {
            flags |= IF_DENY;
        }
        if self.scope_has(scope, &["tc_allow", "allow_port"]) {
            flags |= IF_ALLOW;
        }
        if self.scope_has(scope, &["rate_limit"]) {
            flags |= IF_RATE;
        }
        if self.telemetry.l7 {
            flags |= IF_L7;
        }
        if self.telemetry.flows {
            flags |= IF_FLOWS;
        }
        if self.telemetry.dns || self.has_kind("deny_dns") {
            flags |= IF_DNS;
        }
        if r.qos_egress_bps > 0 || r.qos_ingress_bps > 0 {
            flags |= IF_QOS;
        }
        {
            let sh = lock(&self.shared);
            if sh
                .captures
                .values()
                .any(|c| c.ifindex == idx && !c.info.done)
            {
                flags |= IF_CAPTURE;
            }
        }
        self.dp.set_iface_cfg(
            idx,
            IfaceCfg {
                scope,
                flags,
                // API rates are bits/s; the datapath paces in bytes/s.
                qos_egress_bps: r.qos_egress_bps / 8,
                qos_ingress_bps: r.qos_ingress_bps / 8,
                capture_sample: 0,
                capture_snaplen: 0,
            },
        )
    }

    fn reprogram_all_ifaces(&mut self) -> Result<()> {
        let idxs: Vec<u32> = self.ifaces.keys().copied().collect();
        for idx in idxs {
            self.program_iface(idx)?;
        }
        Ok(())
    }

    fn refresh_scope_flags(&mut self, scope_id: u32) -> Result<()> {
        let mut f = 0;
        if self.scope_has(scope_id, &["deny_ip", "deny_port"])
            || self.dns_blocked.keys().any(|(s, _)| *s == scope_id)
        {
            f |= IF_DENY;
        }
        // Scope 0 is bound at the cgroup root: an allowlist there would
        // default-deny every host process, so host allowlists stay on taps.
        if scope_id != 0 && self.scope_has(scope_id, &["tc_allow", "allow_port"]) {
            f |= IF_ALLOW;
        }
        self.dp.set_scope_flags(scope_id, f)
    }

    fn push_file_watch(&mut self) -> Result<()> {
        // deny_file prefixes (with deny flag) first, then observe-only watches.
        let mut entries: Vec<(String, Option<u32>)> = Vec::new();
        for p in self.policies.values() {
            if p.policy.enabled && p.policy.kind == "deny_file" {
                if let Some(Rule::FileDeny { prefix }) = p.rules.first() {
                    let num = self.policy_nums.get(&p.policy.id).copied().unwrap_or(0);
                    entries.push((prefix.clone(), Some(num)));
                }
            }
        }
        for w in &self.telemetry.file_watch {
            if entries.len() as u32 >= FILE_WATCH_SLOTS {
                break;
            }
            if !entries.iter().any(|(p, _)| p == w) {
                entries.push((w.clone(), None));
            }
        }
        self.dp.set_file_watch(&entries)
    }
}
