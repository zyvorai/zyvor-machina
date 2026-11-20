// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use aya_ebpf::{
    macros::map,
    maps::{Array, HashMap, LpmTrie, LruHashMap, PerCpuHashMap, RingBuf},
};
use machina_bpf_common::*;

const BPF_F_NO_PREALLOC: u32 = 1;

// ---- shared config ---------------------------------------------------------

#[map]
pub static CONFIG: Array<GlobalCfg> = Array::with_max_entries(1, 0);

#[map]
pub static IFACE_CFG: HashMap<u32, IfaceCfg> = HashMap::with_max_entries(4096, 0);

/// cgroup id → policy scope for container (cgroup) enforcement.
#[map]
pub static CGROUP_SCOPE: HashMap<u64, u32> = HashMap::with_max_entries(4096, 0);

/// scope → IF_* flags that apply to cgroup-scoped sockets (IF_DENY / IF_ALLOW).
#[map]
pub static SCOPE_FLAGS: HashMap<u32, u32> = HashMap::with_max_entries(4096, 0);

// ---- enforcement -----------------------------------------------------------

#[map]
pub static DENY_LPM: LpmTrie<DenyKey, RuleVal> = LpmTrie::with_max_entries(65536, 0);

#[map]
pub static ALLOW_LPM: LpmTrie<AllowKey, RuleVal> = LpmTrie::with_max_entries(65536, 0);

#[map]
pub static DENY_PORTS: HashMap<PortKey, RuleVal> = HashMap::with_max_entries(4096, 0);

#[map]
pub static CAP_DENY: HashMap<CapKey, u32> = HashMap::with_max_entries(1024, 0);

/// FNV-1a(path) → policy id for exec deny.
#[map]
pub static EXEC_DENY: HashMap<u64, u32> = HashMap::with_max_entries(4096, 0);

#[map]
pub static FILE_WATCH: Array<FileWatch> = Array::with_max_entries(FILE_WATCH_SLOTS, 0);

// ---- flows / events --------------------------------------------------------

#[map]
pub static FLOWS: LruHashMap<FlowKey, FlowVal> = LruHashMap::with_max_entries(131072, 0);

#[map]
pub static NET_EVENTS: RingBuf = RingBuf::with_byte_size(1 << 22, 0);

#[map]
pub static DNS_EVENTS: RingBuf = RingBuf::with_byte_size(1 << 21, 0);

#[map]
pub static CAPTURE_EVENTS: RingBuf = RingBuf::with_byte_size(1 << 23, 0);

#[map]
pub static PROC_EVENTS: RingBuf = RingBuf::with_byte_size(1 << 23, 0);

#[map]
pub static QOS_STATE: HashMap<u32, QosState> = HashMap::with_max_entries(4096, 0);

/// scope → new-connection rate limit (`rate_limit` policies).
#[map]
pub static RATE_CFG: HashMap<u32, RateCfg> = HashMap::with_max_entries(4096, 0);

/// ifindex → connection token bucket (tokens in milli-connections).
#[map]
pub static CONN_RATE: HashMap<u32, QosState> = HashMap::with_max_entries(4096, 0);

/// ifindex → traffic accounting (every packet on a programmed interface).
#[map]
pub static IFACE_STATS: PerCpuHashMap<u32, IfaceStats> = PerCpuHashMap::with_max_entries(4096, 0);

#[map]
pub static L7_EVENTS: RingBuf = RingBuf::with_byte_size(1 << 22, 0);

// ---- tracepoint layout + health -------------------------------------------

#[map]
pub static TP_OFF: Array<u32> = Array::with_max_entries(tp::COUNT, 0);

#[map]
pub static DROP_REASONS: HashMap<u32, u64> = HashMap::with_max_entries(1024, 0);

#[map]
pub static TCP_HEALTH: LruHashMap<HealthKey, u64> = LruHashMap::with_max_entries(16384, 0);

// ---- CNI -------------------------------------------------------------------

#[map]
pub static CNI_NODE: Array<NodeCfg> = Array::with_max_entries(1, 0);

/// Local pod endpoints keyed by 16-byte address (IPv4-mapped for v4).
#[map]
pub static CNI_ENDPOINTS: HashMap<[u8; ADDR_LEN], Endpoint> = HashMap::with_max_entries(8192, 0);

/// Cluster-wide pod IP → identity.
#[map]
pub static CNI_IDENTITIES: HashMap<[u8; ADDR_LEN], u32> = HashMap::with_max_entries(131072, 0);

/// NetworkPolicy ipBlock CIDRs → identity (fallback when the peer is not a
/// pod). IPv4 prefixes are stored IPv4-mapped (prefix + 96).
#[map]
pub static CNI_CIDR_IDS: LpmTrie<[u8; ADDR_LEN], u32> = LpmTrie::with_max_entries(16384, 0);

#[map]
pub static CNI_POLICY: HashMap<PolicyKey, u32> = HashMap::with_max_entries(65536, 0);

/// Stateful policy conntrack: FlowKey (ifindex 0, local = originator).
#[map]
pub static CNI_CT: LruHashMap<FlowKey, u64> = LruHashMap::with_max_entries(262144, 0);

/// NodePort flow pinning: client-side tuple → chosen backend.
#[map]
pub static CNI_NODEPORT_FWD: LruHashMap<NatCtKey, Backend> =
    LruHashMap::with_max_entries(65536, 0);

/// Maglev tables: (svc, slot) → backend index. Only services with two or
/// more backends have a table; not preallocated.
#[map]
pub static CNI_MAGLEV: HashMap<MaglevKey, u32> = HashMap::with_max_entries(1 << 20, BPF_F_NO_PREALLOC);

/// ClientIP session affinity: (client, svc) → backend index.
#[map]
pub static CNI_AFFINITY: LruHashMap<AffinityKey, AffinityVal> = LruHashMap::with_max_entries(65536, 0);

#[map]
pub static CNI_SERVICES: HashMap<SvcKey, SvcVal> = HashMap::with_max_entries(16384, 0);

#[map]
pub static CNI_BACKENDS: HashMap<BackendKey, Backend> = HashMap::with_max_entries(65536, 0);

/// NodePort frontends (key addr = 0.0.0.0, port = nodePort).
#[map]
pub static CNI_NODEPORTS: HashMap<SvcKey, SvcVal> = HashMap::with_max_entries(4096, 0);

#[map]
pub static CNI_UDP_REVNAT: LruHashMap<RevNatKey, Backend> =
    LruHashMap::with_max_entries(65536, 0);

#[map]
pub static CNI_NODEPORT_CT: LruHashMap<NatCtKey, NatCtVal> =
    LruHashMap::with_max_entries(65536, 0);
