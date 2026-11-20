// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! VM edge (`mn_vm_edge_in/out` on VM taps) and the QEMU sandbox
//! (`mn_qemu_device` + `mn_qemu_egress` on machine-qemu scopes).
//!
//! Both follow the VMs: the periodic rescan programs taps of VMs in the
//! synced edge state as they appear, and (with `auto`) sandboxes every
//! running QEMU scope. Policy misses and sandbox violations only drop with
//! the enforcement lease live; otherwise they are counted.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::net::IpAddr;
use std::os::unix::fs::MetadataExt;

use super::cni::addr16;
use super::*;

pub(super) mod learned;
mod quarantine;
mod threat;

pub(super) use threat::{threat_observe, ThreatHit};

const EDGE_IN: &str = "mn_vm_edge_in";
const EDGE_OUT: &str = "mn_vm_edge_out";
/// Global unicast addresses of this node (management, bridges, uplinks)
/// as `address/prefix`.
pub(super) fn node_cidrs() -> Vec<String> {
    let Ok(out) = std::process::Command::new("ip")
        .args(["-j", "addr", "show"])
        .output()
    else {
        return Vec::new();
    };
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    v.as_array()
        .into_iter()
        .flatten()
        .filter(|l| l["ifname"].as_str() != Some("lo"))
        .flat_map(|l| l["addr_info"].as_array().cloned().unwrap_or_default())
        .filter(|a| a["scope"].as_str() == Some("global"))
        .filter_map(|a| {
            let ip = a["local"].as_str()?;
            let len = a["prefixlen"].as_u64()?;
            Some(format!("{ip}/{len}"))
        })
        .collect()
}

fn node_addresses() -> Vec<[u8; ADDR_LEN]> {
    node_cidrs()
        .iter()
        .filter_map(|c| addr16(c.split('/').next().unwrap_or("")).ok())
        .collect()
}

/// Veth pair held L7 frames are reinjected through: bpfd sends on the
/// first, `mn_vm_l7_inject` on the second's ingress redirects into the tap.
const L7_INJECT: (&str, &str) = ("mnl7inj0", "mnl7inj1");
const L7_INJECT_PROG: &str = "mn_vm_l7_inject";
const MACHINE_SLICE: &str = "machine.slice";
const VM_POLICY_CAP: usize = 131_072;

/// Learned names live at least this long (Cilium's `tofqdns-min-ttl` idea:
/// clients cache answers longer than the TTL says).
const FQDN_MIN_TTL: Duration = Duration::from_secs(600);
const FQDN_MAX_TTL: Duration = Duration::from_secs(86_400);
const FQDN_CACHE_CAP: usize = 8192;

/// One DNS answer address with the names that led to it (query + CNAMEs).
pub(super) struct FqdnLearn {
    names: Vec<String>,
    addr: [u8; ADDR_LEN],
    ttl: u32,
    vm: String,
    ifindex: u32,
}

/// Policy value for one compiled rule.
fn rule_val(deny: bool, auth: u8, l7: bool) -> u32 {
    if deny {
        return VM_POLICY_DENY;
    }
    let mut v = VM_POLICY_ALLOW;
    if l7 {
        v |= VM_POLICY_L7;
    }
    if auth & crate::api::AUTH_REQUIRED != 0 {
        v |= VM_POLICY_AUTH;
    }
    if auth & crate::api::AUTH_ALWAYS_FAIL != 0 {
        v |= VM_POLICY_AUTH_FAIL;
    }
    v
}

/// Two rules on one key: deny wins, auth accumulates, L7 only if both.
fn merge_val(a: u32, b: u32) -> u32 {
    const BOTH: u32 = VM_POLICY_L7 | VM_POLICY_PROXY;
    if (a | b) & VM_POLICY_DENY != 0 {
        VM_POLICY_DENY
    } else {
        ((a | b) & !BOTH) | (a & b & BOTH)
    }
}

/// Routing table / rule priority delivering VM_PROXY_MAGIC frames locally.
const PROXY_TABLE: &str = "4251";
const PROXY_RULE_PREF: &str = "9000";

fn ip(args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("ip").args(args).output()?;
    if !out.status.success() {
        return Err(anyhow!(
            "ip {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// fwmark VM_PROXY_MAGIC → local delivery, and no reverse-path check on the
/// inject veth (proxied frames carry the VM's source address). IPv6 is
/// best effort.
fn ensure_proxy_route(rx: &str) -> Result<()> {
    let mark = format!("{VM_PROXY_MAGIC:#x}");
    for (fam, any) in [("-4", "0.0.0.0/0"), ("-6", "::/0")] {
        let r = (|| {
            if !ip(&[fam, "rule", "show"])?.contains(&format!("lookup {PROXY_TABLE}")) {
                ip(&[
                    fam,
                    "rule",
                    "add",
                    "fwmark",
                    &mark,
                    "lookup",
                    PROXY_TABLE,
                    "pref",
                    PROXY_RULE_PREF,
                ])?;
            }
            ip(&[
                fam,
                "route",
                "replace",
                "local",
                any,
                "dev",
                "lo",
                "table",
                PROXY_TABLE,
            ])
        })();
        match (fam, r) {
            (_, Ok(_)) => {}
            ("-4", Err(e)) => return Err(e),
            (_, Err(e)) => tracing::info!("vm proxy: IPv6 route: {e:#}"),
        }
    }
    std::fs::write(format!("/proc/sys/net/ipv4/conf/{rx}/rp_filter"), "0")
        .context("rp_filter on the inject veth")?;
    Ok(())
}

const AUTH_TTL: Duration = Duration::from_secs(3600);

struct AuthState {
    mode: u8,
    ok: bool,
    note: String,
    expires: Instant,
}

struct FqdnBinding {
    names: BTreeMap<String, Instant>,
    vm: String,
}

#[derive(Default)]
pub(super) struct VmEdgeRuntime {
    pub state: VmEdgeState,
    proxy_cfg: VmProxyCfg,
    proxy_note: String,
    groups: BTreeMap<String, u32>,
    /// tap name → (ifindex, vm)
    taps: HashMap<String, (u32, String)>,
    ips: HashMap<[u8; ADDR_LEN], u32>,
    policy: HashMap<PolicyKey, u32>,
    /// Identities with deny entries: (identity, egress).
    deny: HashSet<(u32, bool)>,
    /// From the last sync, before learned FQDN addresses are merged in.
    base_ips: HashMap<[u8; ADDR_LEN], u32>,
    base_policy: HashMap<PolicyKey, u32>,
    base_index: FlowIndex,
    cidrs: Vec<(Prefix, u32)>,
    fqdn_cache: HashMap<[u8; ADDR_LEN], FqdnBinding>,
    /// Learned address → identity in force.
    fqdn_ids: HashMap<[u8; ADDR_LEN], u32>,
    /// Identities that are subjects of L7 or authentication rules.
    l7auth: HashSet<u32>,
    auth_any: bool,
    auth: HashMap<(u32, u32), AuthState>,
    l7_rules: usize,
    quarantines: BTreeMap<String, quarantine::Held>,
    threat_feeds: BTreeMap<String, threat::Feed>,
    threat_bound: HashMap<[u8; ADDR_LEN], threat::Bound>,
    /// Libvirt VMs outside the edge state whose taps the feeds programmed.
    threat_vms: BTreeSet<String>,
    threat_path: Option<std::path::PathBuf>,
}

/// Queue the A/AAAA answers of a DNS reply whose names match a `toFQDNs`
/// pattern; true when something was queued.
pub(super) fn fqdn_learn(
    s: &mut Shared,
    msg: &crate::dns::DnsMessage,
    vm: Option<&str>,
    ifindex: u32,
) -> bool {
    if s.vm_fqdn_patterns.is_empty() {
        return false;
    }
    let mut names: Vec<String> = vec![crate::netpol::fqdn::normalize(&msg.qname)];
    for a in &msg.answers {
        let n = crate::netpol::fqdn::normalize(&a.name);
        if !names.contains(&n) {
            names.push(n);
        }
    }
    if !names.iter().any(|n| {
        s.vm_fqdn_patterns
            .iter()
            .any(|p| crate::netpol::fqdn::matches(p, n))
    }) {
        return false;
    }
    let mut queued = false;
    for a in msg
        .answers
        .iter()
        .filter(|a| a.rtype == "A" || a.rtype == "AAAA")
    {
        if let Ok(ip) = a.data.parse::<IpAddr>() {
            s.vm_fqdn_queue.push(FqdnLearn {
                names: names.clone(),
                addr: policy::ip_to_addr(ip),
                ttl: a.ttl,
                vm: vm.unwrap_or_default().to_string(),
                ifindex,
            });
            queued = true;
        }
    }
    queued
}

fn prefix_has(p: &Prefix, addr: &[u8; ADDR_LEN]) -> bool {
    (0..ADDR_LEN).all(|i| {
        let start = i as u32 * 8;
        if start >= p.bits {
            true
        } else if start + 8 > p.bits {
            let m = 0xffu8 << (8 - (p.bits - start));
            addr[i] & m == p.addr[i] & m
        } else {
            addr[i] == p.addr[i]
        }
    })
}

/// What the flow reader needs to label events: identity → name/labels and
/// the rule table for attribution.
#[derive(Default, Clone)]
pub(super) struct FlowIndex {
    names: HashMap<u32, (String, BTreeMap<String, String>)>,
    rules: crate::netpol::RuleIndex,
}

impl FlowIndex {
    pub(super) fn name(&self, id: u32) -> Option<&(String, BTreeMap<String, String>)> {
        self.names.get(&id)
    }

    /// Same lookup order as the kernel; any deny match wins.
    fn attribute(
        &self,
        subject: u32,
        peer: u32,
        egress: bool,
        proto: u8,
        port: u16,
    ) -> Option<String> {
        let combos = [
            (peer, proto, port),
            (peer, proto, 0),
            (0, proto, port),
            (0, proto, 0),
            (peer, 0, 0),
            (0, 0, 0),
        ];
        let hits: Vec<&(bool, String)> = combos
            .iter()
            .filter_map(|(pe, pr, po)| self.rules.get(&(subject, *pe, egress, *pr, *po)))
            .collect();
        hits.iter()
            .find(|h| h.0)
            .or_else(|| hits.first())
            .map(|h| h.1.clone())
            .filter(|s| !s.is_empty())
    }
}

fn tcp_flag_names(f: u8) -> String {
    let mut out = Vec::new();
    for (bit, n) in [
        (0x02, "SYN"),
        (0x10, "ACK"),
        (0x01, "FIN"),
        (0x04, "RST"),
        (0x08, "PSH"),
        (0x20, "URG"),
    ] {
        if f & bit != 0 {
            out.push(n);
        }
    }
    out.join(",")
}

pub(super) fn on_vm_flow(
    sh: &SharedState,
    bus: &broadcast::Sender<StreamEvent>,
    wake: &tokio::sync::Notify,
    b: &[u8],
) {
    if b.len() < std::mem::size_of::<VmFlowEvent>() {
        return;
    }
    let ev = unsafe { std::ptr::read_unaligned(b.as_ptr() as *const VmFlowEvent) };
    let (rec, alerts) = {
        let mut s = lock(sh);
        let (iface, tap_vm) = s.iface(ev.ifindex);
        let egress = ev.from_vm != 0;
        let idx = &s.vm_flow_index;
        let subject = idx.name(ev.subject).cloned();
        let peer = idx.name(ev.peer).cloned();
        let port = if ev.icmp != 0 {
            ev.icmp as u16
        } else {
            ev.dport
        };
        let policy = idx.attribute(ev.subject, ev.peer, egress, ev.proto, port);
        let vm = tap_vm
            .or_else(|| subject.as_ref().map(|s| s.0.clone()))
            .unwrap_or_default();
        if let (true, false, Ok(ip)) = (egress, vm.is_empty(), fmt_addr(&ev.src).parse()) {
            s.vm_learned.note(&vm, ip, Instant::now());
        }
        let (src_side, dst_side) = if egress {
            (subject, peer)
        } else {
            (peer, subject)
        };
        let (src_id, dst_id) = if egress {
            (ev.subject, ev.peer)
        } else {
            (ev.peer, ev.subject)
        };
        let rec = VmFlowRecord {
            ts: mono_to_rfc3339(ev.ts_ns),
            host: None,
            iface: iface.unwrap_or_default(),
            vm,
            direction: if egress { "egress" } else { "ingress" }.into(),
            src: fmt_addr(&ev.src),
            src_port: if ev.icmp != 0 { 0 } else { ev.sport },
            dst: fmt_addr(&ev.dst),
            dst_port: if ev.icmp != 0 { 0 } else { ev.dport },
            src_vm: src_side.as_ref().map(|s| s.0.clone()),
            dst_vm: dst_side.as_ref().map(|s| s.0.clone()),
            src_labels: src_side.map(|s| s.1).unwrap_or_default(),
            dst_labels: dst_side.map(|s| s.1).unwrap_or_default(),
            src_identity: src_id,
            dst_identity: dst_id,
            proto: match ev.proto {
                132 => "sctp".into(),
                p => proto_name(p).into(),
            },
            tcp_flags: if ev.proto == policy::IPPROTO_TCP {
                tcp_flag_names(ev.tcp_flags)
            } else {
                String::new()
            },
            icmp_type: (ev.icmp != 0).then(|| ev.icmp - 1),
            bytes: ev.len,
            verdict: match ev.verdict {
                VMF_DROPPED => "DROPPED",
                VMF_AUDIT => "AUDIT",
                _ => "FORWARDED",
            }
            .into(),
            drop_reason: match ev.reason {
                VMF_REASON_POLICY_DENY if ev.peer == crate::netpol::IDENTITY_THREAT => {
                    Some("threat-domain".into())
                }
                VMF_REASON_POLICY_DENY => Some("policy-deny".into()),
                VMF_REASON_DEFAULT_DENY => Some("default-deny".into()),
                VMF_REASON_AUTH_REQUIRED if ev.auth == 2 => Some("auth-test-always-fail".into()),
                VMF_REASON_AUTH_REQUIRED => Some("auth-required".into()),
                VMF_REASON_SPOOFED => Some("spoofed-source".into()),
                VMF_REASON_QUARANTINE => Some("quarantine".into()),
                _ => None,
            },
            policy,
            ..Default::default()
        };
        if ev.reason == VMF_REASON_AUTH_REQUIRED
            && ev.auth == 1
            && !s.vm_auth_queue.contains(&(ev.subject, ev.peer))
        {
            s.vm_auth_queue.push((ev.subject, ev.peer));
            wake.notify_one();
        }
        let alerts = s.record_vm_flow(&rec);
        (rec, alerts)
    };
    publish_vm_flow(bus, &rec, alerts);
}

#[derive(Default)]
pub(super) struct SandboxRuntime {
    pub config: VmSandboxConfig,
    /// VM → cgroup path relative to the cgroup root.
    attached: BTreeMap<String, String>,
    /// VMs attached by request (kept across restarts, re-attached on start).
    pub pinned: HashSet<String>,
    devices: Vec<String>,
    ports: HashSet<u16>,
    notes: Vec<String>,
    configured: bool,
}

/// Group (or per-VM) identity; never one of the reserved CNI identities.
fn group_identity(name: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    if h < 16 {
        h + 16
    } else {
        h
    }
}

fn vm_group(vm: &VmEdgeVm) -> String {
    vm.group
        .clone()
        .unwrap_or_else(|| format!("vm:{}", vm.name))
}

fn vm_ident(vm: &VmEdgeVm) -> u32 {
    vm.identity.unwrap_or_else(|| group_identity(&vm_group(vm)))
}

fn mbps_to_bytes(mbps: u32) -> u64 {
    mbps as u64 * 1_000_000 / 8
}

fn edge_flag_names(f: u32) -> Vec<String> {
    let mut v = Vec::new();
    if f & VME_ISOLATE_IN != 0 {
        v.push("isolate_ingress".to_string());
    }
    if f & VME_ISOLATE_OUT != 0 {
        v.push("isolate_egress".to_string());
    }
    if f & VME_GUEST_SIDE != 0 {
        v.push("guest_side".to_string());
    }
    if f & VME_DENY_IN != 0 {
        v.push("deny_ingress".to_string());
    }
    if f & VME_DENY_OUT != 0 {
        v.push("deny_egress".to_string());
    }
    if f & VME_FLOW_LOG != 0 {
        v.push("flow_log".to_string());
    }
    if f & VME_FQDN != 0 {
        v.push("fqdn".to_string());
    }
    if f & VME_L7_AUTH != 0 {
        v.push("l7_auth".to_string());
    }
    if f & VME_SRC_GUARD != 0 {
        v.push("source_guard".to_string());
    }
    v
}

/// `c 10:232 rwm`, `b 8:* r`, `c 136:* rw` → device rule.
pub(crate) fn parse_dev_rule(s: &str) -> Result<QemuDevRule> {
    let mut it = s.split_whitespace();
    let ty = match it.next() {
        Some("c") => DEVCG_DEV_CHAR,
        Some("b") => DEVCG_DEV_BLOCK,
        _ => return Err(anyhow!("device rule `{s}`: expected `c` or `b`")),
    };
    let (maj, min) = it
        .next()
        .and_then(|mm| mm.split_once(':'))
        .ok_or_else(|| anyhow!("device rule `{s}`: expected MAJOR:MINOR"))?;
    let major: u32 = maj
        .parse()
        .map_err(|_| anyhow!("device rule `{s}`: bad major"))?;
    let minor = if min == "*" {
        DEV_MINOR_ANY
    } else {
        min.parse()
            .map_err(|_| anyhow!("device rule `{s}`: bad minor"))?
    };
    let mut access = 0;
    for c in it.next().unwrap_or("rwm").chars() {
        access |= match c {
            'r' => DEVCG_ACC_READ,
            'w' => DEVCG_ACC_WRITE,
            'm' => DEVCG_ACC_MKNOD,
            _ => return Err(anyhow!("device rule `{s}`: access is a subset of rwm")),
        };
    }
    Ok(QemuDevRule {
        major,
        minor,
        dev_type: ty,
        access,
    })
}

pub(super) fn dev_rule_string(r: &QemuDevRule) -> String {
    let ty = if r.dev_type == DEVCG_DEV_BLOCK {
        'b'
    } else {
        'c'
    };
    let minor = if r.minor == DEV_MINOR_ANY {
        "*".to_string()
    } else {
        r.minor.to_string()
    };
    let mut acc = String::new();
    for (bit, c) in [
        (DEVCG_ACC_READ, 'r'),
        (DEVCG_ACC_WRITE, 'w'),
        (DEVCG_ACC_MKNOD, 'm'),
    ] {
        if r.access & bit != 0 {
            acc.push(c);
        }
    }
    format!("{ty} {}:{minor} {acc}", r.major)
}

/// `49152-49215`, `10809` → ports.
pub(crate) fn parse_ports(specs: &[String]) -> Result<HashSet<u16>> {
    let mut out = HashSet::new();
    for s in specs {
        let (a, b) = s.split_once('-').unwrap_or((s, s));
        let (a, b): (u16, u16) = (
            a.trim().parse().map_err(|_| anyhow!("bad port `{s}`"))?,
            b.trim().parse().map_err(|_| anyhow!("bad port `{s}`"))?,
        );
        if a == 0 || b < a || (b - a) > 1024 {
            return Err(anyhow!("bad port range `{s}`"));
        }
        out.extend(a..=b);
    }
    if out.len() > 1024 {
        return Err(anyhow!("at most 1024 egress ports"));
    }
    Ok(out)
}

/// Char device numbers of the nodes QEMU needs (libvirt's default ACL plus
/// vhost/tun/vfio), resolved on this host; absent nodes are skipped.
pub(super) fn default_dev_rules() -> Vec<QemuDevRule> {
    let mut out = Vec::new();
    for node in [
        "/dev/null",
        "/dev/zero",
        "/dev/full",
        "/dev/random",
        "/dev/urandom",
        "/dev/ptmx",
        "/dev/kvm",
        "/dev/vhost-net",
        "/dev/vhost-vsock",
        "/dev/net/tun",
        "/dev/vfio/vfio",
        "/dev/sev",
        "/dev/userfaultfd",
    ] {
        if let Ok(m) = std::fs::metadata(node) {
            let rdev = m.rdev();
            let major = (((rdev >> 32) & 0xffff_f000) | ((rdev >> 8) & 0xfff)) as u32;
            let minor = (((rdev >> 12) & 0xffff_ff00) | (rdev & 0xff)) as u32;
            out.push(QemuDevRule {
                major,
                minor,
                dev_type: DEVCG_DEV_CHAR,
                access: DEVCG_ACC_READ | DEVCG_ACC_WRITE,
            });
        }
    }
    // Pseudo-terminals (serial consoles) and VFIO group nodes.
    out.push(QemuDevRule {
        major: 136,
        minor: DEV_MINOR_ANY,
        dev_type: DEVCG_DEV_CHAR,
        access: DEVCG_ACC_READ | DEVCG_ACC_WRITE,
    });
    if let Some(m) = char_major("vfio") {
        out.push(QemuDevRule {
            major: m,
            minor: DEV_MINOR_ANY,
            dev_type: DEVCG_DEV_CHAR,
            access: DEVCG_ACC_READ | DEVCG_ACC_WRITE,
        });
    }
    out
}

fn char_major(name: &str) -> Option<u32> {
    let s = std::fs::read_to_string("/proc/devices").ok()?;
    let chars = s.split("Block devices:").next()?;
    chars.lines().find_map(|l| {
        let (num, n) = l.trim().split_once(' ')?;
        (n.trim() == name).then(|| num.parse().ok()).flatten()
    })
}

/// Running QEMU scopes: VM name → cgroup path relative to the cgroup root.
pub(super) fn qemu_scopes() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(Path::new(attribution::CGROUP_ROOT).join(MACHINE_SLICE)) else {
        return out;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with("machine-qemu") {
            continue;
        }
        let rel = format!("{MACHINE_SLICE}/{name}");
        if let Some(vm) = attribution::classify_cgroup(&rel).vm {
            out.insert(vm, rel);
        }
    }
    out
}

impl Engine {
    pub(super) fn lease_live(&self) -> bool {
        self.mode == Mode::Enforce && loader::monotonic_ns() < self.lease_deadline_mono
    }

    // ---- VM edge -------------------------------------------------------------

    pub(super) fn vm_edge_sync(&mut self, state: VmEdgeState) -> Result<VmEdgeStatus> {
        let mut names = HashSet::new();
        for vm in &state.vms {
            if vm.name.is_empty() || !names.insert(vm.name.as_str()) {
                return Err(anyhow!(
                    "vm_edge_sync: VM names must be unique and non-empty"
                ));
            }
        }
        let mut groups: BTreeMap<String, u32> = BTreeMap::new();
        let mut ips: HashMap<[u8; ADDR_LEN], u32> = HashMap::new();
        let mut index = FlowIndex::default();
        for (id, n) in [
            (IDENTITY_HOST, "host"),
            (IDENTITY_WORLD, "world"),
            (crate::netpol::IDENTITY_REMOTE_NODE, "remote-node"),
        ] {
            index.names.insert(id, (n.to_string(), BTreeMap::new()));
        }
        for vm in &state.vms {
            let id = vm_ident(vm);
            groups.insert(
                vm.identity
                    .map_or_else(|| vm_group(vm), |_| format!("vm:{}", vm.name)),
                id,
            );
            if let Some(g) = &vm.group {
                groups.entry(g.clone()).or_insert(id);
            }
            index.names.insert(id, (vm.name.clone(), vm.labels.clone()));
            for a in &vm.addresses {
                ips.insert(addr16(a)?, id);
            }
        }
        let mut cidrs: Vec<(Prefix, u32)> = Vec::new();
        for p in &state.peers {
            if p.cidr.contains('/') {
                let pre =
                    policy::parse_prefix(&p.cidr).map_err(|e| anyhow!("peer {}: {e}", p.cidr))?;
                cidrs.push((pre, p.identity));
            } else {
                ips.entry(addr16(&p.cidr)?).or_insert(p.identity);
            }
            if !p.name.is_empty() {
                index
                    .names
                    .entry(p.identity)
                    .or_insert_with(|| (p.name.clone(), BTreeMap::new()));
            }
        }
        if state.node_is_host {
            for a in node_addresses() {
                ips.entry(a).or_insert(IDENTITY_HOST);
            }
        }
        let mut policy: HashMap<PolicyKey, u32> = HashMap::new();
        let mut deny: HashSet<(u32, bool)> = HashSet::new();
        let mut l7auth: HashSet<u32> = state
            .fqdn
            .iter()
            .filter(|r| r.l7.is_some())
            .map(|r| r.subject_identity)
            .collect();
        let mut auth_any = false;
        for r in &state.policy {
            let subject = match r.subject_identity {
                Some(id) => id,
                None => *groups
                    .get(&r.group)
                    .ok_or_else(|| anyhow!("rule for unknown group `{}`", r.group))?,
            };
            let peer = match (r.peer_identity, r.peer.as_deref()) {
                (Some(id), _) => id,
                (None, None | Some("") | Some("any")) => 0,
                (None, Some("world")) => IDENTITY_WORLD,
                (None, Some(p)) => *groups
                    .get(p)
                    .ok_or_else(|| anyhow!("rule peer `{p}` is not a group"))?,
            };
            let end = if r.port_end > r.port {
                r.port_end
            } else {
                r.port
            };
            if end - r.port >= crate::netpol::MAX_PORT_RANGE as u16 {
                return Err(anyhow!(
                    "rule port range {}-{end} wider than {}",
                    r.port,
                    crate::netpol::MAX_PORT_RANGE
                ));
            }
            let val = rule_val(r.deny, r.auth, r.l7);
            if r.deny {
                deny.insert((subject, r.egress));
            }
            if r.auth != 0 || r.l7 {
                l7auth.insert(subject);
                auth_any |= r.auth != 0;
            }
            for port in r.port..=end {
                let k = PolicyKey {
                    subject_identity: subject,
                    peer_identity: peer,
                    direction: if r.egress {
                        POLICY_EGRESS
                    } else {
                        POLICY_INGRESS
                    },
                    proto: r.proto,
                    port: port.to_be_bytes(),
                };
                let e = policy.entry(k).or_insert(val);
                *e = merge_val(*e, val);
                let src = r.source.clone().unwrap_or_default();
                let ie = index
                    .rules
                    .entry((subject, peer, r.egress, r.proto, port))
                    .or_insert((r.deny, src.clone()));
                if r.deny && !ie.0 {
                    *ie = (true, src);
                }
            }
        }
        if policy.len() > VM_POLICY_CAP {
            return Err(anyhow!(
                "{} VM policy entries exceed the map size ({VM_POLICY_CAP})",
                policy.len()
            ));
        }

        self.dp.addr_lpm_clear::<u32>("VM_CIDR_IDS")?;
        for (p, id) in &cidrs {
            self.dp
                .addr_lpm_insert("VM_CIDR_IDS", p.addr, p.bits, *id)?;
        }
        let patterns: Vec<String> = state
            .fqdn
            .iter()
            .map(|r| r.pattern.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if patterns.is_empty() {
            self.vm_edge.fqdn_cache.clear();
        }
        lock(&self.shared).vm_fqdn_patterns = patterns;
        self.vm_edge.base_ips = ips;
        self.vm_edge.base_policy = policy;
        self.vm_edge.base_index = index;
        self.vm_edge.cidrs = cidrs;
        self.vm_edge.deny = deny;
        self.vm_edge.l7auth = l7auth;
        self.vm_edge.auth_any = auth_any;
        if !auth_any {
            self.vm_edge.auth.clear();
        }
        self.vm_edge.groups = groups;
        self.vm_edge.state = state;
        self.vm_edge_apply()?;
        // Re-push every tap: limits or isolation may have changed.
        for (tap, (idx, _)) in std::mem::take(&mut self.vm_edge.taps) {
            self.vm_edge_unprogram(&tap, idx);
        }
        self.vm_edge_refresh();
        Ok(self.vm_edge_status())
    }

    /// Base state from the last sync plus live `toFQDNs` bindings → maps.
    ///
    /// A learned address outside the exact map gets its own identity (as a
    /// /128 CIDR peer) that inherits every rule of the identity it resolved
    /// to before (longest CIDR or `world`), so deny entries keep winning,
    /// plus allow entries of each matching `toFQDNs` rule.
    fn vm_edge_apply(&mut self) -> Result<()> {
        let rt = &self.vm_edge;
        let mut ips = rt.base_ips.clone();
        let mut policy = rt.base_policy.clone();
        let mut index = rt.base_index.clone();
        let mut fqdn_ids = HashMap::new();
        let mut l7: Vec<VmEdgeL7Rule> = rt.state.l7.clone();
        let now = Instant::now();
        let mut learned: Vec<(&[u8; ADDR_LEN], &FqdnBinding)> = rt.fqdn_cache.iter().collect();
        learned.sort_by_key(|(a, _)| **a);
        for (addr, b) in learned {
            let names: Vec<&String> = b
                .names
                .iter()
                .filter(|(_, exp)| **exp > now)
                .map(|(n, _)| n)
                .collect();
            let rules: Vec<&VmEdgeFqdnRule> = rt
                .state
                .fqdn
                .iter()
                .filter(|r| {
                    names
                        .iter()
                        .any(|n| crate::netpol::fqdn::matches(&r.pattern, n))
                })
                .collect();
            if rules.is_empty() {
                continue;
            }
            let (id, inherit) = match rt.base_ips.get(addr) {
                Some(id) => (*id, None),
                None => {
                    let from = rt
                        .cidrs
                        .iter()
                        .filter(|(p, _)| prefix_has(p, addr))
                        .max_by_key(|(p, _)| p.bits)
                        .map_or(IDENTITY_WORLD, |(_, id)| *id);
                    (
                        crate::netpol::cidr_identity(&Prefix {
                            addr: *addr,
                            bits: 128,
                        }),
                        Some(from),
                    )
                }
            };
            let mut add: Vec<(PolicyKey, u32)> = Vec::new();
            if let Some(from) = inherit {
                add.extend(
                    rt.base_policy
                        .iter()
                        .filter(|(k, _)| k.peer_identity == from)
                        .map(|(k, v)| {
                            (
                                PolicyKey {
                                    peer_identity: id,
                                    ..*k
                                },
                                *v,
                            )
                        }),
                );
                l7.extend(
                    rt.state
                        .l7
                        .iter()
                        .filter(|r| r.peer_identity == from)
                        .map(|r| VmEdgeL7Rule {
                            peer_identity: id,
                            ..r.clone()
                        }),
                );
            }
            for r in &rules {
                let end = if r.port_end > r.port {
                    r.port_end
                } else {
                    r.port
                };
                for port in r.port..=end {
                    let k = PolicyKey {
                        subject_identity: r.subject_identity,
                        peer_identity: id,
                        direction: POLICY_EGRESS,
                        proto: r.proto,
                        port: port.to_be_bytes(),
                    };
                    add.push((k, rule_val(false, 0, r.l7.is_some())));
                }
                if let Some(rules) = &r.l7 {
                    l7.push(VmEdgeL7Rule {
                        subject_identity: r.subject_identity,
                        peer_identity: id,
                        egress: true,
                        proto: r.proto,
                        port: r.port,
                        port_end: r.port_end,
                        rules: rules.clone(),
                        source: r.source.clone(),
                    });
                }
            }
            if policy.len() + add.len() > VM_POLICY_CAP {
                tracing::warn!(
                    "vm edge: VM policy map full; {} not applied",
                    fmt_addr(addr)
                );
                continue;
            }
            if let Some(from) = inherit {
                ips.insert(*addr, id);
                let copied: Vec<_> = index
                    .rules
                    .iter()
                    .filter(|(k, _)| k.1 == from)
                    .map(|(k, v)| ((k.0, id, k.2, k.3, k.4), v.clone()))
                    .collect();
                index.rules.extend(copied);
                index
                    .names
                    .insert(id, (format!("fqdn:{}", names[0]), BTreeMap::new()));
            }
            for (k, v) in add {
                let e = policy.entry(k).or_insert(v);
                *e = merge_val(*e, v);
            }
            for r in &rules {
                let end = if r.port_end > r.port {
                    r.port_end
                } else {
                    r.port
                };
                for port in r.port..=end {
                    let src = r.source.clone().unwrap_or_default();
                    index
                        .rules
                        .entry((r.subject_identity, id, true, r.proto, port))
                        .or_insert((false, src));
                }
            }
            fqdn_ids.insert(*addr, id);
        }
        rt.threat_entries(&mut ips, &mut policy, &mut index);
        rt.quarantine_entries(&mut ips, &mut policy, &mut index);

        let mut want_proxy = false;
        for r in l7.iter().filter(|r| r.egress && r.rules.needs_proxy()) {
            for port in r.port..=r.port_end.max(r.port) {
                let k = PolicyKey {
                    subject_identity: r.subject_identity,
                    peer_identity: r.peer_identity,
                    direction: POLICY_EGRESS,
                    proto: r.proto,
                    port: port.to_be_bytes(),
                };
                if let Some(v) = policy.get_mut(&k).filter(|v| **v & VM_POLICY_L7 != 0) {
                    *v |= VM_POLICY_PROXY;
                    want_proxy = true;
                }
            }
        }
        for (k, id) in self.vm_edge.ips.clone() {
            if ips.get(&k) != Some(&id) {
                self.dp.cni_hash_remove::<[u8; ADDR_LEN], u32>("VM_IPS", &k);
            }
        }
        for (k, id) in &ips {
            if self.vm_edge.ips.get(k) != Some(id) {
                self.dp.cni_hash_insert("VM_IPS", *k, *id)?;
            }
        }
        for k in self.vm_edge.policy.keys().copied().collect::<Vec<_>>() {
            if !policy.contains_key(&k) {
                self.dp.cni_hash_remove::<PolicyKey, u32>("VM_POLICY", &k);
            }
        }
        for (k, v) in &policy {
            if self.vm_edge.policy.get(k) != Some(v) {
                self.dp.cni_hash_insert("VM_POLICY", *k, *v)?;
            }
        }
        self.vm_edge.ips = ips;
        self.vm_edge.policy = policy;
        self.vm_edge.fqdn_ids = fqdn_ids;
        self.vm_edge.l7_rules = l7.len();
        let inject = if l7.is_empty() {
            None
        } else {
            Some(self.ensure_l7_inject(want_proxy))
        };
        let (cfg, note) = match inject.filter(|i| *i != 0 && want_proxy) {
            None if want_proxy => (VmProxyCfg::default(), "off: no inject veth".to_string()),
            None => (VmProxyCfg::default(), String::new()),
            Some(i) => match self
                .vmproxy
                .ensure_running()
                .and_then(|port| ensure_proxy_route(L7_INJECT.1).map(|_| port))
            {
                Ok(port) => (
                    VmProxyCfg {
                        redirect_ifindex: i,
                        port,
                        _pad: 0,
                    },
                    format!("listening on 127.0.0.1:{port}"),
                ),
                Err(e) => {
                    tracing::warn!("vm proxy: {e:#}; proxy rules pass unproxied");
                    (VmProxyCfg::default(), format!("off: {e:#}"))
                }
            },
        };
        if cfg != self.vm_edge.proxy_cfg {
            self.dp.set_vm_proxy(cfg)?;
            self.vm_edge.proxy_cfg = cfg;
        }
        self.vm_edge.proxy_note = note;
        let mut sh = lock(&self.shared);
        if let Some(i) = inject {
            sh.vm_l7_inject = i;
        }
        sh.vm_flow_index = index;
        sh.vm_l7_rules = Arc::new(l7);
        sh.vm_l7_gen += 1;
        Ok(())
    }

    /// Create the L7 reinject veth pair if needed; returns the send side's
    /// ifindex, 0 when it cannot be set up (held frames then wait for the
    /// client's retransmit). `v6_rx`: proxied IPv6 frames enter the stack on
    /// the receive side.
    fn ensure_l7_inject(&mut self, v6_rx: bool) -> u32 {
        let (tx, rx) = L7_INJECT;
        if if_nametoindex(tx).is_none() {
            let ok = std::process::Command::new("ip")
                .args(["link", "add", tx, "type", "veth", "peer", "name", rx])
                .status()
                .is_ok_and(|s| s.success());
            if !ok {
                tracing::warn!(
                    "vm l7: cannot create {tx}/{rx}; held segments wait for retransmits"
                );
                return 0;
            }
        }
        for d in [tx, rx] {
            let v6 = d == rx && v6_rx;
            if v6 {
                for k in ["accept_ra", "autoconf"] {
                    let _ = std::fs::write(format!("/proc/sys/net/ipv6/conf/{d}/{k}"), "0");
                }
            }
            let _ = std::fs::write(
                format!("/proc/sys/net/ipv6/conf/{d}/disable_ipv6"),
                if v6 { "0" } else { "1" },
            );
            let _ = std::process::Command::new("ip")
                .args(["link", "set", d, "up"])
                .status();
        }
        if let Err(e) = self.dp.attach_tc_first(rx, L7_INJECT_PROG, true) {
            tracing::warn!("vm l7: attach {L7_INJECT_PROG} on {rx}: {e:#}");
            return 0;
        }
        if_nametoindex(tx).unwrap_or(0)
    }

    /// Take queued DNS answers, expire old names, reapply on change.
    pub(super) fn vm_fqdn_tick(&mut self) -> Result<()> {
        let queue = std::mem::take(&mut lock(&self.shared).vm_fqdn_queue);
        if queue.is_empty() && self.vm_edge.fqdn_cache.is_empty() {
            return Ok(());
        }
        let now = Instant::now();
        let mut changed = false;
        for l in queue {
            let ttl = Duration::from_secs(l.ttl as u64).clamp(FQDN_MIN_TTL, FQDN_MAX_TTL);
            let b = self
                .vm_edge
                .fqdn_cache
                .entry(l.addr)
                .or_insert_with(|| FqdnBinding {
                    names: BTreeMap::new(),
                    vm: String::new(),
                });
            let tap_vm = self
                .vm_edge
                .taps
                .values()
                .find(|(i, _)| *i == l.ifindex)
                .map(|(_, v)| v.clone());
            if let Some(vm) = tap_vm.or(Some(l.vm)).filter(|v| !v.is_empty()) {
                b.vm = vm;
            }
            for n in l.names {
                let exp = b.names.entry(n).or_insert(now);
                if *exp <= now {
                    changed = true;
                }
                *exp = (*exp).max(now + ttl);
            }
        }
        for b in self.vm_edge.fqdn_cache.values_mut() {
            let before = b.names.len();
            b.names.retain(|_, exp| *exp > now);
            changed |= b.names.len() != before;
        }
        self.vm_edge.fqdn_cache.retain(|_, b| !b.names.is_empty());
        while self.vm_edge.fqdn_cache.len() > FQDN_CACHE_CAP {
            let oldest = self
                .vm_edge
                .fqdn_cache
                .iter()
                .min_by_key(|(_, b)| b.names.values().max().copied())
                .map(|(a, _)| *a);
            match oldest {
                Some(a) => {
                    self.vm_edge.fqdn_cache.remove(&a);
                    changed = true;
                }
                None => break,
            }
        }
        if changed {
            self.vm_edge_apply()?;
        }
        Ok(())
    }

    pub(super) fn vm_fqdn_cache(&self) -> Vec<VmFqdnEntry> {
        let now = Instant::now();
        let mut out = Vec::new();
        for (addr, b) in &self.vm_edge.fqdn_cache {
            for (name, exp) in &b.names {
                let patterns: Vec<String> = self
                    .vm_edge
                    .state
                    .fqdn
                    .iter()
                    .filter(|r| crate::netpol::fqdn::matches(&r.pattern, name))
                    .map(|r| r.pattern.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                out.push(VmFqdnEntry {
                    name: name.clone(),
                    address: fmt_addr(addr),
                    identity: self.vm_edge.fqdn_ids.get(addr).copied().unwrap_or(0),
                    vm: b.vm.clone(),
                    expires_in_secs: exp.saturating_duration_since(now).as_secs(),
                    patterns,
                });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name).then(a.address.cmp(&b.address)));
        out
    }

    fn vm_edge_unprogram(&mut self, tap: &str, idx: u32) {
        self.dp.detach_tc_one(tap, EDGE_IN);
        self.dp.detach_tc_one(tap, EDGE_OUT);
        self.dp.cni_hash_remove::<u32, VmEdgeCfg>("VM_EDGE", &idx);
        for k in [idx << 1, (idx << 1) | 1] {
            self.dp.cni_hash_remove::<u32, VmBucket>("VM_BUCKETS", &k);
        }
        self.dp
            .percpu_remove::<u32, VmEdgeStats>("VM_EDGE_STATS", &idx);
    }

    /// Program taps of VMs in the edge state; drop taps that went away.
    pub(super) fn vm_edge_refresh(&mut self) {
        let watch = self.vm_edge.threat_watch();
        if self.vm_edge.state.vms.is_empty()
            && self.vm_edge.taps.is_empty()
            && self.vm_edge.quarantines.is_empty()
            && !watch
        {
            return;
        }
        let known = |n: &str| self.vm_edge.state.vms.iter().any(|v| v.name == n);
        let scanned: Vec<(String, String)> = if watch
            || !self.vm_edge.quarantines.is_empty()
            || self.vm_edge.state.vms.iter().any(|v| v.taps.is_empty())
        {
            attribution::scan_libvirt()
                .into_iter()
                .map(|(tap, info)| (tap, info.vm))
                .collect()
        } else {
            Vec::new()
        };
        let mut synth: BTreeSet<String> = self
            .vm_edge
            .quarantines
            .keys()
            .filter(|n| !known(n))
            .cloned()
            .collect();
        let watched: BTreeSet<String> = if watch {
            scanned
                .iter()
                .map(|(_, vm)| vm.clone())
                .filter(|n| !known(n))
                .collect()
        } else {
            BTreeSet::new()
        };
        synth.extend(watched.iter().cloned());
        let synthesized: Vec<VmEdgeVm> = synth
            .iter()
            .map(|n| VmEdgeVm {
                name: n.clone(),
                identity: Some(crate::netpol::vm_identity(n)),
                ..Default::default()
            })
            .collect();
        let by_name: HashMap<&str, &VmEdgeVm> = self
            .vm_edge
            .state
            .vms
            .iter()
            .chain(&synthesized)
            .map(|v| (v.name.as_str(), v))
            .collect();
        let mut taps: Vec<(String, &VmEdgeVm)> = Vec::new();
        for (tap, vm) in &scanned {
            if let Some(vm) = by_name.get(vm.as_str()).filter(|v| v.taps.is_empty()) {
                taps.push((tap.clone(), vm));
            }
        }
        for vm in &self.vm_edge.state.vms {
            taps.extend(vm.taps.iter().map(|t| (t.clone(), vm)));
        }
        let mut want: HashMap<String, (String, VmEdgeCfg)> = HashMap::new();
        for (tap, vm) in taps {
            let guest_side = if_nametoindex(&tap)
                .and_then(|i| self.ifaces.get(&i))
                .map(|r| r.guest_side)
                .unwrap_or(true);
            let identity = vm_ident(vm);
            let mut flags = 0;
            if vm.isolate_ingress {
                flags |= VME_ISOLATE_IN;
            }
            if vm.isolate_egress {
                flags |= VME_ISOLATE_OUT;
            }
            if guest_side {
                flags |= VME_GUEST_SIDE;
            }
            if self.vm_edge.deny.contains(&(identity, false)) {
                flags |= VME_DENY_IN;
            }
            if self.vm_edge.deny.contains(&(identity, true)) || self.vm_edge.threat_blocking() {
                flags |= VME_DENY_OUT;
            }
            if self.vm_edge.state.flow_log {
                flags |= VME_FLOW_LOG;
            }
            if watch
                || self
                    .vm_edge
                    .state
                    .fqdn
                    .iter()
                    .any(|r| r.subject_identity == identity)
            {
                flags |= VME_FQDN;
            }
            if self.vm_edge.l7auth.contains(&identity) {
                flags |= VME_L7_AUTH;
            }
            if self.vm_edge.auth_any {
                flags |= VME_SRC_GUARD;
            }
            let cfg = VmEdgeCfg {
                identity,
                flags,
                out_bps: mbps_to_bytes(vm.egress_mbps),
                in_bps: mbps_to_bytes(vm.ingress_mbps),
                pps: vm.pps,
                _pad: 0,
                quarantine_until_ns: self.vm_edge.quarantine_until(&vm.name),
            };
            want.insert(tap, (vm.name.clone(), cfg));
        }
        let stale: Vec<(String, u32)> = self
            .vm_edge
            .taps
            .iter()
            .filter(|(t, (idx, _))| !want.contains_key(*t) || if_nametoindex(t) != Some(*idx))
            .map(|(t, (idx, _))| (t.clone(), *idx))
            .collect();
        for (tap, idx) in stale {
            if if_nametoindex(&tap) == Some(idx) {
                self.vm_edge_unprogram(&tap, idx);
            } else {
                self.dp.forget_tc(&tap);
                self.dp.cni_hash_remove::<u32, VmEdgeCfg>("VM_EDGE", &idx);
                self.dp
                    .percpu_remove::<u32, VmEdgeStats>("VM_EDGE_STATS", &idx);
            }
            self.vm_edge.taps.remove(&tap);
        }
        if self.vm_edge.threat_vms != watched {
            self.vm_edge.threat_vms = watched;
            if self.vm_edge.threat_blocking() {
                if let Err(e) = self.vm_edge_apply() {
                    tracing::warn!("threat feed entries: {e:#}");
                }
            }
        }
        let programmed: HashMap<u32, VmEdgeCfg> = if self.vm_edge.taps.is_empty() {
            HashMap::new()
        } else {
            self.dp
                .hash_entries::<u32, VmEdgeCfg>("VM_EDGE")
                .unwrap_or_default()
                .into_iter()
                .collect()
        };
        for (tap, (vm, cfg)) in want {
            if let Some((idx, _)) = self.vm_edge.taps.get(&tap) {
                let idx = *idx;
                if programmed.get(&idx).is_some_and(|c| *c != cfg) {
                    if let Err(e) = self.dp.cni_hash_insert("VM_EDGE", idx, cfg) {
                        tracing::warn!("vm edge on {tap}: {e:#}");
                    }
                }
                continue;
            }
            let Some(idx) = if_nametoindex(&tap) else {
                continue;
            };
            let res = self
                .dp
                .cni_hash_insert("VM_EDGE", idx, cfg)
                .and_then(|_| self.dp.attach_tc_one(&tap, EDGE_IN, true))
                .and_then(|_| self.dp.attach_tc_one(&tap, EDGE_OUT, false));
            match res {
                Ok(()) => {
                    self.vm_edge.taps.insert(tap, (idx, vm));
                }
                Err(e) => {
                    tracing::warn!("vm edge on {tap}: {e:#}");
                    self.vm_edge_unprogram(&tap, idx);
                }
            }
        }
    }

    pub(super) fn vm_edge_status(&mut self) -> VmEdgeStatus {
        let stats: HashMap<u32, VmEdgeStats> = self
            .dp
            .percpu_sum::<u32, VmEdgeStats>("VM_EDGE_STATS", |a, b| {
                a.out_pkts += b.out_pkts;
                a.out_bytes += b.out_bytes;
                a.in_pkts += b.in_pkts;
                a.in_bytes += b.in_bytes;
                a.denied += b.denied;
                a.observed += b.observed;
                a.rate_dropped += b.rate_dropped;
            })
            .unwrap_or_default()
            .into_iter()
            .collect();
        let cfgs: HashMap<u32, VmEdgeCfg> = self
            .dp
            .hash_entries::<u32, VmEdgeCfg>("VM_EDGE")
            .unwrap_or_default()
            .into_iter()
            .collect();
        let mut taps: Vec<VmEdgeTap> = self
            .vm_edge
            .taps
            .iter()
            .map(|(tap, (idx, vm))| {
                let cfg = cfgs.get(idx).copied().unwrap_or_default();
                let s = stats.get(idx).copied().unwrap_or_default();
                VmEdgeTap {
                    vm: vm.clone(),
                    iface: tap.clone(),
                    identity: cfg.identity,
                    flags: edge_flag_names(cfg.flags),
                    stats: VmEdgeCounters {
                        out_pkts: s.out_pkts,
                        out_bytes: s.out_bytes,
                        in_pkts: s.in_pkts,
                        in_bytes: s.in_bytes,
                        denied: s.denied,
                        observed: s.observed,
                        rate_dropped: s.rate_dropped,
                    },
                }
            })
            .collect();
        taps.sort_by(|a, b| a.iface.cmp(&b.iface));
        let covered: HashSet<&str> = self
            .vm_edge
            .taps
            .values()
            .map(|(_, v)| v.as_str())
            .collect();
        VmEdgeStatus {
            vms: self.vm_edge.state.vms.len(),
            rules: self.vm_edge.policy.len(),
            groups: self.vm_edge.groups.clone(),
            missing: self
                .vm_edge
                .state
                .vms
                .iter()
                .filter(|v| !covered.contains(v.name.as_str()))
                .map(|v| v.name.clone())
                .collect(),
            taps,
            enforcing: self.lease_live(),
            owner: self.vm_edge.state.owner.clone(),
            peers: self.vm_edge.state.peers.len(),
            flow_log: self.vm_edge.state.flow_log,
            cilium: crate::netpol::cilium_present(),
            fqdn_rules: self.vm_edge.state.fqdn.len(),
            fqdn_cache: self.vm_edge.fqdn_ids.len(),
            l7_rules: self.vm_edge.l7_rules,
            auth_entries: self.vm_edge.auth.values().filter(|a| a.ok).count(),
            auth_cert: self.vmauth.cert_info(),
            proxy: self.vm_edge.proxy_note.clone(),
            quarantines: self.vm_quarantines(),
            node_addrs: node_cidrs(),
            learned: lock(&self.shared).vm_learned.report(Instant::now()),
        }
    }

    /// Authenticate identity pairs the datapath asked for. `required` holds
    /// for a VM on this host (the source guard on every tap keeps its
    /// identity unforgeable) and, for a VM on another host, after a mutual
    /// TLS handshake with that host's bpfd (see `vmauth`);
    /// `test-always-fail` never authenticates.
    pub(super) fn vm_auth_tick(&mut self) -> Result<()> {
        let queue = std::mem::take(&mut lock(&self.shared).vm_auth_queue);
        let now = Instant::now();
        self.vm_edge.auth.retain(|_, a| a.expires > now);
        for h in self.vmauth.take_done() {
            let mode = self.auth_mode(h.pair.0, h.pair.1);
            self.auth_set(h.pair, mode, h.ok, h.note)?;
        }
        if self.vmauth.has_cert() {
            self.vmauth.set_view(self.auth_view());
        }
        if queue.is_empty() {
            return Ok(());
        }
        let local: HashMap<u32, String> = self.auth_view().local;
        for (subject, peer) in queue {
            if self
                .vm_edge
                .auth
                .get(&(subject, peer))
                .is_some_and(|a| a.ok)
                || self.vmauth.is_pending((subject, peer))
            {
                continue;
            }
            let mode = self.auth_mode(subject, peer);
            let remote = self
                .vm_edge
                .state
                .peers
                .iter()
                .find(|p| {
                    p.identity == peer
                        && !p.cidr.contains('/')
                        && peer >= 16
                        && peer & 0x8000_0000 == 0
                })
                .map(|p| (p.name.clone(), p.host.clone()));
            let (ok, note) = if mode & crate::api::AUTH_ALWAYS_FAIL != 0 {
                (false, "test-always-fail".to_string())
            } else if let Some(name) = local.get(&peer) {
                (true, format!("authenticated (local VM {name})"))
            } else if let Some((name, host)) = remote {
                if !self.vmauth.has_cert() {
                    (
                        false,
                        format!("VM {name} is on another host and this host has no certificate (the controller issues one)"),
                    )
                } else if host.is_empty() || host == self.vm_edge.state.host_id {
                    (false, format!("VM {name} has no known host"))
                } else {
                    self.vmauth.start((subject, peer), &host);
                    continue;
                }
            } else {
                (false, "peer is not a VM identity".to_string())
            };
            self.auth_set((subject, peer), mode, ok, note)?;
        }
        Ok(())
    }

    fn auth_mode(&self, subject: u32, peer: u32) -> u8 {
        self.vm_edge
            .state
            .policy
            .iter()
            .filter(|r| {
                r.subject_identity == Some(subject)
                    && r.peer_identity.is_some_and(|p| p == peer || p == 0)
            })
            .fold(0u8, |m, r| m | r.auth)
    }

    fn auth_set(
        &mut self,
        (subject, peer): (u32, u32),
        mode: u8,
        ok: bool,
        note: String,
    ) -> Result<()> {
        if ok {
            let key = VmAuthKey {
                subject,
                peer,
                mode: 1,
                _pad: [0; 3],
            };
            self.dp.cni_hash_insert(
                "VM_AUTH",
                key,
                loader::monotonic_ns() + AUTH_TTL.as_nanos() as u64,
            )?;
        }
        self.vm_edge.auth.insert(
            (subject, peer),
            AuthState {
                mode: mode.max(1),
                ok,
                note,
                expires: Instant::now() + AUTH_TTL,
            },
        );
        Ok(())
    }

    /// Local VM identities (on a tap here), fleet VM hosts and addresses.
    fn auth_view(&self) -> super::vmauth::View {
        let st = &self.vm_edge.state;
        let on_tap: HashSet<&str> = self
            .vm_edge
            .taps
            .values()
            .map(|(_, v)| v.as_str())
            .collect();
        super::vmauth::View {
            host_id: st.host_id.clone(),
            local: st
                .vms
                .iter()
                .filter(|v| on_tap.contains(v.name.as_str()))
                .map(|v| (vm_ident(v), v.name.clone()))
                .collect(),
            vm_hosts: st
                .peers
                .iter()
                .filter(|p| !p.host.is_empty())
                .map(|p| (p.identity, p.host.clone()))
                .collect(),
            host_addrs: st.host_addrs.clone(),
        }
    }

    pub(super) fn vm_auth_table(&self) -> Vec<VmAuthEntry> {
        let now = Instant::now();
        let idx = lock(&self.shared).vm_flow_index.clone();
        let name = |id: u32| {
            idx.name(id)
                .map_or_else(|| format!("identity:{id}"), |n| n.0.clone())
        };
        let mut out: Vec<VmAuthEntry> = self
            .vm_edge
            .auth
            .iter()
            .map(|((s, p), a)| VmAuthEntry {
                subject: name(*s),
                subject_identity: *s,
                peer: name(*p),
                peer_identity: *p,
                mode: if a.mode & crate::api::AUTH_ALWAYS_FAIL != 0 {
                    "test-always-fail"
                } else {
                    "required"
                }
                .into(),
                state: a.note.clone(),
                expires_in_secs: if a.ok {
                    a.expires.saturating_duration_since(now).as_secs()
                } else {
                    0
                },
            })
            .collect();
        out.sort_by(|a, b| a.subject.cmp(&b.subject).then(a.peer.cmp(&b.peer)));
        out
    }

    // ---- QEMU sandbox --------------------------------------------------------

    pub(super) fn vm_sandbox_configure(
        &mut self,
        config: VmSandboxConfig,
    ) -> Result<VmSandboxStatus> {
        let enforce = match config.mode.as_str() {
            "observe" => false,
            "enforce" => true,
            m => return Err(anyhow!("sandbox mode `{m}`: expected observe or enforce")),
        };
        let mut rules = default_dev_rules();
        for s in &config.extra_devices {
            rules.push(parse_dev_rule(s)?);
        }
        if rules.len() > QEMU_DEV_RULES {
            return Err(anyhow!(
                "at most {QEMU_DEV_RULES} device rules (have {})",
                rules.len()
            ));
        }
        let ports = parse_ports(&config.egress_ports)?;
        let mut cfg = QemuSandboxCfg {
            n: rules.len() as u32,
            flags: if enforce { SANDBOX_ENFORCE } else { 0 },
            ..Default::default()
        };
        cfg.rules[..rules.len()].copy_from_slice(&rules);
        self.dp.array_set("QEMU_SANDBOX", 0, cfg)?;
        for p in self.sandbox.ports.clone() {
            if !ports.contains(&p) {
                self.dp.cni_hash_remove::<u16, u8>("QEMU_PORTS", &p);
            }
        }
        for p in &ports {
            if !self.sandbox.ports.contains(p) {
                self.dp.cni_hash_insert("QEMU_PORTS", *p, 1u8)?;
            }
        }
        self.sandbox.ports = ports;
        self.sandbox.devices = rules.iter().map(dev_rule_string).collect();
        self.sandbox.config = config;
        self.sandbox.configured = true;
        self.sandbox_refresh();
        Ok(self.vm_sandbox_status())
    }

    fn ensure_sandbox_config(&mut self) -> Result<()> {
        if !self.sandbox.configured {
            self.vm_sandbox_configure(self.sandbox.config.clone())?;
        }
        Ok(())
    }

    pub(super) fn vm_sandbox_attach(
        &mut self,
        vm: &str,
        cgroup: Option<&str>,
    ) -> Result<VmSandboxStatus> {
        self.ensure_sandbox_config()?;
        let rel = match cgroup {
            Some(c) => c.trim_matches('/').to_string(),
            None => qemu_scopes()
                .remove(vm)
                .ok_or_else(|| anyhow!("no running machine-qemu scope for VM {vm}"))?,
        };
        if rel.split('/').any(|s| s == "..") {
            return Err(anyhow!(
                "cgroup path must stay under {}",
                attribution::CGROUP_ROOT
            ));
        }
        if let Some(old) = self.sandbox.attached.get(vm).cloned() {
            if old != rel {
                self.dp
                    .detach_sandbox(&format!("{}/{old}", attribution::CGROUP_ROOT));
            }
        }
        self.dp
            .attach_sandbox(&Path::new(attribution::CGROUP_ROOT).join(&rel))?;
        self.sandbox.attached.insert(vm.to_string(), rel);
        self.sandbox.pinned.insert(vm.to_string());
        Ok(self.vm_sandbox_status())
    }

    pub(super) fn vm_sandbox_detach(&mut self, vm: &str) -> bool {
        self.sandbox.pinned.remove(vm);
        match self.sandbox.attached.remove(vm) {
            Some(rel) => {
                self.dp
                    .detach_sandbox(&format!("{}/{rel}", attribution::CGROUP_ROOT));
                true
            }
            None => false,
        }
    }

    /// Forget scopes that went away; with `auto` (or for pinned VMs that
    /// restarted) sandbox running QEMU scopes.
    pub(super) fn sandbox_refresh(&mut self) {
        if !self.sandbox.config.auto
            && self.sandbox.attached.is_empty()
            && self.sandbox.pinned.is_empty()
        {
            return;
        }
        let root = Path::new(attribution::CGROUP_ROOT);
        let gone: Vec<(String, String)> = self
            .sandbox
            .attached
            .iter()
            .filter(|(_, rel)| !root.join(rel).is_dir())
            .map(|(v, r)| (v.clone(), r.clone()))
            .collect();
        for (vm, rel) in gone {
            self.dp
                .forget_sandbox(&format!("{}/{rel}", attribution::CGROUP_ROOT));
            self.sandbox.attached.remove(&vm);
        }
        if self.ensure_sandbox_config().is_err() {
            return;
        }
        for (vm, rel) in qemu_scopes() {
            if self.sandbox.attached.contains_key(&vm)
                || !(self.sandbox.config.auto || self.sandbox.pinned.contains(&vm))
            {
                continue;
            }
            match self.dp.attach_sandbox(&root.join(&rel)) {
                Ok(()) => {
                    self.sandbox.attached.insert(vm, rel);
                }
                Err(e) => {
                    let n = format!("sandbox {vm}: {e:#}");
                    if !self.sandbox.notes.contains(&n) {
                        tracing::warn!("{n}");
                        self.sandbox.notes.push(n);
                    }
                }
            }
        }
    }

    pub(super) fn vm_sandbox_status(&mut self) -> VmSandboxStatus {
        let dev = self
            .dp
            .hash_entries::<DevHitKey, u64>("QEMU_DEV_HITS")
            .unwrap_or_default();
        let net = self
            .dp
            .hash_entries::<NetHitKey, u64>("QEMU_NET_HITS")
            .unwrap_or_default();
        let mut sh = lock(&self.shared);
        let mut who = |id: u64| -> (Option<String>, Option<String>) {
            let path = sh.cgroups.lookup(id);
            let vm = path
                .as_deref()
                .and_then(|p| attribution::classify_cgroup(p).vm);
            (vm, path)
        };
        let mut device_hits: Vec<SandboxHit> = dev
            .into_iter()
            .map(|(k, count)| {
                let (vm, cgroup) = who(k.cgroup);
                let r = QemuDevRule {
                    major: k.major,
                    minor: k.minor,
                    dev_type: k.dev_type as u32,
                    access: k.access as u32,
                };
                SandboxHit {
                    vm,
                    cgroup,
                    target: dev_rule_string(&r),
                    count,
                }
            })
            .collect();
        let mut egress_hits: Vec<SandboxHit> = net
            .into_iter()
            .map(|(k, count)| {
                let (vm, cgroup) = who(k.cgroup);
                let addr = fmt_addr(&k.addr);
                let port = u16::from_be_bytes(k.port);
                let target = if addr.contains(':') {
                    format!("{} [{addr}]:{port}", proto_name(k.proto))
                } else {
                    format!("{} {addr}:{port}", proto_name(k.proto))
                };
                SandboxHit {
                    vm,
                    cgroup,
                    target,
                    count,
                }
            })
            .collect();
        drop(sh);
        device_hits.sort_by_key(|h| std::cmp::Reverse(h.count));
        egress_hits.sort_by_key(|h| std::cmp::Reverse(h.count));
        VmSandboxStatus {
            config: self.sandbox.config.clone(),
            devices: self.sandbox.devices.clone(),
            attached: self.sandbox.attached.clone(),
            enforcing: self.sandbox.config.mode == "enforce" && self.lease_live(),
            device_hits,
            egress_hits,
            notes: self.sandbox.notes.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_rules_round_trip() {
        let r = parse_dev_rule("c 10:232 rw").unwrap();
        assert_eq!(
            (r.major, r.minor, r.dev_type, r.access),
            (10, 232, DEVCG_DEV_CHAR, DEVCG_ACC_READ | DEVCG_ACC_WRITE)
        );
        assert_eq!(dev_rule_string(&r), "c 10:232 rw");
        let r = parse_dev_rule("b 8:*").unwrap();
        assert_eq!(r.minor, DEV_MINOR_ANY);
        assert_eq!(dev_rule_string(&r), "b 8:* rwm");
        assert!(parse_dev_rule("x 1:1").is_err());
        assert!(parse_dev_rule("c 1:1 rq").is_err());
    }

    #[test]
    fn ports_and_identities() {
        let p = parse_ports(&["49152-49155".into(), "10809".into()]).unwrap();
        assert_eq!(p.len(), 5);
        assert!(parse_ports(&["0".into()]).is_err());
        assert!(parse_ports(&["2000-1000".into()]).is_err());
        let a = group_identity("web");
        assert_eq!(a, group_identity("web"));
        assert_ne!(a, group_identity("db"));
        assert!(group_identity("") >= 16);
    }
}
