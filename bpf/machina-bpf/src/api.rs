// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Wire types for the machina-bpfd Unix-socket API (newline-delimited JSON).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DEFAULT_SOCKET: &str = "/run/machina-bpf/bpfd.sock";
pub const SOCKET_ENV: &str = "MACHINA_BPFD_SOCK";

/// Where a policy applies.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scope {
    /// Every VM tap and every registered cgroup on the host.
    #[default]
    Host,
    /// Taps belonging to one libvirt domain.
    Vm { name: String },
    /// A cgroup v2 subtree (containers, systemd units), path relative to /sys/fs/cgroup.
    Cgroup { path: String },
}

impl Scope {
    pub fn key(&self) -> Option<String> {
        match self {
            Scope::Host => None,
            Scope::Vm { name } => Some(format!("vm:{name}")),
            Scope::Cgroup { path } => Some(format!("cgroup:{}", path.trim_matches('/'))),
        }
    }

    pub fn label(&self) -> String {
        self.key().unwrap_or_else(|| "host".into())
    }
}

/// Policy kinds understood by the native datapath.
pub const POLICY_KINDS: &[&str] = &[
    "deny_ip",
    "deny_port",
    "tc_allow",
    "allow_port",
    "deny_process",
    "deny_file",
    "deny_cap",
    "deny_dns",
    "rate_limit",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub kind: String,
    #[serde(rename = "match")]
    pub match_value: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub scope: Scope,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Observe,
    Enforce,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModeState {
    pub mode: Mode,
    /// RFC 3339 wall-clock expiry of the enforce lease.
    pub lease_expires_at: Option<String>,
    pub lease_remaining_secs: Option<u64>,
    /// True when enforce was requested but the lease has lapsed (datapath failed open).
    #[serde(default)]
    pub lease_expired: bool,
    /// Enforcers that drop only under a live lease. All of them start in
    /// observe when bpfd starts; `node_isolation` holds its own shorter lease.
    #[serde(default)]
    pub covers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TelemetryConfig {
    #[serde(default = "default_true")]
    pub exec: bool,
    #[serde(default)]
    pub fork: bool,
    #[serde(default = "default_true")]
    pub connect: bool,
    #[serde(default = "default_true")]
    pub flows: bool,
    #[serde(default = "default_true")]
    pub dns: bool,
    /// First client payload per TCP flow: TLS SNI/ALPN, HTTP request line, SSH banner.
    #[serde(default = "default_true")]
    pub l7: bool,
    /// Path prefixes reported on open (max 8 including deny_file prefixes).
    #[serde(default = "default_watch")]
    pub file_watch: Vec<String>,
    /// Interface name globs auto-attached as workload taps.
    #[serde(default = "default_patterns")]
    pub iface_patterns: Vec<String>,
    /// `mn_sockops` on the root cgroup: connect latency + TCP pressure.
    #[serde(default = "default_true")]
    pub tcp: bool,
}

fn default_watch() -> Vec<String> {
    vec![
        "/etc/shadow".into(),
        "/etc/sudoers".into(),
        "/root/.ssh/".into(),
        "/etc/machina/".into(),
        "/etc/libvirt/".into(),
    ]
}

fn default_patterns() -> Vec<String> {
    vec!["vnet*".into(), "tap*".into()]
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            exec: true,
            fork: false,
            connect: true,
            flows: true,
            dns: true,
            l7: true,
            file_watch: default_watch(),
            iface_patterns: default_patterns(),
            tcp: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KernelFeatures {
    pub kernel: String,
    pub btf: bool,
    pub tcx: bool,
    pub lsm_bpf: bool,
    pub tracefs: Option<String>,
    pub cgroup2: bool,
    /// fentry/fexit (BTF trampolines; kernel >= 5.5 with vmlinux BTF).
    #[serde(default)]
    pub fentry: bool,
    /// sched_ext available (`/sys/kernel/sched_ext` exists).
    #[serde(default)]
    pub sched_ext: bool,
    /// `/sys/kernel/sched_ext/state` (disabled / enabled / ...).
    #[serde(default)]
    pub sched_ext_state: Option<String>,
    /// AF_XDP sockets can be created.
    #[serde(default)]
    pub xsk: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IfaceStatus {
    pub name: String,
    pub ifindex: u32,
    pub vm: Option<String>,
    pub mac: Option<String>,
    pub scope: u32,
    pub flags: Vec<String>,
    pub guest_side: bool,
    pub xdp: bool,
    pub qos_egress_bps: u64,
    pub qos_ingress_bps: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Counters {
    pub net_events: u64,
    pub drops: u64,
    pub observed: u64,
    pub flows_opened: u64,
    pub proc_events: u64,
    pub dns_events: u64,
    pub capture_packets: u64,
    pub anomalies: u64,
    #[serde(default)]
    pub l7_events: u64,
    #[serde(default)]
    pub rate_limited: u64,
    #[serde(default)]
    pub tls_fingerprints: u64,
    #[serde(default)]
    pub ssl_events: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BpfStatus {
    pub available: bool,
    pub programs_compiled: bool,
    pub version: String,
    pub features: KernelFeatures,
    pub mode: ModeState,
    pub policies_total: usize,
    pub policies_enabled: usize,
    pub interfaces: Vec<IfaceStatus>,
    pub cgroups: Vec<String>,
    pub tracepoints: Vec<String>,
    pub counters: Counters,
    pub telemetry: TelemetryConfig,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct FlowRecord {
    /// Owning workload (`vm`, `pod`, `container` or `service`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload: Option<Workload>,
    pub iface: String,
    pub ifindex: u32,
    pub vm: Option<String>,
    pub proto: String,
    pub local: String,
    pub local_port: u16,
    pub remote: String,
    pub remote_port: u16,
    /// "local" (workload initiated) or "remote".
    pub origin: String,
    pub tx_pkts: u64,
    pub tx_bytes: u64,
    pub rx_pkts: u64,
    pub rx_bytes: u64,
    pub first_seen: String,
    pub last_seen: String,
    /// "pass" | "drop" | "observed"
    pub verdict: String,
    pub tcp_flags: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetEventRecord {
    /// Owning workload (`vm`, `pod`, `container` or `service`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload: Option<Workload>,
    pub ts: String,
    /// flow_open | flow_close | deny | allow_miss | qos_drop
    pub kind: String,
    /// pass | drop | observed
    pub verdict: String,
    pub policy_id: Option<String>,
    pub iface: Option<String>,
    pub vm: Option<String>,
    pub proto: String,
    pub local: String,
    pub local_port: u16,
    pub remote: String,
    pub remote_port: u16,
    pub pkt_len: u32,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct DnsAnswer {
    pub name: String,
    pub rtype: String,
    pub ttl: u32,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DnsRecord {
    /// Owning workload (`vm`, `pod`, `container` or `service`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload: Option<Workload>,
    pub ts: String,
    pub iface: Option<String>,
    pub vm: Option<String>,
    pub client: String,
    pub server: String,
    pub id: u16,
    pub is_response: bool,
    pub qname: String,
    pub qtype: String,
    pub rcode: String,
    pub answers: Vec<DnsAnswer>,
}

/// First client payload of a TCP flow, classified.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct L7Record {
    /// Owning workload (`vm`, `pod`, `container` or `service`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload: Option<Workload>,
    pub ts: String,
    pub iface: Option<String>,
    pub vm: Option<String>,
    /// tls | http | ssh
    pub protocol: String,
    /// "outbound" (workload is the client) or "inbound".
    pub direction: String,
    pub client: String,
    pub client_port: u16,
    pub server: String,
    pub server_port: u16,
    /// TLS server name, or the HTTP Host header.
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alpn: Vec<String>,
    pub tls_version: Option<String>,
    pub method: Option<String>,
    pub path: Option<String>,
    pub user_agent: Option<String>,
    pub banner: Option<String>,
}

/// Traffic totals for one workload (VM, or interface when unattributed).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct AccountingRecord {
    pub vm: Option<String>,
    pub interfaces: Vec<String>,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub tx_pkts: u64,
    pub rx_pkts: u64,
    pub drops: u64,
    /// RFC 3339 start of the accounting window (persisted across restarts).
    pub since: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProcRecord {
    /// Owning workload (`vm`, `pod`, `container` or `service`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload: Option<Workload>,
    pub ts: String,
    /// exec | exit | fork | file_open | connect | cap_denied
    pub kind: String,
    pub pid: u32,
    pub tgid: u32,
    pub ppid: Option<u32>,
    pub child_pid: Option<u32>,
    pub uid: u32,
    pub gid: u32,
    pub comm: String,
    pub path: Option<String>,
    pub cmdline: Option<String>,
    pub cgroup_id: u64,
    pub cgroup: Option<String>,
    pub unit: Option<String>,
    pub vm: Option<String>,
    pub container: Option<String>,
    pub denied: bool,
    pub killed: bool,
    pub policy_id: Option<String>,
    pub open_flags: Option<u32>,
    pub capability: Option<String>,
    pub proto: Option<String>,
    pub saddr: Option<String>,
    pub sport: Option<u16>,
    pub daddr: Option<String>,
    pub dport: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Anomaly {
    pub id: String,
    pub ts: String,
    /// port_scan | inbound_scan | beaconing | egress_volume_spike |
    /// new_destination | policy_violation_burst | suspicious_exec | dns_tunneling
    pub kind: String,
    /// low | medium | high | critical
    pub severity: String,
    pub summary: String,
    pub vm: Option<String>,
    pub iface: Option<String>,
    pub local: Option<String>,
    pub remote: Option<String>,
    #[serde(default)]
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DropReason {
    pub reason: u32,
    pub name: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TcpHealth {
    /// retransmit | rst_sent | rst_recv
    pub kind: String,
    pub addr: String,
    pub count: u64,
}

/// Active connect latency towards one remote endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConnectHealth {
    pub addr: String,
    pub port: u16,
    pub count: u64,
    pub failures: u64,
    pub avg_us: u64,
    pub max_us: u64,
    /// Upper bound of the bucket holding the 90th percentile (None = > 1 s).
    pub p90_le_us: Option<u64>,
    /// Counts per bucket: <100µs, <1ms, <5ms, <10ms, <50ms, <100ms, <1s, ≥1s.
    pub hist: Vec<u64>,
}

/// Last TCP socket snapshot towards a peer (state change / retransmit).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TcpPeerPressure {
    pub addr: String,
    pub srtt_us: u32,
    pub cwnd: u32,
    pub ssthresh: u32,
    pub mss: u32,
    pub total_retrans: u32,
    pub retrans_events: u32,
    /// Bytes/s from the kernel's delivery-rate sample (0 = none yet).
    pub delivery_rate_bps: u64,
    pub age_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetHealth {
    pub drop_reasons: Vec<DropReason>,
    pub tcp: Vec<TcpHealth>,
    #[serde(default)]
    pub connect: Vec<ConnectHealth>,
    #[serde(default)]
    pub pressure: Vec<TcpPeerPressure>,
    /// Cgroup carrying `mn_sockops`, if attached.
    #[serde(default)]
    pub sockops: Option<String>,
}

/// ICMP errors per datapath interface.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IcmpError {
    pub iface: String,
    pub vm: Option<String>,
    /// unreachable | time_exceeded | param_problem | packet_too_big
    pub kind: String,
    pub code: u8,
    pub family: String,
    /// from_workload | to_workload
    pub direction: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CaptureInfo {
    pub id: String,
    pub iface: String,
    pub vm: Option<String>,
    pub started_at: String,
    pub ends_at: String,
    pub packets: u64,
    pub bytes: u64,
    pub max_packets: usize,
    pub sample: u32,
    pub snaplen: u32,
    pub done: bool,
}

// ---- machina-cni ------------------------------------------------------------

/// A local pod: its IPv4 address and the host side of its veth pair.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct CniEndpoint {
    pub ip: String,
    pub host_iface: String,
    pub pod_mac: String,
    pub host_mac: String,
    #[serde(default)]
    pub pod: Option<String>,
    /// CNI_CONTAINERID (the pod sandbox); joins the pod to its kubepods cgroup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
}

/// One allowed (subject, peer, direction, proto, port) tuple; peer 0 = any,
/// proto 0 = any, port 0 = any.
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub struct CniPolicyEntry {
    pub subject: u32,
    pub peer: u32,
    pub egress: bool,
    pub proto: u8,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CniBackend {
    pub addr: String,
    pub port: u16,
    /// Backend runs on another node (NodePort reaches it by SNAT or DSR).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remote: bool,
    /// Address of the node hosting a remote backend (DSR encap target).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
}

/// A service frontend. `addr` "0.0.0.0" / "::" = NodePort on this node's
/// IPv4 / IPv6 address.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct CniService {
    pub addr: String,
    pub port: u16,
    pub proto: u8,
    pub backends: Vec<CniBackend>,
    #[serde(default)]
    pub name: Option<String>,
    /// `sessionAffinity: ClientIP` timeout in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affinity_secs: Option<u32>,
}

/// A synced service as programmed: Maglev table (2+ backends) and live
/// ClientIP affinity pins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CniServiceStatus {
    #[serde(flatten)]
    pub service: CniService,
    pub maglev: bool,
    pub affinity_entries: usize,
}

/// Node-level datapath settings for `cni_configure`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct CniNodeConfig {
    /// Node primary IPv4 address (NodePort frontend).
    pub node_addr: String,
    #[serde(default)]
    pub node_addr6: Option<String>,
    /// Interface that gets the NodePort classifier (none = no NodePort).
    #[serde(default)]
    pub uplink: Option<String>,
    /// How NodePort reaches remote backends: "snat" (default) or "dsr".
    #[serde(default)]
    pub lb_mode: Option<String>,
    /// Also accelerate NodePort → local backend in XDP on the uplink.
    #[serde(default)]
    pub xdp: bool,
}

/// Identity / isolation for one pod IP (cluster-wide).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct CniIdentity {
    pub ip: String,
    pub identity: u32,
    #[serde(default)]
    pub ingress_isolated: bool,
    #[serde(default)]
    pub egress_isolated: bool,
}

/// The CNI state ABI this build speaks (see [`CniState::version`]).
pub const CNI_STATE_VERSION: u32 = machina_bpf_common::CNI_ABI_VERSION;

/// Full desired CNI state; each sync replaces the previous one.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct CniState {
    /// Must equal [`CNI_STATE_VERSION`]; bpfd rejects other agents.
    #[serde(default)]
    pub version: u32,
    pub identities: Vec<CniIdentity>,
    pub policy: Vec<CniPolicyEntry>,
    /// NetworkPolicy ipBlock CIDRs → identity.
    pub cidrs: Vec<(String, u32)>,
    pub services: Vec<CniService>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CniStatus {
    pub configured: bool,
    pub version: u32,
    pub node_addr: Option<String>,
    #[serde(default)]
    pub node_addr6: Option<String>,
    pub uplink: Option<String>,
    #[serde(default)]
    pub lb_mode: String,
    #[serde(default)]
    pub xdp: bool,
    /// Services with a Maglev table (two or more backends).
    #[serde(default)]
    pub maglev_services: usize,
    pub endpoints: Vec<CniEndpoint>,
    pub identities: usize,
    pub policy_entries: usize,
    pub services: usize,
    pub last_sync: Option<String>,
}

/// One VM at the edge. `group` (from controller VM labels) picks the policy
/// identity; VMs without a group get their own (`vm:<name>`).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmEdgeVm {
    pub name: String,
    #[serde(default)]
    pub group: Option<String>,
    /// Guest addresses, so other VMs can match this one as a peer.
    #[serde(default)]
    pub addresses: Vec<String>,
    /// Tap names; empty = discover from libvirt's live domain XML.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub taps: Vec<String>,
    #[serde(default)]
    pub isolate_ingress: bool,
    #[serde(default)]
    pub isolate_egress: bool,
    /// Mbit/s from / towards the VM; 0 = unlimited.
    #[serde(default)]
    pub egress_mbps: u32,
    #[serde(default)]
    pub ingress_mbps: u32,
    /// Packets/s each direction; 0 = unlimited.
    #[serde(default)]
    pub pps: u32,
    /// Explicit identity (VM network policy compiler); overrides `group`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

/// Rule between groups or identities. `peer` None = any peer (including
/// non-VMs), proto 0 = any, port 0 = any. For ICMP (1 / 58) `port` is the
/// ICMP type + 1. `port_end` > `port` expands to a range (at most 256).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct VmEdgeRule {
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub peer: Option<String>,
    #[serde(default)]
    pub egress: bool,
    #[serde(default)]
    pub proto: u8,
    #[serde(default)]
    pub port: u16,
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub port_end: u16,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deny: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_identity: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_identity: Option<u32>,
    /// `policy-name spec[0].ingress[1]`, for flow attribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Allow only after the peer identity is authenticated
    /// (`AUTH_REQUIRED`, or `AUTH_ALWAYS_FAIL` for `test-always-fail`).
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub auth: u8,
    /// L7 rules apply (see [`VmEdgeState::l7`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub l7: bool,
}

pub const AUTH_REQUIRED: u8 = 1;
pub const AUTH_ALWAYS_FAIL: u8 = 2;

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

/// L7 rules of one allow entry (same key as its [`VmEdgeRule`]).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct VmEdgeL7Rule {
    pub subject_identity: u32,
    /// 0 = any peer.
    pub peer_identity: u32,
    pub egress: bool,
    pub proto: u8,
    pub port: u16,
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub port_end: u16,
    pub rules: crate::netpol::l7::L7Rules,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// One authenticated (subject, peer) pair.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmAuthEntry {
    pub subject: String,
    pub subject_identity: u32,
    pub peer: String,
    pub peer_identity: u32,
    /// `required` or `test-always-fail`.
    pub mode: String,
    /// `authenticated`, or why not.
    pub state: String,
    pub expires_in_secs: u64,
}

/// `VmAuthIdentity` reply.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmAuthIdentity {
    pub csr: String,
    #[serde(default)]
    pub cert: Option<VmAuthCertInfo>,
}

/// The host certificate a bpfd holds.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmAuthCertInfo {
    pub host_id: String,
    /// Unix seconds.
    pub not_after: i64,
    /// Accepting handshakes on `authca::AUTH_PORT`.
    pub listening: bool,
}

fn is_zero_u16(v: &u16) -> bool {
    *v == 0
}

/// Address (or prefix) → identity outside the VMs in the state: VMs on
/// other hosts, CIDR peers, the host itself and other hypervisors.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct VmEdgePeer {
    /// Address or CIDR.
    pub cidr: String,
    pub identity: u32,
    /// VM name, `host`, `remote-node` or the CIDR text.
    #[serde(default)]
    pub name: String,
    /// Host id of a VM peer (who to authenticate it with).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub host: String,
}

/// Egress allow towards addresses a DNS reply to the subject resolved for a
/// name matching `pattern` (normalized `toFQDNs` selector).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct VmEdgeFqdnRule {
    pub pattern: String,
    pub subject_identity: u32,
    #[serde(default)]
    pub proto: u8,
    #[serde(default)]
    pub port: u16,
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub port_end: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l7: Option<crate::netpol::l7::L7Rules>,
}

/// One learned name → address binding.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmFqdnEntry {
    pub name: String,
    pub address: String,
    pub identity: u32,
    /// VM whose DNS reply taught it.
    pub vm: String,
    pub expires_in_secs: u64,
    /// `toFQDNs` patterns it satisfies.
    pub patterns: Vec<String>,
}

/// Desired VM edge state; each `vm_edge_sync` replaces the previous one.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmEdgeState {
    pub vms: Vec<VmEdgeVm>,
    #[serde(default)]
    pub policy: Vec<VmEdgeRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub peers: Vec<VmEdgePeer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fqdn: Vec<VmEdgeFqdnRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub l7: Vec<VmEdgeL7Rule>,
    /// Emit per-flow verdict events (`flow` topic) on every edge tap.
    #[serde(default)]
    pub flow_log: bool,
    /// Who synced it (`daemon`, `controller`, empty = manual).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner: String,
    /// This host's id in the fleet (controller syncs only).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub host_id: String,
    /// Host id → address, for bpfd-to-bpfd authentication.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub host_addrs: BTreeMap<String, String>,
    /// Also count every global address of the receiving node as `host`
    /// (a controller only knows the management address).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub node_is_host: bool,
}

/// One VM edge verdict (`flow` topic / `vm_flows`).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmFlowRecord {
    pub ts: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub iface: String,
    /// The VM owning the tap.
    pub vm: String,
    /// "ingress" (towards the VM) or "egress" (from the VM).
    pub direction: String,
    pub src: String,
    pub src_port: u16,
    pub dst: String,
    pub dst_port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src_vm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dst_vm: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub src_labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dst_labels: BTreeMap<String, String>,
    pub src_identity: u32,
    pub dst_identity: u32,
    pub proto: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tcp_flags: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icmp_type: Option<u8>,
    pub bytes: u32,
    /// FORWARDED, DROPPED or AUDIT.
    pub verdict: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    /// `http`, `kafka`, `tls` or `dns` for L7 verdicts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l7_type: Option<String>,
    /// `GET example.com/api`, `kafka produce v7 topic=orders`, ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l7: Option<String>,
}

/// Flows folded by (source, destination, direction, protocol, port, verdict,
/// reason, policy): the history behind learn mode, replay and the service map.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct VmFlowEdge {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// VM name, else the address.
    pub src: String,
    pub dst: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src_vm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dst_vm: Option<String>,
    /// Non-VM identity of the source: `host`, `world`, `remote-node` or a CIDR.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src_entity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dst_entity: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub src_labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dst_labels: BTreeMap<String, String>,
    pub direction: String,
    pub proto: String,
    /// Destination port; the ICMP type for ICMP.
    pub port: u16,
    pub verdict: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    pub count: u64,
    pub bytes: u64,
    pub first_seen: String,
    pub last_seen: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub l7: Vec<VmFlowL7Stat>,
}

/// One normalised L7 request on an edge (`GET api/users/{id}`).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct VmFlowL7Stat {
    pub kind: String,
    pub request: String,
    pub count: u64,
    #[serde(default)]
    pub denied: u64,
    /// Response classes (`2xx`, `4xx`, ...); proxied HTTP only.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub status: BTreeMap<String, u64>,
    #[serde(default)]
    pub latency_n: u64,
    #[serde(default)]
    pub latency_ms_total: u64,
    #[serde(default)]
    pub latency_ms_max: u64,
}

/// Lateral-movement / scan detection (`alert` topic).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct VmFlowAlert {
    pub ts: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// `port_scan`, `host_sweep`, `deny_burst`, `new_peer`, `threat_domain`,
    /// `new_domain`.
    pub kind: String,
    /// `low`, `medium`, `high`.
    pub severity: String,
    pub src: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src_vm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dst: Option<String>,
    pub detail: String,
    #[serde(default)]
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VmEdgeTap {
    pub vm: String,
    pub iface: String,
    pub identity: u32,
    pub flags: Vec<String>,
    pub stats: VmEdgeCounters,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmEdgeCounters {
    pub out_pkts: u64,
    pub out_bytes: u64,
    pub in_pkts: u64,
    pub in_bytes: u64,
    pub denied: u64,
    pub observed: u64,
    pub rate_dropped: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VmEdgeStatus {
    pub vms: usize,
    pub rules: usize,
    pub groups: BTreeMap<String, u32>,
    pub taps: Vec<VmEdgeTap>,
    /// VMs in the state with no tap on this host.
    pub missing: Vec<String>,
    pub enforcing: bool,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub peers: usize,
    #[serde(default)]
    pub flow_log: bool,
    /// Why Cilium looks present on this host (None = absent).
    #[serde(default)]
    pub cilium: Option<String>,
    #[serde(default)]
    pub fqdn_rules: usize,
    /// Learned name → address bindings in force.
    #[serde(default)]
    pub fqdn_cache: usize,
    #[serde(default)]
    pub l7_rules: usize,
    /// Authenticated (subject, peer) pairs.
    #[serde(default)]
    pub auth_entries: usize,
    #[serde(default)]
    pub auth_cert: Option<VmAuthCertInfo>,
    /// TLS-intercepting / rewriting proxy: `listening on …`, or why it is
    /// off while proxy rules exist; empty without proxy rules.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proxy: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quarantines: Vec<VmQuarantine>,
    /// Global addresses of this node as `address/prefix`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub node_addrs: Vec<String>,
    /// VM → source addresses its tap sent from recently (unverified).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub learned: BTreeMap<String, Vec<String>>,
}

/// One exception while a VM is quarantined. `peer`: `host` (this host's
/// addresses), `world`, `any`, or a VM in the synced edge state.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmQuarantineAllow {
    /// `ingress` (towards the VM) or `egress`.
    pub direction: String,
    pub peer: String,
    /// `tcp`, `udp`, `sctp`, `icmp`, `icmpv6`, or empty for any.
    #[serde(default)]
    pub proto: String,
    /// 0 = any.
    #[serde(default)]
    pub port: u16,
}

/// A quarantined VM: every flow on its taps is dropped, enforcement lease
/// or not, except the allowlist; flows already open are cut too. The
/// kernel lifts it at `until` even if bpfd is gone.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmQuarantine {
    pub vm: String,
    /// RFC 3339.
    pub since: String,
    pub until: String,
    #[serde(default)]
    pub remaining_secs: u64,
    #[serde(default)]
    pub allow: Vec<VmQuarantineAllow>,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub by: String,
    /// Taps carrying it now (empty: VM not running here).
    #[serde(default)]
    pub taps: Vec<String>,
}

pub const QUARANTINE_MAX_SECS: u64 = 86_400;
pub const QUARANTINE_DEFAULT_SECS: u64 = 3600;

/// Body of the daemon / controller quarantine endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VmQuarantineBody {
    #[serde(default)]
    pub secs: Option<u64>,
    #[serde(default)]
    pub allow: Vec<VmQuarantineAllow>,
    /// Shorthand for an `ingress` `host` `tcp/22` exception.
    #[serde(default)]
    pub allow_host_ssh: bool,
    #[serde(default)]
    pub reason: String,
}

impl VmQuarantineBody {
    pub fn into_request(self, vm: String, by: String) -> Request {
        let mut allow = self.allow;
        if self.allow_host_ssh {
            allow.push(VmQuarantineAllow {
                direction: "ingress".into(),
                peer: "host".into(),
                proto: "tcp".into(),
                port: 22,
            });
        }
        Request::VmQuarantine {
            vm,
            secs: self.secs.unwrap_or(QUARANTINE_DEFAULT_SECS),
            allow,
            reason: self.reason,
            by,
        }
    }
}

/// One DNS threat feed (without its domain list).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct VmThreatFeed {
    pub name: String,
    /// URL it was fetched from, or empty for an inline list.
    #[serde(default)]
    pub source: String,
    /// Deny egress to the addresses its domains resolve to (drops need the
    /// enforcement lease); otherwise alert only.
    #[serde(default)]
    pub block: bool,
    #[serde(default)]
    pub domains: usize,
    #[serde(default)]
    pub updated: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
}

/// An address a blocking feed's domain resolved to for a VM.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct VmThreatBlock {
    pub address: String,
    pub domain: String,
    pub feed: String,
    pub vm: String,
    pub expires_in_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct VmThreatStatus {
    pub feeds: Vec<VmThreatFeed>,
    #[serde(default)]
    pub blocked: Vec<VmThreatBlock>,
    /// VMs whose DNS replies are being checked.
    #[serde(default)]
    pub watched_vms: usize,
}

/// Traffic from `sources` (VM addresses on this host) leaving the host is
/// rewritten to come from `egress_ip`, an address configured on this host.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmEgressSnatRule {
    /// The Fleet Cloud project the rule is for (informational).
    #[serde(default)]
    pub project: String,
    pub egress_ip: String,
    pub sources: Vec<String>,
}

/// Every egress SNAT rule for one host; replaces the previous set.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmEgressSnat {
    #[serde(default)]
    pub rules: Vec<VmEgressSnatRule>,
    /// Destinations never rewritten. `None` = private, link-local,
    /// loopback, CGNAT and multicast ranges.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,
    /// Add egress IPs missing on the host to the uplink (as /32 or /128) and
    /// announce them (gratuitous ARP, unsolicited NA). `None` = yes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manage_addresses: Option<bool>,
    /// Interface for managed addresses; default: the interface of the
    /// default route of the address family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct VmEgressSnatStatus {
    pub rules: Vec<VmEgressSnatRule>,
    pub exclude: Vec<String>,
    /// The nftables table is installed.
    pub active: bool,
    /// Rules or sources left out, with the reason.
    #[serde(default)]
    pub skipped: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    /// Egress IPs bpfd added to an interface (`IP/prefix@dev`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub managed: Vec<String>,
}

/// One VM address and the fleet address it is reached by over the overlay.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmOverlayMap {
    pub local: String,
    pub fleet: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub vm: String,
}

/// Another host on the overlay.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmOverlayPeer {
    #[serde(default)]
    pub host: String,
    /// WireGuard public key (base64).
    pub public_key: String,
    /// `address:port` the peer listens on.
    pub endpoint: String,
    /// Prefixes accepted from this peer by WireGuard (source ownership).
    pub prefixes: Vec<String>,
    /// Destination prefixes installed through this peer. Empty means `prefixes`
    /// for backward compatibility. Keeping this separate lets an accepted VPC
    /// peering establish source ownership without routing traffic until a route
    /// table actually selects the peer.
    #[serde(default)]
    pub routes: Vec<String>,
}

/// One direct routed pair admitted through the overlay guard. Used by Fleet
/// Cloud VPC peering; normal VM-overlay traffic still requires fleet-address DNAT.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmOverlayRoute {
    pub source: String,
    pub destination: String,
}

/// The WireGuard overlay between hosts: VM traffic to another host's fleet
/// prefix leaves encrypted, with the sending VM's fleet address as source,
/// and arrives at the VM behind the destination fleet address.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmOverlay {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub listen_port: u16,
    /// This host's fleet prefixes (`/24` IPv4, `/64` IPv6); the first
    /// address of each is the host's own.
    #[serde(default)]
    pub prefixes: Vec<String>,
    /// The whole fleet ranges: unreachable unless a peer route is more
    /// specific, so fleet traffic never leaves through the uplink.
    #[serde(default)]
    pub fleet_prefixes: Vec<String>,
    #[serde(default)]
    pub mappings: Vec<VmOverlayMap>,
    #[serde(default)]
    pub peers: Vec<VmOverlayPeer>,
    /// Direct routed VPC source/destination pairs allowed to cross machina-wg.
    #[serde(default)]
    pub direct_routes: Vec<VmOverlayRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmOverlayPeerStatus {
    pub public_key: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub allowed_ips: Vec<String>,
    /// Unix seconds of the last handshake (0 = never).
    #[serde(default)]
    pub latest_handshake: u64,
    #[serde(default)]
    pub rx_bytes: u64,
    #[serde(default)]
    pub tx_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmOverlayStatus {
    pub enabled: bool,
    #[serde(default)]
    pub interface: String,
    /// This host's WireGuard public key, once the overlay was enabled.
    #[serde(default)]
    pub public_key: String,
    #[serde(default)]
    pub listen_port: u16,
    #[serde(default)]
    pub prefixes: Vec<String>,
    #[serde(default)]
    pub mappings: Vec<VmOverlayMap>,
    #[serde(default)]
    pub peers: Vec<VmOverlayPeerStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
}

pub const CHAOS_MAX_SECS: u64 = 3600;

/// One injected network fault on a VM's taps. It ends on its own when the
/// lease runs out, whether or not anyone stops it.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct VmChaosFault {
    /// Caller's id (e.g. experiment and step); starting the same id again
    /// replaces the fault.
    pub id: String,
    #[serde(default)]
    pub vm: String,
    /// An explicit interface instead of the VM's taps (tests).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tap: Option<String>,
    /// Added latency towards the VM.
    #[serde(default)]
    pub delay_ms: u32,
    #[serde(default)]
    pub jitter_ms: u32,
    /// Packets towards the VM dropped at random.
    #[serde(default)]
    pub loss_pct: f64,
    /// Traffic between the VM and these CIDRs is dropped both ways.
    #[serde(default)]
    pub partition: Vec<String>,
    /// Lease, 1..=CHAOS_MAX_SECS.
    pub secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct VmChaosActive {
    #[serde(flatten)]
    pub fault: VmChaosFault,
    /// Interfaces it is applied to.
    #[serde(default)]
    pub taps: Vec<String>,
    /// Wall clock end (RFC 3339).
    pub until: String,
    #[serde(default)]
    pub remaining_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct VmChaosStatus {
    #[serde(default)]
    pub faults: Vec<VmChaosActive>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
}

/// A sleeping VM and the addresses whose traffic wakes it.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmWakeEntry {
    pub vm: String,
    #[serde(default)]
    pub addresses: Vec<String>,
}

/// The host's wake set: every sleeping (managed-saved) VM. Traffic to one
/// of its addresses publishes a `vm_wake` stream event.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmWake {
    #[serde(default)]
    pub entries: Vec<VmWakeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmWakeEvent {
    pub vm: String,
    pub address: String,
    /// Which hook saw the packet: "host" (sent by the host), "forward"
    /// (routed or DNATed) or "arp" (a peer on the same bridge).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub via: String,
    /// Unix seconds.
    pub at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmWakeStatus {
    #[serde(default)]
    pub entries: Vec<VmWakeEntry>,
    /// Most recent wakes, newest last.
    #[serde(default)]
    pub wakes: Vec<VmWakeEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
}

/// QEMU sandbox settings (device allowlist + egress ports). Enforcement
/// also needs the bpfd enforcement lease.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VmSandboxConfig {
    /// "observe" (default) or "enforce".
    #[serde(default = "default_observe")]
    pub mode: String,
    /// Attach to every running machine-qemu scope automatically.
    #[serde(default)]
    pub auto: bool,
    /// Extra device rules, `c|b MAJOR:MINOR|* [rwm]`.
    #[serde(default)]
    pub extra_devices: Vec<String>,
    /// QEMU egress ports besides loopback (live migration, NBD).
    #[serde(default = "default_qemu_ports")]
    pub egress_ports: Vec<String>,
}

fn default_observe() -> String {
    "observe".into()
}

fn default_qemu_ports() -> Vec<String> {
    vec!["49152-49215".into(), "10809".into()]
}

impl Default for VmSandboxConfig {
    fn default() -> Self {
        Self {
            mode: default_observe(),
            auto: false,
            extra_devices: Vec::new(),
            egress_ports: default_qemu_ports(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SandboxHit {
    pub vm: Option<String>,
    pub cgroup: Option<String>,
    /// "c 10:232 rw" for devices, "tcp 10.0.0.1:443" for egress.
    pub target: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VmSandboxStatus {
    pub config: VmSandboxConfig,
    /// The resolved device allowlist.
    pub devices: Vec<String>,
    /// VM → sandboxed cgroup path.
    pub attached: BTreeMap<String, String>,
    pub enforcing: bool,
    pub device_hits: Vec<SandboxHit>,
    pub egress_hits: Vec<SandboxHit>,
    pub notes: Vec<String>,
}

/// Opt-in TLS visibility. Both parts are off by default and rate limited.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TlsConfig {
    /// `mn_tlsfp`: sample ClientHellos on the root cgroup for JA3/JA4; also
    /// fingerprints ClientHellos seen by the tap L7 path.
    #[serde(default)]
    pub fingerprints: bool,
    #[serde(default = "default_fp_rate")]
    pub fingerprint_rate: u32,
    /// libssl uprobes: HTTP method/host/path/status from TLS plaintext
    /// heads (bodies never leave bpfd).
    #[serde(default)]
    pub ssl_uprobes: bool,
    /// Process names (`comm`) to capture; empty captures nothing unless
    /// `ssl_all_processes`.
    #[serde(default)]
    pub ssl_comms: Vec<String>,
    #[serde(default)]
    pub ssl_all_processes: bool,
    #[serde(default = "default_ssl_rate")]
    pub ssl_rate: u32,
}

fn default_fp_rate() -> u32 {
    50
}
fn default_ssl_rate() -> u32 {
    200
}

impl Default for TlsConfig {
    fn default() -> Self {
        serde_json::from_str("{}").expect("tls defaults")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TlsFingerprint {
    pub ts: String,
    /// `host` (mn_tlsfp, process egress) or `tap` (datapath L7 path).
    pub source: String,
    pub iface: Option<String>,
    pub cgroup: Option<String>,
    /// VM / pod / container owning the client.
    pub workload: Option<Workload>,
    pub client: String,
    pub server: String,
    pub server_port: u16,
    pub sni: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alpn: Vec<String>,
    pub tls_version: String,
    pub ja3: String,
    pub ja3_hash: String,
    pub ja4: String,
    /// The ClientHello did not fit in the sample; fingerprints are partial.
    pub truncated: bool,
}

/// HTTP metadata from OpenSSL plaintext (never payload bodies).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SslRecord {
    /// Owning workload (`vm`, `pod`, `container` or `service`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload: Option<Workload>,
    pub ts: String,
    pub pid: u32,
    pub comm: String,
    /// write (request side) | read
    pub direction: String,
    pub bytes: u32,
    /// http1 | http2 | other
    pub protocol: String,
    pub method: Option<String>,
    pub host: Option<String>,
    pub path: Option<String>,
    pub status: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TlsStatus {
    pub config: TlsConfig,
    /// Cgroup carrying `mn_tlsfp`.
    pub fingerprint_cgroup: Option<String>,
    /// libssl objects with uprobes attached.
    pub ssl_libraries: Vec<String>,
    pub fingerprints_seen: u64,
    pub ssl_events_seen: u64,
    pub notes: Vec<String>,
}

/// XDP DDoS shield on the uplink (shares the `mn_xdp_uplink` dispatcher
/// with the NodePort fast path, so both must use the same interface).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShieldConfig {
    #[serde(default)]
    pub iface: String,
    /// `off`, `audit` or `enforce` (drops need the enforcement lease).
    #[serde(default = "default_shield_mode")]
    pub mode: String,
    /// Protect every destination instead of `protected`.
    #[serde(default)]
    pub protect_all: bool,
    /// Protected destination addresses.
    #[serde(default)]
    pub protected: Vec<String>,
    /// Per-source packets/s per class; 0 = unlimited.
    #[serde(default = "default_syn_pps")]
    pub syn_pps: u32,
    #[serde(default = "default_udp_pps")]
    pub udp_pps: u32,
    #[serde(default = "default_icmp_pps")]
    pub icmp_pps: u32,
    #[serde(default)]
    pub other_pps: u32,
    #[serde(default = "default_burst_secs")]
    pub burst_secs: u32,
    /// Source CIDRs never limited / always dropped.
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

fn default_shield_mode() -> String {
    "off".into()
}
fn default_syn_pps() -> u32 {
    1000
}
fn default_udp_pps() -> u32 {
    5000
}
fn default_icmp_pps() -> u32 {
    100
}
fn default_burst_secs() -> u32 {
    2
}

impl Default for ShieldConfig {
    fn default() -> Self {
        serde_json::from_str("{}").expect("shield defaults")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ShieldCounters {
    pub checked: u64,
    pub passed: u64,
    pub audited: u64,
    pub dropped: u64,
    pub dropped_bytes: u64,
    pub denied: u64,
    pub malformed: u64,
    pub syn_limited: u64,
    pub udp_limited: u64,
    pub icmp_limited: u64,
    pub other_limited: u64,
}

/// A source over its rate (top talkers by hits).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShieldSource {
    pub addr: String,
    pub class: String,
    pub hits: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ShieldStatus {
    pub config: ShieldConfig,
    /// Interface carrying the dispatcher with the shield on.
    pub attached: Option<String>,
    pub enforcing: bool,
    pub stats: ShieldCounters,
    pub sources: Vec<ShieldSource>,
    pub tracked_sources: usize,
}

/// Emergency node isolation on the uplink: drop everything except the
/// allowlist, for at most `lease_secs` (mandatory, short), then fail open.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeIsoConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub iface: String,
    /// Required when enabling; renewing re-arms the deadline.
    #[serde(default)]
    pub lease_secs: Option<u64>,
    /// Count would-be drops without dropping.
    #[serde(default)]
    pub dry_run: bool,
    /// Ports allowed in either direction (local or remote side).
    #[serde(default = "default_nodeiso_tcp")]
    pub allow_tcp: Vec<u16>,
    #[serde(default)]
    pub allow_udp: Vec<u16>,
    /// Peer CIDRs that bypass isolation entirely.
    #[serde(default)]
    pub exempt: Vec<String>,
    #[serde(default = "default_true")]
    pub allow_icmp: bool,
}

/// SSH, kube-apiserver, machina daemon/controller, agent gRPC, kubelet.
fn default_nodeiso_tcp() -> Vec<u16> {
    vec![22, 6443, 5092, 5093, 50051, 10250]
}

impl Default for NodeIsoConfig {
    fn default() -> Self {
        serde_json::from_str("{}").expect("nodeiso defaults")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeIsoCounters {
    pub checked: u64,
    pub passed: u64,
    pub dropped_in: u64,
    pub dropped_out: u64,
    pub would_drop: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeIsoStatus {
    pub config: NodeIsoConfig,
    pub attached: Option<String>,
    /// Dropping right now (enabled, not dry-run, lease live).
    pub isolating: bool,
    pub lease_expires_at: Option<String>,
    pub lease_remaining_secs: Option<u64>,
    /// The last isolation ended because its lease ran out.
    pub lease_expired: bool,
    pub stats: NodeIsoCounters,
}

/// Network change audit (kprobe on `rtnetlink_rcv_msg`; observe only).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RtnlConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Keep only requests issued from the host network namespace.
    #[serde(default = "default_true")]
    pub host_netns_only: bool,
    /// Object kinds to record (`link`, `addr`, `route`, `neigh`, `rule`,
    /// `qdisc`, `class`, `filter`); empty = all but `class`.
    #[serde(default)]
    pub kinds: Vec<String>,
}

impl Default for RtnlConfig {
    fn default() -> Self {
        serde_json::from_str("{}").expect("rtnl defaults")
    }
}

/// One state-changing rtnetlink request and the process that sent it.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RtnlRecord {
    pub ts: String,
    /// `link`, `addr`, `route`, `neigh`, `rule`, `qdisc`, `class`, `filter`.
    pub kind: String,
    /// `new`, `del` or `set`.
    pub action: String,
    /// NLM_F_CREATE was set (a create rather than a change).
    pub create: bool,
    pub ifindex: Option<u32>,
    pub iface: Option<String>,
    /// Routes: destination CIDR (`default` when none).
    pub dst: Option<String>,
    pub pid: u32,
    pub tgid: u32,
    pub uid: u32,
    pub comm: String,
    pub cmdline: Option<String>,
    pub cgroup: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload: Option<Workload>,
    /// Requester's network namespace inode (unknown once the process exited).
    pub netns: Option<u64>,
    pub host_netns: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RtnlStatus {
    pub config: RtnlConfig,
    pub attached: bool,
    pub events: u64,
    pub dropped: u64,
    pub stored: usize,
    pub notes: Vec<String>,
}

/// One sampled service port.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct L7SamplePort {
    pub port: u16,
    /// `redis`, `postgres`, `mysql`, `kafka` or `http2` (gRPC).
    pub protocol: String,
}

/// Sampled plaintext L7 on a cgroup's sockets (cgroup_skb; observe only).
/// Records land in the L7 store with `protocol` set and only the
/// operation name in `method` (gRPC method path in `path`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct L7SampleConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Empty = the standard ports (6379, 5432, 3306, 9092, 50051).
    #[serde(default)]
    pub ports: Vec<L7SamplePort>,
    /// One sample per flow and direction per this many milliseconds.
    #[serde(default = "default_l7s_gap")]
    pub flow_gap_ms: u64,
    /// Host-wide samples per second.
    #[serde(default = "default_l7s_rate")]
    pub rate: u32,
}

fn default_l7s_gap() -> u64 {
    250
}

fn default_l7s_rate() -> u32 {
    200
}

impl Default for L7SampleConfig {
    fn default() -> Self {
        serde_json::from_str("{}").expect("l7 sample defaults")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct L7SampleOp {
    pub protocol: String,
    pub op: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct L7SampleStatus {
    pub config: L7SampleConfig,
    /// Cgroup the samplers are attached to.
    pub attached: Option<String>,
    pub eligible: u64,
    pub emitted: u64,
    pub rate_limited: u64,
    pub ringbuf_full: u64,
    pub load_fail: u64,
    /// Samples that didn't decode to an operation.
    pub undecoded: u64,
    /// Most frequent operations since enable.
    pub top: Vec<L7SampleOp>,
    pub notes: Vec<String>,
}

/// A non-libvirt VMM tracked as a VM (cgroup relative to /sys/fs/cgroup).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VmIntelTarget {
    pub name: String,
    pub cgroup: String,
}

/// VM runtime intelligence. Observe only; `sched_switch` is a hot path, so
/// this is off unless enabled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VmIntelConfig {
    #[serde(default)]
    pub enabled: bool,
    /// `flight` (KVM exits, vCPU run-queue latency, migrations, CPU
    /// residency), `io` (block latency, vhost), `mem` (fault + reclaim
    /// latency, first KVM entry), `topology` (per-CPU IRQ time). Empty = all.
    #[serde(default)]
    pub features: Vec<String>,
    /// Extra VMMs tracked alongside libvirt's machine-qemu scopes.
    #[serde(default)]
    pub extra: Vec<VmIntelTarget>,
}

impl Default for VmIntelConfig {
    fn default() -> Self {
        serde_json::from_str("{}").expect("vm intel defaults")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmIntelTracked {
    pub name: String,
    pub cgroup: String,
    pub processes: usize,
    pub threads: usize,
    pub vcpus: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmIntelCpu {
    pub cpu: u32,
    pub irq_ns: u64,
    pub softirq_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VmIntelStatus {
    pub config: VmIntelConfig,
    /// Attached hooks (`prog` or `prog@function`).
    pub hooks: Vec<String>,
    pub vms: Vec<VmIntelTracked>,
    /// Per-CPU interrupt time (topology).
    pub cpus: Vec<VmIntelCpu>,
    pub notes: Vec<String>,
}

/// log2 latency histogram: bucket `le_ns` counts durations below it.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct VmIntelHist {
    pub count: u64,
    pub p50_ns: u64,
    pub p99_ns: u64,
    pub buckets: Vec<VmIntelBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmIntelBucket {
    pub le_ns: u64,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmIntelExit {
    pub reason: u32,
    pub name: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VmIntelReport {
    pub name: String,
    pub exits: Vec<VmIntelExit>,
    pub runq: VmIntelHist,
    pub block: VmIntelHist,
    pub fault: VmIntelHist,
    pub reclaim: VmIntelHist,
    pub vhost_work: u64,
    pub vhost_kicks: u64,
    pub migrations: u64,
    /// vCPU run time per physical CPU.
    pub residency: Vec<VmIntelResidency>,
    /// QEMU process start → first KVM_RUN entry.
    pub boot_to_first_entry_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VmIntelResidency {
    pub cpu: u32,
    pub ns: u64,
}

fn default_guard_mode() -> String {
    "audit".into()
}

/// VMM guard (BPF-LSM on QEMU cgroups). `audit` records violations;
/// `enforce` denies them and needs `lease_secs` (it reverts to audit when
/// the lease runs out and is never persisted).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_guard_mode")]
    pub mode: String,
    #[serde(default)]
    pub lease_secs: Option<u64>,
    /// Only allowlisted binaries may be exec'd from a QEMU cgroup.
    #[serde(default = "default_true")]
    pub exec: bool,
    /// No writable+executable mappings (W^X).
    #[serde(default = "default_true")]
    pub wx: bool,
    /// Only allowlisted char devices may be opened.
    #[serde(default = "default_true")]
    pub devices: bool,
    /// Extra executables (paths) on top of the QEMU binaries found here.
    #[serde(default)]
    pub allow_exec: Vec<String>,
    /// Extra char devices (`c 10:229`, `c 240:*`) on top of QEMU's defaults.
    #[serde(default)]
    pub allow_devices: Vec<String>,
    /// VMs to guard; empty = every running VM.
    #[serde(default)]
    pub vms: Vec<String>,
    /// Non-libvirt VMMs by cgroup.
    #[serde(default)]
    pub extra: Vec<VmIntelTarget>,
}

impl Default for GuardConfig {
    fn default() -> Self {
        serde_json::from_str("{}").expect("guard defaults")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct GuardTarget {
    pub name: String,
    pub cgroup: String,
    pub cgroups: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuardStatus {
    pub config: GuardConfig,
    /// `bpf` is in /sys/kernel/security/lsm (hooks actually run).
    pub lsm_active: bool,
    pub lsm_list: String,
    pub hooks: Vec<String>,
    /// Denying right now (enforce with a live lease).
    pub enforcing: bool,
    pub lease_remaining_secs: Option<u64>,
    pub lease_expired: bool,
    pub guarded: Vec<GuardTarget>,
    pub allowed_exec: Vec<String>,
    pub allowed_devices: Vec<String>,
    pub audited: u64,
    pub denied: u64,
    pub dropped: u64,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuardRecord {
    pub ts: String,
    /// `exec`, `mprotect` or `open`.
    pub hook: String,
    pub denied: bool,
    pub vm: Option<String>,
    pub tgid: u32,
    pub pid: u32,
    pub comm: String,
    pub detail: String,
}

/// Bridge-less redirect for one VM between an outer device (pod veth or a
/// dedicated NIC) and its tap. Redirects only while the enforcement lease is
/// live; not persisted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectConfig {
    pub vm: String,
    #[serde(default)]
    pub outer_iface: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Allow a physical NIC as the outer device.
    #[serde(default)]
    pub force: bool,
    /// Tap (default: the VM's tap known to bpfd).
    #[serde(default)]
    pub tap: Option<String>,
    /// Guest MAC (default: the tap MAC with libvirt's fe: → 52: prefix).
    #[serde(default)]
    pub mac: Option<String>,
    /// Guest IPs, matched when the destination MAC isn't the guest's.
    #[serde(default)]
    pub ips: Vec<String>,
    /// Also send the VM's frames straight out of the outer device.
    #[serde(default = "default_true")]
    pub reverse: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DirectEntry {
    pub vm: String,
    pub outer_iface: String,
    pub tap: String,
    pub mac: String,
    pub ips: Vec<String>,
    pub reverse: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DirectStatus {
    pub entries: Vec<DirectEntry>,
    /// `iface:program` links.
    pub attached: Vec<String>,
    /// Redirecting right now (enforcement lease live).
    pub active: bool,
    pub redirected_in: u64,
    pub redirected_out: u64,
    /// Frames passed through because the lease wasn't live.
    pub idle: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum QuicLbMode {
    /// Rewrite MACs and bounce out of the uplink; backends hold the VIP.
    #[default]
    Dsr,
    /// IPv4-in-IPv4 to the backend (IPv4 VIPs only).
    Ipip,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct QuicLbBackend {
    pub addr: String,
    /// Next-hop MAC (default: the uplink's IPv4 neighbour entry).
    #[serde(default)]
    pub mac: Option<String>,
    /// Server id the backend encodes in its CIDs (default: derived from addr).
    #[serde(default)]
    pub server_id: Option<u16>,
}

fn default_cid_len() -> u8 {
    8
}

/// One QUIC service on the uplink XDP dispatcher. `enabled: false` removes it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuicLbConfig {
    #[serde(default)]
    pub iface: String,
    pub vip: String,
    pub port: u16,
    #[serde(default)]
    pub backends: Vec<QuicLbBackend>,
    /// Length of server-issued short-header DCIDs (3..=20).
    #[serde(default = "default_cid_len")]
    pub cid_len: u8,
    /// QUIC-LB config rotation id in the CID's top three bits (0..=6).
    #[serde(default)]
    pub config_id: u8,
    #[serde(default)]
    pub mode: QuicLbMode,
    /// Outer IPIP source (default: the uplink's first IPv4 address).
    #[serde(default)]
    pub encap_src: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct QuicLbBackendStatus {
    pub addr: String,
    pub mac: String,
    pub server_id: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuicLbServiceStatus {
    pub vip: String,
    pub port: u16,
    pub mode: QuicLbMode,
    pub cid_len: u8,
    pub config_id: u8,
    pub backends: Vec<QuicLbBackendStatus>,
    /// Routed by the server id in the CID.
    pub routed_cid: u64,
    /// Routed by Maglev on the 5-tuple.
    pub maglev: u64,
    pub initial: u64,
    pub unknown_sid: u64,
    pub tx: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuicLbStatus {
    pub iface: Option<String>,
    pub attached: bool,
    pub services: Vec<QuicLbServiceStatus>,
}

/// Attach the AF_XDP program to a dedicated interface (never one carrying a
/// default route or the uplink dispatcher).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AfxdpConfig {
    pub iface: String,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AfxdpQueue {
    pub queue: u32,
    /// Gate open (a socket was registered and not unregistered).
    pub enabled: bool,
    pub redirected: u64,
    /// Frames passed to the stack because no socket was bound (consumer gone).
    pub no_socket: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AfxdpStatus {
    pub iface: Option<String>,
    pub attached: bool,
    pub queues: Vec<AfxdpQueue>,
}

/// A process standing in for a VM (tests, non-libvirt VMMs): its `CPU n/KVM`
/// threads are scheduled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScxTarget {
    pub name: String,
    pub pid: u32,
}

/// VM-aware sched_ext scheduler. Only listed VMs' vCPU threads switch to
/// SCHED_EXT, always under a lease; never persisted.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ScxConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub lease_secs: Option<u64>,
    /// libvirt VM names.
    #[serde(default)]
    pub vms: Vec<String>,
    #[serde(default)]
    pub extra: Vec<ScxTarget>,
    /// Queue delay above which a dispatch counts as a latency violation.
    #[serde(default)]
    pub latency_target_us: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScxVmStatus {
    pub name: String,
    pub vcpus: usize,
    pub enqueues: u64,
    pub dispatches: u64,
    pub avg_queue_delay_us: f64,
    pub max_queue_delay_us: f64,
    pub runtime_ms: u64,
    pub latency_violations: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScxStatus {
    /// Kernel has sched_ext.
    pub supported: bool,
    /// Helper binary in use (None = not found).
    pub helper: Option<String>,
    pub running: bool,
    /// /sys/kernel/sched_ext/state.
    pub kernel_state: String,
    /// Name of the loaded scheduler, if any.
    pub ops: Option<String>,
    /// Tasks the kernel refused to put on SCHED_EXT since boot.
    pub nr_rejected: u64,
    pub lease_remaining_secs: Option<u64>,
    pub lease_expired: bool,
    /// Why the last run ended.
    pub last_exit: Option<String>,
    pub vms: Vec<ScxVmStatus>,
    pub notes: Vec<String>,
}


/// One normalized item in the per-VM Black Box flight recorder.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct BlackBoxEvent {
    pub ts: String,
    pub topic: String,
    pub kind: String,
    pub severity: String,
    pub summary: String,
    #[serde(default)]
    pub event: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BlackBoxIncidentStatus {
    pub id: String,
    pub triggered_at: String,
    pub reason: String,
    pub post_secs: u64,
    pub freeze_at: String,
    pub frozen: bool,
    pub automatic: bool,
    pub events: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct BlackBoxIncident {
    pub vm: String,
    pub status: BlackBoxIncidentStatus,
    pub events: Vec<BlackBoxEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BlackBoxVmStatus {
    pub vm: String,
    pub pre_secs: u64,
    pub max_events: usize,
    pub rolling_events: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incident: Option<BlackBoxIncidentStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct BlackBoxSnapshot {
    pub status: BlackBoxVmStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incident: Option<BlackBoxIncident>,
}

/// Who a record belongs to: `vm`, `pod`, `container` or `service`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Workload {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ns: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Status,
    ListPolicies,
    ApplyPolicy {
        policy: Policy,
    },
    RemovePolicy {
        id: String,
    },
    SetMode {
        mode: Mode,
        #[serde(default)]
        lease_secs: Option<u64>,
    },
    ListInterfaces,
    AttachInterface {
        name: String,
        #[serde(default)]
        guest_side: bool,
        #[serde(default)]
        xdp: bool,
    },
    DetachInterface {
        name: String,
    },
    Flows {
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        vm: Option<String>,
    },
    Events {
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        kind: Option<String>,
    },
    Dns {
        #[serde(default)]
        limit: Option<usize>,
    },
    L7 {
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        vm: Option<String>,
        /// tls | http | ssh
        #[serde(default)]
        protocol: Option<String>,
    },
    /// Per-VM traffic totals since `since` (or since the last reset).
    Accounting {
        #[serde(default)]
        vm: Option<String>,
    },
    ResetAccounting {
        #[serde(default)]
        vm: Option<String>,
    },
    ProcEvents {
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        kind: Option<String>,
    },
    Anomalies {
        #[serde(default)]
        limit: Option<usize>,
    },
    NetHealth,
    CaptureStart {
        iface: String,
        #[serde(default)]
        duration_secs: Option<u64>,
        #[serde(default)]
        sample: Option<u32>,
        #[serde(default)]
        snaplen: Option<u32>,
        #[serde(default)]
        max_packets: Option<usize>,
    },
    CaptureList,
    CaptureGet {
        id: String,
    },
    SetQos {
        #[serde(default)]
        iface: Option<String>,
        #[serde(default)]
        vm: Option<String>,
        /// Bits/s sent by the workload; 0 = unlimited.
        #[serde(default)]
        egress_bps: u64,
        /// Bits/s towards the workload; 0 = unlimited.
        #[serde(default)]
        ingress_bps: u64,
    },
    GetTelemetry,
    SetTelemetry {
        telemetry: TelemetryConfig,
    },
    /// Node address (NodePort matching) and the uplink that gets the NodePort
    /// classifier; also attaches socket-level service load balancing.
    CniConfigure {
        #[serde(flatten)]
        config: CniNodeConfig,
    },
    CniAddEndpoint {
        endpoint: CniEndpoint,
    },
    CniDelEndpoint {
        ip: String,
    },
    CniSync {
        state: CniState,
    },
    CniStatus,
    CniServices,
    /// VM group identities, policy and rate limits on VM taps.
    VmEdgeSync {
        state: VmEdgeState,
    },
    VmEdgeStatus,
    /// `toFQDNs` bindings learned from DNS replies to VMs.
    VmFqdnCache,
    /// Mutual-authentication table of the VM edge.
    VmAuthTable,
    /// This host's authentication key (created on first use) as a CSR, and
    /// the certificate it holds.
    VmAuthIdentity,
    /// Install the controller-signed host certificate and CA.
    VmAuthCert {
        host_id: String,
        ca_pem: String,
        cert_pem: String,
        not_after: i64,
    },
    /// Handshake with another host's bpfd (diagnostics; no identities).
    VmAuthProbe {
        host_id: String,
        address: String,
    },
    /// Recent VM edge verdicts, newest first.
    VmFlows {
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        vm: Option<String>,
        #[serde(default)]
        verdict: Option<String>,
    },
    /// Flow history edges (persisted, 7 days).
    VmFlowEdges {
        #[serde(default)]
        vm: Option<String>,
    },
    VmFlowEdgesReset {},
    /// Recent detection alerts, newest first.
    VmFlowAlerts {
        #[serde(default)]
        limit: Option<usize>,
    },
    /// Quarantine a VM for `secs` (1 .. QUARANTINE_MAX_SECS); replaces an
    /// existing quarantine of the same VM.
    VmQuarantine {
        vm: String,
        secs: u64,
        #[serde(default)]
        allow: Vec<VmQuarantineAllow>,
        #[serde(default)]
        reason: String,
        #[serde(default)]
        by: String,
    },
    VmQuarantineRelease {
        vm: String,
    },
    VmQuarantines,
    /// Create or replace one DNS threat feed. While any feed exists, every
    /// running VM's DNS replies are checked.
    VmThreatFeedSet {
        name: String,
        #[serde(default)]
        source: String,
        #[serde(default)]
        block: bool,
        domains: Vec<String>,
    },
    VmThreatFeedRemove {
        name: String,
    },
    VmThreatFeeds,
    /// Replace the egress SNAT rules (nftables table `machina_egress`).
    VmEgressSnatSet {
        config: VmEgressSnat,
    },
    VmEgressSnatStatus,
    /// Replace the WireGuard overlay config (interface `machina-wg`).
    VmOverlaySet {
        config: VmOverlay,
    },
    VmOverlayStatus,
    /// Inject a network fault on a VM for a lease.
    VmChaosStart {
        fault: VmChaosFault,
    },
    /// End faults now: one id, or every id starting with `prefix`.
    VmChaosStop {
        #[serde(default)]
        id: String,
        #[serde(default)]
        prefix: String,
    },
    VmChaosStatus,
    /// Replace the wake set (nftables tables `machina_wake`).
    VmWakeSet {
        config: VmWake,
    },
    VmWakeStatus,
    VmSandboxConfigure {
        config: VmSandboxConfig,
    },
    /// Sandbox one VM's QEMU (`cgroup` relative to /sys/fs/cgroup; default:
    /// its machine-qemu scope).
    VmSandboxAttach {
        vm: String,
        #[serde(default)]
        cgroup: Option<String>,
    },
    VmSandboxDetach {
        vm: String,
    },
    VmSandboxStatus,
    /// Re-follow VM taps and QEMU scopes now (sent on VM start/stop).
    VmRefresh,
    ShieldConfigure {
        config: ShieldConfig,
    },
    ShieldStatus,
    NodeIsoConfigure {
        config: NodeIsoConfig,
    },
    NodeIsoStatus,
    IcmpErrors,
    TlsConfigure {
        config: TlsConfig,
    },
    TlsStatus,
    TlsFingerprints {
        #[serde(default)]
        limit: Option<usize>,
    },
    SslEvents {
        #[serde(default)]
        limit: Option<usize>,
    },
    RtnlConfigure {
        config: RtnlConfig,
    },
    RtnlStatus,
    RtnlEvents {
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        iface: Option<String>,
    },
    L7SampleConfigure {
        config: L7SampleConfig,
    },
    L7SampleStatus,
    VmIntelConfigure {
        config: VmIntelConfig,
    },
    VmIntelStatus,
    VmIntelVm {
        name: String,
    },
    GuardConfigure {
        config: GuardConfig,
    },
    GuardStatus,
    GuardEvents {
        #[serde(default)]
        limit: Option<usize>,
    },
    DirectConfigure {
        config: DirectConfig,
    },
    DirectStatus,
    QuicLbConfigure {
        config: QuicLbConfig,
    },
    QuicLbStatus,
    AfxdpConfigure {
        config: AfxdpConfig,
    },
    /// Must carry the consumer's AF_XDP socket as SCM_RIGHTS ancillary data
    /// on the same message; the socket must be bound to (iface, queue).
    AfxdpRegister {
        iface: String,
        queue: u32,
    },
    AfxdpUnregister {
        iface: String,
        queue: u32,
    },
    AfxdpStatus,
    ScxConfigure {
        config: ScxConfig,
    },
    ScxStatus,
    BlackBoxList,
    BlackBoxGet {
        vm: String,
    },
    BlackBoxTrigger {
        vm: String,
        #[serde(default)]
        post_secs: Option<u64>,
        #[serde(default)]
        reason: String,
    },
    BlackBoxClear {
        vm: String,
    },
    /// Stream events (`net`, `dns`, `l7`, `proc`, `anomaly`) as JSON lines
    /// until the client disconnects.
    Subscribe {
        topics: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(data: impl Serialize) -> Self {
        Self {
            ok: true,
            data: serde_json::to_value(data).unwrap_or(Value::Null),
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: Value::Null,
            error: Some(msg.into()),
        }
    }
}

/// One streamed event on a `Subscribe` connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEvent {
    pub topic: String,
    pub event: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_wire_format() {
        let r: Request = serde_json::from_str(
            r#"{"op":"apply_policy","policy":{"id":"p1","kind":"deny_ip","match":"10.0.0.0/8","scope":{"kind":"vm","name":"web"}}}"#,
        )
        .unwrap();
        match r {
            Request::ApplyPolicy { policy } => {
                assert!(policy.enabled);
                assert_eq!(policy.scope, Scope::Vm { name: "web".into() });
                assert_eq!(policy.scope.key().as_deref(), Some("vm:web"));
            }
            other => panic!("unexpected {other:?}"),
        }
        let s = serde_json::to_string(&Request::SetMode {
            mode: Mode::Enforce,
            lease_secs: Some(60),
        })
        .unwrap();
        assert_eq!(s, r#"{"op":"set_mode","mode":"enforce","lease_secs":60}"#);
        let r: Request = serde_json::from_str(r#"{"op":"l7","protocol":"tls"}"#).unwrap();
        assert!(matches!(r, Request::L7 { protocol: Some(ref p), .. } if p == "tls"));
        let r: Request = serde_json::from_str(r#"{"op":"accounting"}"#).unwrap();
        assert!(matches!(r, Request::Accounting { vm: None }));
    }

    #[test]
    fn scope_defaults_to_host() {
        let p: Policy =
            serde_json::from_str(r#"{"id":"x","kind":"deny_process","match":"/usr/bin/nc"}"#)
                .unwrap();
        assert_eq!(p.scope, Scope::Host);
        assert_eq!(p.scope.label(), "host");
    }
}
