// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Shared layouts between `machina-bpf-ebpf` (kernel) and `machina-bpf` (userspace).
//!
//! Every struct is `#[repr(C)]` with explicit padding so map keys never carry
//! uninitialised bytes. Addresses are always 16 bytes: IPv4 is stored as an
//! IPv4-mapped IPv6 address (`::ffff:a.b.c.d`).

#![no_std]

pub const ADDR_LEN: usize = 16;
pub const COMM_LEN: usize = 16;
pub const PATH_LEN: usize = 256;
pub const DNS_PAYLOAD_LEN: usize = 512;
pub const L7_PAYLOAD_LEN: usize = 512;
pub const CAPTURE_SNAPLEN: usize = 1024;
pub const FILE_WATCH_SLOTS: u32 = 8;
pub const FILE_WATCH_PREFIX_LEN: usize = 64;

/// Bits of the IPv4-mapped prefix (`::ffff:0:0/96`).
pub const V4_MAPPED_PREFIX_BITS: u32 = 96;

// ---------------------------------------------------------------------------
// Global config (CONFIG array, index 0)
// ---------------------------------------------------------------------------

pub const MODE_OBSERVE: u32 = 0;
pub const MODE_ENFORCE: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlobalCfg {
    /// `MODE_OBSERVE` or `MODE_ENFORCE`.
    pub mode: u32,
    pub flags: u32,
    /// CLOCK_MONOTONIC deadline (ns). Enforce mode is only honoured while
    /// `bpf_ktime_get_ns() < lease_deadline_ns`; the datapath fails open to
    /// observe on its own when the lease lapses, even if the loader is gone.
    pub lease_deadline_ns: u64,
}

pub const GLOBAL_EXEC_DENY: u32 = 1 << 0;
pub const GLOBAL_FILE_WATCH: u32 = 1 << 1;
pub const GLOBAL_CONNECT_EVENTS: u32 = 1 << 2;
pub const GLOBAL_FORK_EVENTS: u32 = 1 << 3;
pub const GLOBAL_EXEC_EVENTS: u32 = 1 << 4;
pub const GLOBAL_CAP_DENY: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Per-interface config (IFACE_CFG hash, key = ifindex)
// ---------------------------------------------------------------------------

/// Ingress hook on this interface carries traffic *from* the workload
/// (true for VM taps / pod host-side veths, false for uplinks).
pub const IF_GUEST_SIDE: u32 = 1 << 0;
pub const IF_DENY: u32 = 1 << 1;
/// Default-deny workload egress unless an ALLOW rule matches.
pub const IF_ALLOW: u32 = 1 << 2;
pub const IF_FLOWS: u32 = 1 << 3;
pub const IF_CAPTURE: u32 = 1 << 4;
pub const IF_QOS: u32 = 1 << 5;
pub const IF_DNS: u32 = 1 << 6;
/// Token-bucket limit on new workload-initiated connections (`rate_limit`).
pub const IF_RATE: u32 = 1 << 7;
/// Report the first client payload of each TCP flow (TLS SNI / HTTP request).
pub const IF_L7: u32 = 1 << 8;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfaceCfg {
    /// Policy scope id (0 = host-global only).
    pub scope: u32,
    pub flags: u32,
    /// Rate of traffic sent by the workload, bytes/s. 0 = unlimited.
    /// Paced with EDT (needs fq) on egress hooks, policed on ingress hooks.
    pub qos_egress_bps: u64,
    /// Rate of traffic towards the workload, bytes/s. 0 = unlimited.
    pub qos_ingress_bps: u64,
    /// Capture 1-in-N packets (0 or 1 = every packet).
    pub capture_sample: u32,
    pub capture_snaplen: u32,
}

// ---------------------------------------------------------------------------
// Enforcement keys
// ---------------------------------------------------------------------------

/// LPM data for DENY_LPM. Prefix length = 32 (scope) + address bits.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DenyKey {
    pub scope: [u8; 4],
    pub addr: [u8; ADDR_LEN],
}

/// LPM data for ALLOW_LPM. Prefix length = 64 (scope+proto+pad+port) + address bits.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllowKey {
    pub scope: [u8; 4],
    /// IPPROTO_TCP / IPPROTO_UDP, 0 = any.
    pub proto: u8,
    pub _pad: u8,
    /// Network byte order, 0 = any.
    pub port: [u8; 2],
    pub addr: [u8; ADDR_LEN],
}

pub const DENY_KEY_FIXED_BITS: u32 = 32;
pub const ALLOW_KEY_FIXED_BITS: u32 = 64;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuleVal {
    pub policy_id: u32,
    pub flags: u32,
}

// ---------------------------------------------------------------------------
// Flows
// ---------------------------------------------------------------------------

/// Flow key from the workload's point of view.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FlowKey {
    pub ifindex: u32,
    pub proto: u8,
    pub _pad: [u8; 3],
    pub local_port: u16,
    pub remote_port: u16,
    pub _pad2: u32,
    pub local: [u8; ADDR_LEN],
    pub remote: [u8; ADDR_LEN],
}

pub const ORIGIN_LOCAL: u8 = 1;
pub const ORIGIN_REMOTE: u8 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlowVal {
    pub first_ns: u64,
    pub last_ns: u64,
    pub tx_pkts: u64,
    pub tx_bytes: u64,
    pub rx_pkts: u64,
    pub rx_bytes: u64,
    pub tcp_flags: u32,
    pub origin: u8,
    pub verdict: u8,
    pub _pad: u16,
}

/// For FLOW_OPEN, `NetEvent::policy_id` carries the flow origin (ORIGIN_*).
pub const NET_EV_FLOW_OPEN: u32 = 1;
pub const NET_EV_FLOW_CLOSE: u32 = 2;
pub const NET_EV_DENY: u32 = 3;
pub const NET_EV_ALLOW_MISS: u32 = 4;
pub const NET_EV_QOS_DROP: u32 = 5;
pub const NET_EV_RATE_LIMIT: u32 = 6;

/// Set in `FlowVal::tcp_flags` (above the 8 TCP flag bits) once the flow's
/// first client payload has been reported on L7_EVENTS.
pub const FLOW_L7_SEEN: u32 = 1 << 16;

/// RATE_CFG value (key = policy scope): new connections per second + burst.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RateCfg {
    pub per_sec: u32,
    pub burst: u32,
    pub policy_id: u32,
    pub _pad: u32,
}

/// IFACE_STATS per-CPU value (key = ifindex), workload point of view.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfaceStats {
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub tx_pkts: u64,
    pub rx_pkts: u64,
    pub drops: u64,
}

pub const VERDICT_PASS: u32 = 0;
pub const VERDICT_DROP: u32 = 1;
/// Would have dropped, but the datapath is in observe mode (or lease expired).
pub const VERDICT_OBSERVED: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetEvent {
    pub ts_ns: u64,
    pub kind: u32,
    pub verdict: u32,
    pub policy_id: u32,
    pub pkt_len: u32,
    pub key: FlowKey,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}

// ---------------------------------------------------------------------------
// DNS / capture
// ---------------------------------------------------------------------------

pub const DIR_FROM_WORKLOAD: u32 = 1;
pub const DIR_TO_WORKLOAD: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DnsEvent {
    pub ts_ns: u64,
    pub ifindex: u32,
    pub dir: u32,
    pub payload_len: u32,
    /// DNS_EV_TCP: payload is a DNS-over-TCP segment (length prefix first).
    pub flags: u32,
    pub local: [u8; ADDR_LEN],
    pub remote: [u8; ADDR_LEN],
    pub payload: [u8; DNS_PAYLOAD_LEN],
}

pub const DNS_EV_TCP: u32 = 1;

/// First client→server payload of a TCP flow (`key` from the workload's view).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct L7Event {
    pub ts_ns: u64,
    pub ifindex: u32,
    pub dir: u32,
    pub payload_len: u32,
    pub _pad: u32,
    pub key: FlowKey,
    pub payload: [u8; L7_PAYLOAD_LEN],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CaptureEvent {
    pub ts_ns: u64,
    pub ifindex: u32,
    pub dir: u32,
    pub pkt_len: u32,
    pub cap_len: u32,
    pub data: [u8; CAPTURE_SNAPLEN],
}

// ---------------------------------------------------------------------------
// Process telemetry
// ---------------------------------------------------------------------------

pub const PROC_EV_EXEC: u32 = 1;
pub const PROC_EV_EXIT: u32 = 2;
pub const PROC_EV_FORK: u32 = 3;
pub const PROC_EV_FILE_OPEN: u32 = 4;
pub const PROC_EV_CONNECT: u32 = 5;

pub const PROC_FLAG_EXEC_DENIED: u32 = 1 << 0;
pub const PROC_FLAG_EXEC_KILLED: u32 = 1 << 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProcEvent {
    pub ts_ns: u64,
    pub cgroup_id: u64,
    pub kind: u32,
    pub pid: u32,
    pub tgid: u32,
    /// Child pid for FORK, 0 otherwise.
    pub child_pid: u32,
    pub uid: u32,
    pub gid: u32,
    pub flags: u32,
    pub policy_id: u32,
    /// openat flags for FILE_OPEN.
    pub open_flags: u32,
    pub family: u16,
    pub proto: u16,
    pub sport: u16,
    pub dport: u16,
    pub _pad: u32,
    pub saddr: [u8; ADDR_LEN],
    pub daddr: [u8; ADDR_LEN],
    pub comm: [u8; COMM_LEN],
    pub path: [u8; PATH_LEN],
}

/// A path prefix watched by the openat tracepoint (FILE_WATCH array).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FileWatch {
    pub len: u32,
    /// `WATCH_DENY`: SIGKILL the opener while enforcement is active.
    pub flags: u32,
    pub policy_id: u32,
    pub _pad: u32,
    pub prefix: [u8; FILE_WATCH_PREFIX_LEN],
}

pub const WATCH_DENY: u32 = 1;

/// DENY_PORTS key: remote port deny (`deny_port`). `proto` 0 = any.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PortKey {
    pub scope: u32,
    pub proto: u8,
    pub _pad: u8,
    /// Network byte order.
    pub port: [u8; 2],
}

/// CAP_DENY key (`deny_cap`): capability number within a scope.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CapKey {
    pub scope: u32,
    pub cap: u32,
}

pub const PROC_EV_CAP_DENIED: u32 = 6;

/// Tracepoint field offsets discovered from tracefs `format` files at load time
/// (TP_OFF array). The kernel side never hard-codes tracepoint layouts.
pub mod tp {
    pub const EXEC_FILENAME_LOC: u32 = 0;
    pub const FORK_PARENT_PID: u32 = 1;
    pub const FORK_CHILD_PID: u32 = 2;
    pub const OPENAT_FILENAME: u32 = 3;
    pub const OPENAT_FLAGS: u32 = 4;
    pub const ISS_OLDSTATE: u32 = 5;
    pub const ISS_NEWSTATE: u32 = 6;
    pub const ISS_SPORT: u32 = 7;
    pub const ISS_DPORT: u32 = 8;
    pub const ISS_FAMILY: u32 = 9;
    pub const ISS_PROTOCOL: u32 = 10;
    pub const ISS_SADDR_V6: u32 = 11;
    pub const ISS_DADDR_V6: u32 = 12;
    pub const KFREE_REASON: u32 = 13;
    pub const RETRANS_SADDR_V6: u32 = 14;
    pub const SEND_RST_SADDR_V6: u32 = 15;
    pub const RECV_RST_SADDR_V6: u32 = 16;
    pub const COUNT: u32 = 17;
    /// Sentinel for "field not present on this kernel".
    pub const MISSING: u32 = u32::MAX;
}

// ---------------------------------------------------------------------------
// Network health
// ---------------------------------------------------------------------------

pub const HEALTH_RETRANSMIT: u32 = 1;
pub const HEALTH_RST_SENT: u32 = 2;
pub const HEALTH_RST_RECV: u32 = 3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct HealthKey {
    pub kind: u32,
    pub _pad: u32,
    pub addr: [u8; ADDR_LEN],
}

/// Connect latency histogram bucket upper bounds (µs); the last is open.
pub const CONNECT_BUCKETS_US: [u64; 7] = [100, 1_000, 5_000, 10_000, 50_000, 100_000, 1_000_000];
pub const CONNECT_BUCKETS: usize = 8;

/// CONNECT_HEALTH key: remote address + port (host order).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ConnKey {
    pub addr: [u8; ADDR_LEN],
    pub port: u16,
    pub _pad: [u8; 6],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConnStats {
    /// Established active connects.
    pub count: u64,
    /// SYN_SENT → CLOSE (refused / timed out).
    pub failures: u64,
    pub sum_us: u64,
    pub max_us: u64,
    pub hist: [u64; CONNECT_BUCKETS],
}

/// TCP_PRESSURE: last socket snapshot towards a remote address.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TcpPressure {
    /// Smoothed RTT ×8 (as the kernel keeps it), µs.
    pub srtt_us8: u32,
    pub cwnd: u32,
    pub ssthresh: u32,
    pub mss: u32,
    pub total_retrans: u32,
    /// Retransmit callbacks seen across sockets to this peer.
    pub retrans_events: u32,
    pub rate_delivered: u32,
    pub rate_interval_us: u32,
    pub last_ns: u64,
}

pub const ICMP_ERR_UNREACH: u8 = 1;
pub const ICMP_ERR_TIME_EXCEEDED: u8 = 2;
pub const ICMP_ERR_PARAM: u8 = 3;
/// IPv4 fragmentation needed / IPv6 packet too big.
pub const ICMP_ERR_PTB: u8 = 4;

/// ICMP_ERRORS key (value: count).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct IcmpErrKey {
    pub ifindex: u32,
    pub kind: u8,
    pub code: u8,
    pub from_workload: u8,
    pub v6: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QosState {
    pub tokens: u64,
    pub last_ns: u64,
    pub next_tstamp: u64,
}

// ---------------------------------------------------------------------------
// CNI datapath
// ---------------------------------------------------------------------------

pub const IDENTITY_HOST: u32 = 1;
pub const IDENTITY_WORLD: u32 = 2;
/// Addresses a blocking DNS threat feed resolved (bpfd runtime only).
pub const IDENTITY_THREAT: u32 = 4;

pub const EP_INGRESS_ISOLATED: u32 = 1 << 0;
pub const EP_EGRESS_ISOLATED: u32 = 1 << 1;

/// Version of the CNI map/state ABI; bpfd rejects `CniSync` from agents
/// built against another one. Addresses are 16-byte (IPv4-mapped for v4).
pub const CNI_ABI_VERSION: u32 = 2;

/// Local pod endpoint (CNI_ENDPOINTS, key = 16-byte pod address).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Endpoint {
    pub host_ifindex: u32,
    pub identity: u32,
    pub flags: u32,
    pub _pad: u32,
    pub pod_mac: [u8; 6],
    pub host_mac: [u8; 6],
    pub _pad2: u32,
}

/// NetworkPolicy verdict key (CNI_POLICY). `peer_identity` 0 = any peer,
/// `port` 0 = any port, `proto` 0 = any protocol.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PolicyKey {
    pub subject_identity: u32,
    pub peer_identity: u32,
    /// 0 = ingress to subject, 1 = egress from subject.
    pub direction: u8,
    pub proto: u8,
    /// Network byte order.
    pub port: [u8; 2],
}

pub const POLICY_INGRESS: u8 = 0;
pub const POLICY_EGRESS: u8 = 1;
/// OR-ed into the direction of VM quarantine allowlist entries (VM_POLICY).
pub const POLICY_QUARANTINE: u8 = 2;

/// ClusterIP / NodePort service frontend (CNI_SERVICES / CNI_NODEPORTS).
/// NodePorts use the unspecified address of their family
/// (`::ffff:0.0.0.0` for IPv4, `::` for IPv6).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SvcKey {
    pub addr: [u8; ADDR_LEN],
    /// Network order.
    pub port: [u8; 2],
    pub proto: u8,
    pub _pad: u8,
}

/// Session affinity by client address (`sessionAffinity: ClientIP`).
pub const SVC_F_AFFINITY: u32 = 1 << 0;
/// externalTrafficPolicy=Local: NodePort only uses this node's backends.
pub const SVC_F_LOCAL: u32 = 1 << 1;

/// Maglev lookup table size per service (prime, ≥ 10× the backends of any
/// realistic service so disruption on backend churn stays near 1/N).
pub const MAGLEV_M: u32 = 1021;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SvcVal {
    pub svc_id: u32,
    pub backend_count: u32,
    pub flags: u32,
    pub affinity_secs: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BackendKey {
    pub svc_id: u32,
    pub index: u32,
}

/// Backend lives on another node (NodePort needs SNAT or DSR to reach it).
pub const BE_F_REMOTE: u8 = 1 << 0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Backend {
    pub addr: [u8; ADDR_LEN],
    pub port: [u8; 2],
    pub flags: u8,
    pub _pad: u8,
    /// Remote node address hosting the backend (DSR encap target).
    pub node: [u8; ADDR_LEN],
}

/// Maglev slot (CNI_MAGLEV): `slot` in [0, MAGLEV_M) → backend index.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MaglevKey {
    pub svc_id: u32,
    pub slot: u32,
}

/// Session affinity entry (CNI_AFFINITY): (service, client) → backend index.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AffinityKey {
    pub client: [u8; ADDR_LEN],
    pub svc_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AffinityVal {
    pub index: u32,
    pub _pad: u32,
    pub last_ns: u64,
}

/// UDP reverse translation for socket LB (CNI_UDP_REVNAT).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RevNatKey {
    pub cookie: u64,
    pub addr: [u8; ADDR_LEN],
    pub port: [u8; 2],
    pub _pad: [u8; 6],
}

/// NodePort DNAT conntrack (CNI_NODEPORT_CT): key is the backend-side tuple
/// as seen on the reply path.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NatCtKey {
    pub backend_addr: [u8; ADDR_LEN],
    pub client_addr: [u8; ADDR_LEN],
    pub backend_port: [u8; 2],
    pub client_port: [u8; 2],
    pub proto: u8,
    pub _pad: [u8; 3],
}

/// The client address was replaced by the node address (eTP=Cluster SNAT).
pub const NAT_F_SNAT: u16 = 1 << 0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NatCtVal {
    pub frontend_addr: [u8; ADDR_LEN],
    pub frontend_port: [u8; 2],
    pub flags: u16,
    pub _pad: u32,
    /// Original client address when `NAT_F_SNAT` is set.
    pub client_addr: [u8; ADDR_LEN],
    pub last_ns: u64,
}

/// NodePort to remote backends: 0 = SNAT through this node, 1 = DSR (IPIP).
pub const NODE_F_DSR: u32 = 1 << 0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeCfg {
    /// Node primary IPv4 (IPv4-mapped) for NodePort matching.
    pub node_addr: [u8; ADDR_LEN],
    /// Node primary IPv6, all-zero when the node is single-stack.
    pub node_addr6: [u8; ADDR_LEN],
    pub flags: u32,
    pub uplink_ifindex: u32,
}

// ---------------------------------------------------------------------------
// Uplink XDP dispatcher
// ---------------------------------------------------------------------------

pub const XDP_SLOTS: u32 = 4;
pub const XDP_SLOT_NODEPORT: u32 = 0;
pub const XDP_SLOT_QUICLB: u32 = 1;

pub const XDP_F_SHIELD: u32 = 1 << 0;
pub const XDP_F_NODEPORT: u32 = 1 << 1;
pub const XDP_F_NODEISO: u32 = 1 << 2;
pub const XDP_F_QUICLB: u32 = 1 << 3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XdpCfg {
    pub flags: u32,
    pub _pad: u32,
}

// ---------------------------------------------------------------------------
// QUIC connection-ID load balancer (uplink XDP tail call)
// ---------------------------------------------------------------------------

pub const QLB_MAX_SVCS: u32 = 64;
pub const QLB_MAX_BACKENDS: u32 = 256;
/// L2 direct server return: rewrite MACs, XDP_TX.
pub const QLB_MODE_DSR: u8 = 0;
/// IPv4-in-IPv4 toward the backend (IPv4 VIPs only).
pub const QLB_MODE_IPIP: u8 = 1;

/// QLB_SVCS key: VIP (v4-mapped or v6) + UDP port (network order).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct QlbSvcKey {
    pub addr: [u8; ADDR_LEN],
    pub port: [u8; 2],
    pub _pad: [u8; 2],
}

/// CIDs whose first byte has `config_id` in its top three bits carry a
/// 16-bit server id in bytes 1..3 (QUIC-LB plaintext layout).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QlbSvc {
    pub svc_id: u32,
    pub backend_count: u32,
    pub cid_len: u8,
    pub mode: u8,
    pub config_id: u8,
    pub _pad: u8,
    pub src_mac: [u8; 6],
    pub _pad2: [u8; 2],
    /// Outer source for IPIP.
    pub encap_src: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct QlbBeKey {
    pub svc_id: u32,
    pub idx: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QlbBackend {
    pub addr: [u8; ADDR_LEN],
    pub mac: [u8; 6],
    pub _pad: [u8; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct QlbSidKey {
    pub svc_id: u32,
    pub sid: u16,
    pub _pad: u16,
}

/// QLB_STATS[svc_id * QLB_STAT_SLOTS + stat] (per-CPU u64).
pub const QLB_STAT_CID: u32 = 0;
pub const QLB_STAT_MAGLEV: u32 = 1;
pub const QLB_STAT_UNKNOWN_SID: u32 = 2;
pub const QLB_STAT_INITIAL: u32 = 3;
pub const QLB_STAT_TX: u32 = 4;
pub const QLB_STAT_ERR: u32 = 5;
pub const QLB_STAT_SLOTS: u32 = 8;

// ---------------------------------------------------------------------------
// AF_XDP fast path (dedicated interface only)
// ---------------------------------------------------------------------------

pub const AFXDP_MAX_QUEUES: u32 = 64;
/// AFXDP_STATS[queue * AFXDP_STAT_SLOTS + stat] (per-CPU u64).
pub const AFXDP_STAT_REDIRECT: u32 = 0;
/// Gate open but no socket bound to the queue: passed to the stack.
pub const AFXDP_STAT_NOSOCK: u32 = 1;
pub const AFXDP_STAT_SLOTS: u32 = 2;

/// The server id a CID routes to, if it carries one for `config_id`.
pub fn qlb_cid_sid(cid: &[u8; 3], config_id: u8) -> Option<u16> {
    (cid[0] >> 5 == config_id).then(|| u16::from_be_bytes([cid[1], cid[2]]))
}

// ---------------------------------------------------------------------------
// Node isolation (uplink drop-all with an allowlist, under its own lease)
// ---------------------------------------------------------------------------

/// NODEISO_CFG[0]. Drops happen only while `now < deadline_ns`
/// (CLOCK_MONOTONIC) and `dry_run == 0`; a zeroed config passes everything.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeIsoCfg {
    pub enabled: u32,
    pub dry_run: u32,
    pub allow_icmp: u32,
    pub _pad: u32,
    pub deadline_ns: u64,
}

/// NODEISO_PORTS key: `proto << 16 | port`.
#[inline(always)]
pub const fn nodeiso_port_key(proto: u8, port: u16) -> u32 {
    (proto as u32) << 16 | port as u32
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeIsoStats {
    pub checked: u64,
    pub passed: u64,
    pub dropped_in: u64,
    pub dropped_out: u64,
    pub would_drop: u64,
}

// ---------------------------------------------------------------------------
// TLS fingerprints (mn_tlsfp) and OpenSSL uprobes (mn_ssl_*), both opt-in
// ---------------------------------------------------------------------------

/// ClientHello bytes captured per fingerprint sample (power of two).
pub const TLSFP_LEN: usize = 2048;
/// SSL_read/SSL_write plaintext bytes captured (HTTP heads only leave bpfd).
pub const SSL_DATA_LEN: usize = 256;
pub const SSL_DIR_WRITE: u32 = 1;
pub const SSL_DIR_READ: u32 = 2;

/// TLSFP_CFG[0] / SSL_CFG[0]. `rate` = samples per second, host-wide.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SampleCfg {
    pub enabled: u32,
    pub rate: u32,
    /// SSL: capture every process instead of the SSL_COMMS allowlist.
    pub all: u32,
    pub _pad: u32,
}

/// Global sampling token bucket (milli-tokens).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SampleBucket {
    pub tokens: u64,
    pub last_ns: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TlsFpEvent {
    pub ts_ns: u64,
    pub cgroup: u64,
    /// Payload bytes in the segment / bytes captured.
    pub len: u32,
    pub cap_len: u32,
    pub sport: u16,
    pub dport: u16,
    pub v6: u8,
    pub _pad: [u8; 3],
    pub src: [u8; ADDR_LEN],
    pub dst: [u8; ADDR_LEN],
    pub data: [u8; TLSFP_LEN],
}

/// SSL_read(_ex) arguments saved between entry and return.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SslReadArgs {
    pub buf: u64,
    /// `size_t *readbytes` of SSL_read_ex; 0 for SSL_read.
    pub readbytes: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SslEvent {
    pub ts_ns: u64,
    pub pid_tgid: u64,
    pub len: u32,
    pub dir: u32,
    pub cap_len: u32,
    pub _pad: u32,
    pub comm: [u8; 16],
    pub data: [u8; SSL_DATA_LEN],
}

// ---------------------------------------------------------------------------
// XDP DDoS shield (inline in the uplink dispatcher)
// ---------------------------------------------------------------------------

pub const SHIELD_OFF: u32 = 0;
pub const SHIELD_AUDIT: u32 = 1;
/// Drops need the enforcement lease too; without it enforce acts as audit.
pub const SHIELD_ENFORCE: u32 = 2;

pub const SHIELD_CLASS_SYN: u8 = 0;
pub const SHIELD_CLASS_UDP: u8 = 1;
pub const SHIELD_CLASS_ICMP: u8 = 2;
pub const SHIELD_CLASS_OTHER: u8 = 3;
pub const SHIELD_CLASSES: usize = 4;

/// SHIELD_CFG[0]. `pps[class]` per source; 0 = unlimited.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShieldCfg {
    pub mode: u32,
    pub protect_all: u32,
    pub pps: [u32; SHIELD_CLASSES],
    /// Bucket depth in seconds of `pps`.
    pub burst_secs: u32,
    pub _pad: u32,
}

/// Per-source, per-class bucket (SHIELD_SOURCES, LRU).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ShieldSrcKey {
    pub addr: [u8; ADDR_LEN],
    pub class: u8,
    pub _pad: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShieldSrcState {
    /// Milli-tokens.
    pub tokens: u64,
    pub last_ns: u64,
    /// Packets over the rate (dropped or audited).
    pub hits: u64,
}

/// SHIELD_STATS (per-CPU, one slot).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShieldStats {
    /// Packets towards a protected destination.
    pub checked: u64,
    pub passed: u64,
    /// Would have been dropped (audit, or enforce without a lease).
    pub audited: u64,
    pub dropped: u64,
    pub dropped_bytes: u64,
    /// Source in the deny CIDR set.
    pub denied: u64,
    pub malformed: u64,
    pub limited: [u64; SHIELD_CLASSES],
}

// ---------------------------------------------------------------------------
// VM edge (tc on VM taps) and QEMU sandbox (cgroup hooks on machine scopes)
// ---------------------------------------------------------------------------

/// Default-deny traffic towards the VM (only policy / replies pass).
pub const VME_ISOLATE_IN: u32 = 1 << 0;
/// Default-deny traffic from the VM.
pub const VME_ISOLATE_OUT: u32 = 1 << 1;
/// The tap's tc ingress hook carries traffic *from* the VM.
pub const VME_GUEST_SIDE: u32 = 1 << 2;
/// The VM has deny rules towards it / from it (checked even when not isolated).
pub const VME_DENY_IN: u32 = 1 << 3;
pub const VME_DENY_OUT: u32 = 1 << 4;
/// Emit VM_FLOW_EVENTS for new flows and drops.
pub const VME_FLOW_LOG: u32 = 1 << 5;
/// Copy UDP DNS replies towards the VM to DNS_EVENTS (`toFQDNs` learning).
pub const VME_FQDN: u32 = 1 << 6;
/// The VM is the subject of L7 or authentication rules.
pub const VME_L7_AUTH: u32 = 1 << 7;
/// Drop packets from the VM whose source address belongs to another identity.
pub const VME_SRC_GUARD: u32 = 1 << 8;

/// VM_POLICY value bits. Any DENY hit wins; AUTH bits are OR-ed across the
/// matching entries; L7 applies only when every matching entry carries it.
pub const VM_POLICY_ALLOW: u32 = 1;
pub const VM_POLICY_DENY: u32 = 2;
/// Requests are parsed and checked by bpfd (VM_L7_EVENTS).
pub const VM_POLICY_L7: u32 = 4;
/// New flows need a live VM_AUTH entry (`authentication.mode: required`).
pub const VM_POLICY_AUTH: u32 = 8;
/// `authentication.mode: test-always-fail`.
pub const VM_POLICY_AUTH_FAIL: u32 = 16;
/// With L7: new TCP flows from the VM go through bpfd's terminating proxy
/// (TLS interception, header rewriting) while enforcement is live.
pub const VM_POLICY_PROXY: u32 = 32;

pub const VMF_FORWARDED: u8 = 0;
pub const VMF_DROPPED: u8 = 1;
/// Would have been dropped; let through (observe / no lease).
pub const VMF_AUDIT: u8 = 2;

pub const VMF_REASON_NONE: u8 = 0;
pub const VMF_REASON_POLICY_DENY: u8 = 1;
pub const VMF_REASON_DEFAULT_DENY: u8 = 2;
pub const VMF_REASON_AUTH_REQUIRED: u8 = 3;
pub const VMF_REASON_SPOOFED: u8 = 4;
pub const VMF_REASON_QUARANTINE: u8 = 5;

/// One VM edge verdict (VM_FLOW_EVENTS): first packet of a flow, or a
/// drop / audit (at most one per flow per second).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VmFlowEvent {
    pub ts_ns: u64,
    pub ifindex: u32,
    pub subject: u32,
    pub peer: u32,
    pub len: u32,
    pub src: [u8; ADDR_LEN],
    pub dst: [u8; ADDR_LEN],
    pub sport: u16,
    pub dport: u16,
    pub proto: u8,
    pub from_vm: u8,
    pub verdict: u8,
    pub reason: u8,
    pub tcp_flags: u8,
    /// ICMP type + 1 (0 = not ICMP).
    pub icmp: u8,
    /// 1 = required, 2 = test-always-fail (VMF_REASON_AUTH_REQUIRED).
    pub auth: u8,
    pub _pad: [u8; 5],
}

/// Frame bytes copied to VM_L7_EVENTS: a whole GSO packet fits, so bpfd can
/// reinject what it holds.
pub const VM_L7_CAP: usize = 65536;
/// Bytes of `VmL7Event` before `data`.
pub const VM_L7_HDR: usize = 96;

/// One segment past the flow's L7 window (VM_L7_EVENTS). `held` = the
/// segment was dropped; bpfd reinjects it once its bytes are allowed.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmL7Event {
    pub ts_ns: u64,
    pub ifindex: u32,
    pub subject: u32,
    pub peer: u32,
    /// TCP sequence / acknowledgement numbers of the segment (host order).
    pub seq: u32,
    pub ack: u32,
    /// L4 payload length.
    pub len: u32,
    /// Frame bytes in `data` (from the Ethernet header).
    pub cap: u32,
    /// Payload bytes inside the window already allowed.
    pub skip: u32,
    /// Length of the whole frame (`skb->len`).
    pub frame_len: u32,
    /// Offset of the L4 payload in `data`.
    pub payload_off: u16,
    /// `skb->gso_size` (0 = not a GSO packet).
    pub gso_size: u16,
    pub src: [u8; ADDR_LEN],
    pub dst: [u8; ADDR_LEN],
    pub sport: u16,
    pub dport: u16,
    pub proto: u8,
    pub from_vm: u8,
    pub held: u8,
    pub v6: u8,
    /// Offsets of the IP and L4 headers in `data`.
    pub l3_off: u16,
    pub l4_off: u16,
    /// Captured on the tc ingress hook (reinject into the receive path).
    pub ingress: u8,
    pub _pad: [u8; 3],
    pub data: [u8; VM_L7_CAP],
}

/// skb mark of a frame bpfd reinjects through the L7 inject veth:
/// magic | VM_L7_INJECT_INGRESS (into the tap's receive path, as if the VM
/// sent it; otherwise out of the tap towards the VM) | tap ifindex.
pub const VM_L7_INJECT_MAGIC: u32 = 0xb700_0000;
pub const VM_L7_INJECT_MAGIC_MASK: u32 = 0xff00_0000;
pub const VM_L7_INJECT_INGRESS: u32 = 1 << 23;
pub const VM_L7_INJECT_IFINDEX: u32 = (1 << 23) - 1;

pub const L7S_NONE: u8 = 0;
/// Unused (bpfd orders held segments itself).
pub const L7S_PENDING: u8 = 1;
/// Denied: data segments drop (bpfd already answered the client).
pub const L7S_DENIED: u8 = 2;
/// TCP: allowed for the rest of the connection (TLS, upgrades). UDP: one
/// reinjected datagram of `pass_until` bytes starting with `allow_seq`
/// passes once.
pub const L7S_OPEN: u8 = 3;

/// Per-edge L7 window (VM_L7_FLOW, key = conntrack FlowKey of the
/// originator with the tap ifindex): segments inside
/// `[allow_seq, pass_until)` pass.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VmL7Flow {
    pub allow_seq: u32,
    pub pass_until: u32,
    pub pending_seq: u32,
    pub state: u8,
    pub _pad: [u8; 3],
}

/// skb mark of a VM frame redirected to bpfd's transparent L7 proxy through
/// the inject veth (`mn_vm_l7_inject` assigns it to the proxy socket); also
/// the fwmark of the policy route that delivers it locally.
pub const VM_PROXY_MAGIC: u32 = 0xb600_0000;
/// Socket mark of the proxy's upstream connections: magic | slot, where
/// VM_PROXY_SRC[slot] is the client identity the server's edge sees. Slots
/// avoid bits 8-15 (Cilium's mark magic) so kube-proxy / Cilium rules on
/// the host never match them.
pub const VM_PROXY_UP_MAGIC: u32 = 0xb500_0000;
pub const VM_PROXY_SLOT: u32 = 0x00ff_00ff;

/// VM_PROXY_CFG[0]: where proxied frames go and the proxy listener port
/// (127.0.0.1 / ::1). `redirect_ifindex` 0 = no proxy.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VmProxyCfg {
    pub redirect_ifindex: u32,
    pub port: u16,
    pub _pad: u16,
}

/// VM_PROXY_FLOW value (key: the client's FlowKey, ifindex 0): the
/// identities the edge resolved when it redirected the flow.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VmProxyFlow {
    pub subject: u32,
    pub peer: u32,
    pub ifindex: u32,
    pub _pad: u32,
}

/// VM_AUTH key; value = expiry (monotonic ns).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VmAuthKey {
    pub subject: u32,
    pub peer: u32,
    /// 1 = required.
    pub mode: u8,
    pub _pad: [u8; 3],
}

/// Per-tap VM edge config (VM_EDGE, key = tap ifindex). Rates are policed
/// with token buckets; 0 = unlimited.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VmEdgeCfg {
    pub identity: u32,
    pub flags: u32,
    /// Bytes/s from the VM.
    pub out_bps: u64,
    /// Bytes/s towards the VM.
    pub in_bps: u64,
    /// Packets/s, each direction.
    pub pps: u32,
    pub _pad: u32,
    /// Quarantined until this monotonic time: only `POLICY_QUARANTINE`
    /// allowlist entries pass, whatever the enforcement mode.
    pub quarantine_until_ns: u64,
}

/// Token bucket state (VM_BUCKETS, key = ifindex << 1 | from_vm).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VmBucket {
    pub bytes: u64,
    /// Milli-packets.
    pub pkts: u64,
    pub last_ns: u64,
}

/// Per-tap counters (VM_EDGE_STATS, per-CPU, key = ifindex).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VmEdgeStats {
    pub out_pkts: u64,
    pub out_bytes: u64,
    pub in_pkts: u64,
    pub in_bytes: u64,
    /// Policy misses dropped (enforce + lease).
    pub denied: u64,
    /// Policy misses let through (observe).
    pub observed: u64,
    /// Over the Mbps / PPS limit.
    pub rate_dropped: u64,
}

pub const DEVCG_DEV_BLOCK: u32 = 1;
pub const DEVCG_DEV_CHAR: u32 = 2;
pub const DEVCG_ACC_MKNOD: u32 = 1;
pub const DEVCG_ACC_READ: u32 = 2;
pub const DEVCG_ACC_WRITE: u32 = 4;
/// `QemuDevRule::minor` wildcard.
pub const DEV_MINOR_ANY: u32 = u32::MAX;
pub const QEMU_DEV_RULES: usize = 32;
/// QEMU sandbox mode bit in `QemuSandboxCfg::flags`: deny (needs the lease).
pub const SANDBOX_ENFORCE: u32 = 1 << 0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QemuDevRule {
    pub major: u32,
    pub minor: u32,
    pub dev_type: u32,
    pub access: u32,
}

/// Device allowlist shared by every sandboxed QEMU scope (QEMU_SANDBOX[0]).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QemuSandboxCfg {
    pub n: u32,
    pub flags: u32,
    pub rules: [QemuDevRule; QEMU_DEV_RULES],
}

impl Default for QemuSandboxCfg {
    fn default() -> Self {
        Self {
            n: 0,
            flags: 0,
            rules: [QemuDevRule::default(); QEMU_DEV_RULES],
        }
    }
}

/// Device access outside the allowlist (QEMU_DEV_HITS).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DevHitKey {
    pub cgroup: u64,
    pub major: u32,
    pub minor: u32,
    pub dev_type: u16,
    pub access: u16,
    pub _pad: u32,
}

/// QEMU-originated IP egress outside loopback / allowed ports (QEMU_NET_HITS).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NetHitKey {
    pub cgroup: u64,
    pub addr: [u8; ADDR_LEN],
    /// Network order.
    pub port: [u8; 2],
    pub proto: u8,
    pub _pad: [u8; 5],
}

// ---------------------------------------------------------------------------
// Network change audit (kprobe on rtnetlink_rcv_msg, observe only)
// ---------------------------------------------------------------------------

/// RTNL_CFG[0]. `type_mask` bit `n` records RTM type `16 + n`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RtnlCfg {
    pub enabled: u32,
    /// Record only requests from this network namespace (inode); 0 = all.
    pub netns: u32,
    pub type_mask: u64,
    /// Kernel layout from BTF: `sk_buff.sk`, `sock.__sk_common.skc_net.net`,
    /// `net.ns.inum`. All zero = namespace unknown.
    pub off_skb_sk: u32,
    pub off_sk_net: u32,
    pub off_net_inum: u32,
    pub _pad: u32,
}

/// Link, address, route, neighbour, rule, qdisc and tc filter new/del/set.
pub const RTNL_DEFAULT_MASK: u64 =
    rtnl_bits(&[16, 17, 19, 20, 21, 24, 25, 28, 29, 32, 33, 36, 37, 44, 45]);

pub const fn rtnl_bits(types: &[u16]) -> u64 {
    let mut m = 0u64;
    let mut i = 0;
    while i < types.len() {
        let t = types[i];
        if t >= 16 && t < 80 {
            m |= 1 << (t - 16);
        }
        i += 1;
    }
    m
}

/// One state-changing rtnetlink request, as seen in the requesting task.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RtnlEvent {
    pub ts_ns: u64,
    pub cgroup_id: u64,
    pub tgid: u32,
    pub pid: u32,
    pub uid: u32,
    pub nlmsg_type: u16,
    pub nlmsg_flags: u16,
    /// Interface the request names by index (0: by name or none).
    pub ifindex: u32,
    pub nlmsg_len: u32,
    /// Routes: rtm_family / rtm_dst_len.
    pub family: u8,
    pub dst_len: u8,
    pub _pad: u16,
    /// Requester's network namespace inode (0 = unknown).
    pub netns: u32,
    pub comm: [u8; COMM_LEN],
    /// Routes: RTA_DST (4 or 16 bytes), else zero.
    pub dst: [u8; ADDR_LEN],
    /// Link requests naming the device by IFLA_IFNAME.
    pub ifname: [u8; 16],
}

/// RTNL_STATS (per-CPU) index.
pub const RTNL_STAT_EVENTS: u32 = 0;
pub const RTNL_STAT_DROPPED: u32 = 1;

// ---------------------------------------------------------------------------
// Sampled plaintext L7 (cgroup_skb, observe only)
// ---------------------------------------------------------------------------

pub const L7S_COPY: usize = 128;
pub const L7S_REDIS: u8 = 1;
pub const L7S_POSTGRES: u8 = 2;
pub const L7S_MYSQL: u8 = 3;
pub const L7S_KAFKA: u8 = 4;
pub const L7S_HTTP2: u8 = 5;

/// L7S_CFG[0]. One sample per flow+direction per `flow_gap_ns`, and at most
/// `rate` samples/s host-wide.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct L7sCfg {
    pub enabled: u32,
    pub rate: u32,
    pub flow_gap_ns: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct L7sEvent {
    pub ts_ns: u64,
    pub cgroup_id: u64,
    pub v6: u8,
    pub ingress: u8,
    /// L7S_* protocol id from L7S_PORTS.
    pub proto: u8,
    /// The destination port is the service port (a request).
    pub to_server: u8,
    pub sport: u16,
    pub dport: u16,
    pub src: [u8; ADDR_LEN],
    pub dst: [u8; ADDR_LEN],
    pub cap_len: u16,
    pub _pad: u16,
    /// Payload bytes in the segment.
    pub len: u32,
    pub data: [u8; L7S_COPY],
}

/// L7S_STATS (per-CPU) slots.
pub const L7S_STAT_ELIGIBLE: u32 = 0;
pub const L7S_STAT_EMITTED: u32 = 1;
pub const L7S_STAT_RATE_LIMITED: u32 = 2;
pub const L7S_STAT_RINGBUF_FULL: u32 = 3;
pub const L7S_STAT_LOAD_FAIL: u32 = 4;
pub const L7S_STAT_SLOTS: u32 = 8;

// ---------------------------------------------------------------------------
// VM runtime intelligence (observe only, opt-in)
// ---------------------------------------------------------------------------

/// VmiCfg.features bits.
pub const VMI_F_FLIGHT: u32 = 1; // kvm exits, vCPU run-queue latency, migrations, residency
pub const VMI_F_IO: u32 = 2; // block request latency, vhost kicks/work
pub const VMI_F_MEM: u32 = 4; // page-fault + direct-reclaim latency, first KVM entry
pub const VMI_F_TOPO: u32 = 8; // per-CPU hardirq/softirq time

/// VMI_CFG[0]. Tracepoint field offsets come from tracefs formats.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VmiCfg {
    pub enabled: u32,
    pub features: u32,
    pub off_exit_reason: u32,
    pub off_wakeup_pid: u32,
    pub off_switch_prev_pid: u32,
    pub off_switch_prev_state: u32,
    pub off_switch_next_pid: u32,
    pub off_migrate_pid: u32,
    pub off_entry_vcpu: u32,
    pub off_bio_dev: u32,
    pub off_bio_sector: u32,
    pub off_rqc_dev: u32,
    pub off_rqc_sector: u32,
    pub _pad: u32,
}

/// VMI_TIDS value. `vcpu` is u32::MAX for non-vCPU threads.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VmiThread {
    pub vm: u32,
    pub vcpu: u32,
}

/// VMI_HIST key. `vm` 0 = host-wide (topology).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VmiKey {
    pub vm: u32,
    pub kind: u16,
    pub slot: u16,
}

/// In-flight block I/O (keyed by device and start sector): start time and
/// owning VM.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VmiBlk {
    pub ts: u64,
    pub vm: u32,
    pub _pad: u32,
}

/// VmiKey.kind. Histogram kinds use `slot` = log2(ns) bucket and count
/// events; RESIDENCY/IRQ/SOFTIRQ use `slot` = CPU and sum nanoseconds.
pub mod vmi_kind {
    pub const EXIT: u16 = 1; // slot = exit reason
    pub const RUNQ: u16 = 2;
    pub const BLK: u16 = 3;
    pub const VHOST_WORK: u16 = 4; // slot 0, count
    pub const VHOST_KICK: u16 = 5; // slot 0, count
    pub const FAULT: u16 = 6;
    pub const RECLAIM: u16 = 7;
    pub const MIGRATE: u16 = 8; // slot 0, count
    pub const RESIDENCY: u16 = 9;
    pub const IRQ: u16 = 10;
    pub const SOFTIRQ: u16 = 11;
}

/// VMI_TS key namespaces (`ns << 32 | tid`).
pub const VMI_TS_RUNQ: u64 = 1;
pub const VMI_TS_RUN: u64 = 2;
pub const VMI_TS_FAULT: u64 = 3;
pub const VMI_TS_RECLAIM: u64 = 4;

/// log2 bucket of a nanosecond duration (bucket b covers [2^b, 2^(b+1))).
#[inline(always)]
pub fn log2_slot(ns: u64) -> u16 {
    if ns == 0 {
        0
    } else {
        (63 - ns.leading_zeros()) as u16
    }
}

// ---------------------------------------------------------------------------
// VMM guard (BPF-LSM; audit by default, enforce only under a lease)
// ---------------------------------------------------------------------------

/// GUARD_POLICIES value bits (per QEMU cgroup).
pub const GUARD_EXEC: u32 = 1; // exec only allowlisted binaries
pub const GUARD_WX: u32 = 2; // no writable+executable mappings
pub const GUARD_DEV: u32 = 4; // open only allowlisted char devices

/// GUARD_CFG[0]. Offsets come from kernel BTF.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuardCfg {
    pub enabled: u32,
    pub enforce: u32,
    /// Monotonic ns; enforcement stops by itself past this.
    pub lease_deadline_ns: u64,
    pub off_bprm_file: u32,
    pub off_file_inode: u32,
    pub off_inode_ino: u32,
    pub off_inode_sb: u32,
    pub off_sb_dev: u32,
    pub off_inode_mode: u32,
    pub off_inode_rdev: u32,
    pub off_vma_flags: u32,
}

/// GUARD_FILES key: an executable by (kernel dev_t, inode).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct GuardFileKey {
    pub ino: u64,
    pub dev: u32,
    pub _pad: u32,
}

pub const GUARD_HOOK_EXEC: u8 = 1;
pub const GUARD_HOOK_MPROTECT: u8 = 2;
pub const GUARD_HOOK_OPEN: u8 = 3;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GuardEvent {
    pub ts_ns: u64,
    pub cgroup_id: u64,
    pub tgid: u32,
    pub pid: u32,
    pub hook: u8,
    /// 1 = denied, 0 = audited only.
    pub denied: u8,
    pub _pad: u16,
    /// exec: dev; mprotect: prot; open: major.
    pub a: u32,
    /// exec: inode; mprotect: vm_flags; open: minor.
    pub b: u64,
    pub comm: [u8; 16],
}

pub const GUARD_STAT_AUDITED: u32 = 0;
pub const GUARD_STAT_DENIED: u32 = 1;
pub const GUARD_STAT_DROPPED: u32 = 2;

// ---------------------------------------------------------------------------
// Bridge-less direct redirect (outer device <-> VM tap; under the lease)
// ---------------------------------------------------------------------------

/// DIRECT_CFG[0]. Redirects only while `enabled` and before the deadline.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirectCfg {
    pub enabled: u32,
    pub _pad: u32,
    pub lease_deadline_ns: u64,
}

pub const DIRECT_STAT_IN: u32 = 0;
pub const DIRECT_STAT_OUT: u32 = 1;
pub const DIRECT_STAT_IDLE: u32 = 2;

/// DIRECT_MAC key: a MAC address in the low 48 bits.
#[inline(always)]
pub fn mac_key(m: &[u8; 6]) -> u64 {
    (m[0] as u64) << 40
        | (m[1] as u64) << 32
        | (m[2] as u64) << 24
        | (m[3] as u64) << 16
        | (m[4] as u64) << 8
        | m[5] as u64
}

// ---------------------------------------------------------------------------
// Helpers usable from both sides
// ---------------------------------------------------------------------------

/// IPv4 → IPv4-mapped IPv6 bytes.
#[inline(always)]
pub const fn v4_mapped(o: [u8; 4]) -> [u8; ADDR_LEN] {
    [
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, o[0], o[1], o[2], o[3],
    ]
}

/// FNV-1a 64 over a NUL-terminated byte path (shared by the exec deny map).
#[inline(always)]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0 {
            break;
        }
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

#[cfg(all(feature = "user", target_os = "linux"))]
mod pod {
    use super::*;
    macro_rules! pod {
        ($($t:ty),*) => { $(unsafe impl aya::Pod for $t {})* };
    }
    pod!(
        GlobalCfg,
        IfaceCfg,
        DenyKey,
        AllowKey,
        RuleVal,
        FlowKey,
        FlowVal,
        NetEvent,
        FileWatch,
        PortKey,
        CapKey,
        HealthKey,
        QosState,
        Endpoint,
        PolicyKey,
        SvcKey,
        SvcVal,
        BackendKey,
        Backend,
        RevNatKey,
        NatCtKey,
        NatCtVal,
        NodeCfg,
        RateCfg,
        IfaceStats,
        MaglevKey,
        AffinityKey,
        AffinityVal,
        XdpCfg,
        VmEdgeCfg,
        VmBucket,
        VmEdgeStats,
        VmFlowEvent,
        VmL7Event,
        VmL7Flow,
        VmProxyCfg,
        VmProxyFlow,
        VmAuthKey,
        QemuDevRule,
        QemuSandboxCfg,
        DevHitKey,
        NetHitKey,
        ShieldCfg,
        ShieldSrcKey,
        ShieldSrcState,
        ShieldStats,
        ConnKey,
        ConnStats,
        TcpPressure,
        IcmpErrKey,
        SampleCfg,
        SampleBucket,
        SslReadArgs,
        NodeIsoCfg,
        NodeIsoStats,
        RtnlCfg,
        L7sCfg,
        VmiCfg,
        VmiThread,
        VmiKey,
        VmiBlk,
        GuardCfg,
        GuardFileKey,
        DirectCfg,
        QlbSvcKey,
        QlbSvc,
        QlbBeKey,
        QlbBackend,
        QlbSidKey
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn layouts_have_no_implicit_padding() {
        assert_eq!(size_of::<GlobalCfg>(), 16);
        assert_eq!(size_of::<IfaceCfg>(), 32);
        assert_eq!(size_of::<DenyKey>(), 20);
        assert_eq!(size_of::<AllowKey>(), 24);
        assert_eq!(size_of::<FlowKey>(), 48);
        assert_eq!(size_of::<FlowVal>(), 56);
        assert_eq!(size_of::<NetEvent>(), 88);
        assert_eq!(size_of::<ProcEvent>(), 64 + 32 + 16 + PATH_LEN);
        assert_eq!(size_of::<Endpoint>(), 32);
        assert_eq!(size_of::<PolicyKey>(), 12);
        assert_eq!(size_of::<NatCtKey>(), 40);
        assert_eq!(size_of::<NatCtVal>(), 48);
        assert_eq!(size_of::<SvcKey>(), 20);
        assert_eq!(size_of::<SvcVal>(), 16);
        assert_eq!(size_of::<Backend>(), 36);
        assert_eq!(size_of::<RevNatKey>(), 32);
        assert_eq!(size_of::<AffinityKey>(), 20);
        assert_eq!(size_of::<AffinityVal>(), 16);
        assert_eq!(size_of::<NodeCfg>(), 40);
        assert_eq!(size_of::<VmEdgeCfg>(), 40);
        assert_eq!(size_of::<VmBucket>(), 24);
        assert_eq!(size_of::<VmEdgeStats>(), 56);
        assert_eq!(size_of::<VmFlowEvent>(), 72);
        assert_eq!(size_of::<VmL7Event>(), VM_L7_HDR + VM_L7_CAP);
        assert_eq!(size_of::<VmL7Flow>(), 16);
        assert_eq!(size_of::<VmProxyCfg>(), 8);
        assert_eq!(size_of::<VmProxyFlow>(), 16);
        assert_eq!(size_of::<VmAuthKey>(), 12);
        assert_eq!(size_of::<QemuSandboxCfg>(), 8 + 16 * QEMU_DEV_RULES);
        assert_eq!(size_of::<DevHitKey>(), 24);
        assert_eq!(size_of::<NetHitKey>(), 32);
        assert_eq!(size_of::<TlsFpEvent>(), 64 + TLSFP_LEN);
        assert_eq!(size_of::<SslEvent>(), 48 + SSL_DATA_LEN);
        assert_eq!(size_of::<SampleCfg>(), 16);
        assert_eq!(size_of::<ConnKey>(), 24);
        assert_eq!(size_of::<ConnStats>(), 32 + 8 * CONNECT_BUCKETS);
        assert_eq!(size_of::<TcpPressure>(), 40);
        assert_eq!(size_of::<IcmpErrKey>(), 8);
        assert_eq!(size_of::<ShieldCfg>(), 32);
        assert_eq!(size_of::<NodeIsoCfg>(), 24);
        assert_eq!(size_of::<NodeIsoStats>(), 40);
        assert_eq!(size_of::<ShieldSrcKey>(), 20);
        assert_eq!(size_of::<ShieldSrcState>(), 24);
        assert_eq!(size_of::<ShieldStats>(), 56 + 8 * SHIELD_CLASSES);
        assert_eq!(size_of::<RateCfg>(), 16);
        assert_eq!(size_of::<IfaceStats>(), 40);
        assert_eq!(size_of::<L7Event>(), 24 + 48 + L7_PAYLOAD_LEN);
        assert_eq!(size_of::<RtnlCfg>(), 32);
        assert_eq!(size_of::<RtnlEvent>(), 96);
        assert_eq!(size_of::<L7sCfg>(), 16);
        assert_eq!(size_of::<L7sEvent>(), 64 + L7S_COPY);
        assert_eq!(size_of::<VmiCfg>(), 56);
        assert_eq!(size_of::<VmiKey>(), 8);
        assert_eq!(size_of::<VmiBlk>(), 16);
        assert_eq!(size_of::<GuardCfg>(), 48);
        assert_eq!(size_of::<GuardFileKey>(), 16);
        assert_eq!(size_of::<GuardEvent>(), 56);
        assert_eq!(size_of::<DirectCfg>(), 16);
        assert_eq!(size_of::<QlbSvcKey>(), 20);
        assert_eq!(size_of::<QlbSvc>(), 24);
        assert_eq!(size_of::<QlbBackend>(), 24);
        assert_eq!(size_of::<QlbSidKey>(), 8);
        assert_eq!(qlb_cid_sid(&[0x1f, 0x12, 0x34], 0), Some(0x1234));
        assert_eq!(qlb_cid_sid(&[0x3f, 0x12, 0x34], 1), Some(0x1234));
        assert_eq!(qlb_cid_sid(&[0xe0, 0x12, 0x34], 0), None);
        assert_eq!(
            mac_key(&[0x52, 0x54, 0, 0xaa, 0xbb, 0xcc]),
            0x5254_00aa_bbcc
        );
        assert_eq!(
            (log2_slot(0), log2_slot(1), log2_slot(1023), log2_slot(1024)),
            (0, 0, 9, 10)
        );
    }

    #[test]
    fn rtnl_mask() {
        assert_eq!(rtnl_bits(&[16, 17]), 0b11);
        assert_eq!(
            RTNL_DEFAULT_MASK & rtnl_bits(&[18, 22, 26, 30]),
            0,
            "GET types stay out"
        );
    }

    #[test]
    fn fnv_stops_at_nul() {
        assert_eq!(fnv1a64(b"/bin/sh\0garbage"), fnv1a64(b"/bin/sh"));
        assert_ne!(fnv1a64(b"/bin/sh"), fnv1a64(b"/bin/bash"));
    }

    #[test]
    fn v4_mapping() {
        assert_eq!(&v4_mapped([10, 0, 0, 1])[10..], &[0xff, 0xff, 10, 0, 0, 1]);
    }
}
