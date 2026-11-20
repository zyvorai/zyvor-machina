// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! machina-cni datapath (dual-stack): pod-to-pod redirect, stateful
//! NetworkPolicy, NodePort (local DNAT, SNAT or IPIP DSR to remote
//! backends) and socket-level service load balancing with Maglev backend
//! selection and ClientIP session affinity.
//!
//! Every address is 16 bytes; IPv4 is carried IPv4-mapped (`::ffff:a.b.c.d`).

use aya_ebpf::{
    bindings::{BPF_F_ADJ_ROOM_ENCAP_L3_IPV4, bpf_adj_room_mode::BPF_ADJ_ROOM_MAC},
    helpers::generated::{bpf_get_netns_cookie, bpf_get_socket_cookie, bpf_redirect_neigh, bpf_redirect_peer},
    macros::{cgroup_sock_addr, classifier},
    maps::lpm_trie::Key,
    programs::{SockAddrContext, TcContext},
};
use machina_bpf_common::*;

use crate::{
    maps::*,
    net::{TC_ACT_SHOT, TC_ACT_UNSPEC, emit_net, now_ns},
    parse::*,
};

const BPF_F_PSEUDO_HDR: u64 = 0x10;
const BPF_NOEXIST: u64 = 1;
const IPPROTO_IPIP: u8 = 4;
const IPPROTO_ICMPV6: u8 = 58;
const TC_ACT_REDIRECT: i32 = 7;

#[inline(always)]
fn is_v4(a: &[u8; ADDR_LEN]) -> bool {
    a[10] == 0xff && a[11] == 0xff && a[0] == 0 && a[8] == 0
}

#[inline(always)]
fn is_node_addr(n: &NodeCfg, a: &[u8; ADDR_LEN]) -> bool {
    n.node_addr == *a || (n.node_addr6[0] != 0 && n.node_addr6 == *a)
}

/// NodePort frontends use the unspecified address of their family.
#[inline(always)]
fn nodeport_addr(v6: bool) -> [u8; ADDR_LEN] {
    if v6 { [0; ADDR_LEN] } else { v4_mapped([0; 4]) }
}

#[inline(always)]
fn node_addr_for(n: &NodeCfg, v6: bool) -> [u8; ADDR_LEN] {
    if v6 { n.node_addr6 } else { n.node_addr }
}

/// Link-local sources reach a pod veth only from its host side.
#[inline(always)]
fn identity_of(addr: &[u8; ADDR_LEN]) -> u32 {
    if addr[0] == 0xfe && addr[1] & 0xc0 == 0x80 {
        return IDENTITY_HOST;
    }
    if let Some(n) = CNI_NODE.get(0) {
        if is_node_addr(n, addr) {
            return IDENTITY_HOST;
        }
    }
    if let Some(&id) = unsafe { CNI_IDENTITIES.get(addr) } {
        return id;
    }
    match CNI_CIDR_IDS.get(&Key::new(128, *addr)) {
        Some(&id) => id,
        None => IDENTITY_WORLD,
    }
}

/// IPv6 neighbour discovery / link-local control traffic always passes
/// policy (the IPv6 equivalent of ARP).
#[inline(always)]
fn is_v6_control(t: &Tuple) -> bool {
    t.v6 && t.proto == IPPROTO_ICMPV6
        && ((t.src[0] == 0xfe && t.src[1] & 0xc0 == 0x80)
            || (t.dst[0] == 0xfe && t.dst[1] & 0xc0 == 0x80)
            || t.dst[0] == 0xff)
}

#[inline(always)]
fn pol(subject: u32, peer: u32, dir: u8, proto: u8, port: u16) -> bool {
    let k = PolicyKey {
        subject_identity: subject,
        peer_identity: peer,
        direction: dir,
        proto,
        port: port.to_be_bytes(),
    };
    unsafe { CNI_POLICY.get(&k) }.is_some()
}

#[inline(always)]
fn policy_allows(subject: u32, peer: u32, dir: u8, proto: u8, port: u16) -> bool {
    pol(subject, peer, dir, proto, port)
        || pol(subject, peer, dir, proto, 0)
        || pol(subject, 0, dir, proto, port)
        || pol(subject, 0, dir, proto, 0)
        || pol(subject, peer, dir, 0, 0)
        || pol(subject, 0, dir, 0, 0)
}

#[inline(always)]
fn ct_key(t: &Tuple, reverse: bool) -> FlowKey {
    if reverse {
        FlowKey {
            proto: t.proto,
            local_port: t.dport,
            remote_port: t.sport,
            local: t.dst,
            remote: t.src,
            ..FlowKey::default()
        }
    } else {
        FlowKey {
            proto: t.proto,
            local_port: t.sport,
            remote_port: t.dport,
            local: t.src,
            remote: t.dst,
            ..FlowKey::default()
        }
    }
}

#[inline(always)]
fn is_reply(t: &Tuple) -> bool {
    CNI_CT.get_ptr(&ct_key(t, true)).is_some()
}

#[inline(always)]
fn ct_track(t: &Tuple, now: u64) {
    let k = ct_key(t, false);
    match CNI_CT.get_ptr_mut(&k) {
        Some(p) => unsafe { *p = now },
        None => {
            let _ = CNI_CT.insert(&k, &now, 0);
        }
    }
}

#[inline(always)]
fn l4_csum_off(t: &Tuple) -> Option<usize> {
    match t.proto {
        IPPROTO_TCP => Some(t.l4_off + 16),
        IPPROTO_UDP => Some(t.l4_off + 6),
        _ => None,
    }
}

#[inline(always)]
fn word(a: &[u8; ADDR_LEN], i: usize) -> u64 {
    u32::from_ne_bytes([a[i], a[i + 1], a[i + 2], a[i + 3]]) as u64
}

/// Rewrite the source or destination address and L4 port, fixing the L3
/// (IPv4) and L4 checksums. Out of line to keep callers' stacks small.
#[inline(never)]
fn rewrite(ctx: &TcContext, t: &Tuple, dst: bool, addr: &[u8; ADDR_LEN], port: [u8; 2]) -> bool {
    let Some(csum_off) = l4_csum_off(t) else {
        return false;
    };
    let old: &[u8; ADDR_LEN] = if dst { &t.dst } else { &t.src };
    let old_port: [u8; 2] = if dst { t.dport.to_be_bytes() } else { t.sport.to_be_bytes() };
    let port_off = t.l4_off + if dst { 2 } else { 0 };
    let udp_nocsum = !t.v6 && t.proto == IPPROTO_UDP && matches!(ctx.load::<[u8; 2]>(csum_off), Ok([0, 0]));

    if t.v6 {
        let mut i = 0;
        while i < ADDR_LEN {
            if ctx
                .l4_csum_replace(csum_off, word(old, i), word(addr, i), BPF_F_PSEUDO_HDR | 4)
                .is_err()
            {
                return false;
            }
            i += 4;
        }
        let ip_off = t.l3_off + if dst { 24 } else { 8 };
        if ctx.store(ip_off, addr, 0).is_err() {
            return false;
        }
    } else {
        let (from_ip, to_ip) = (word(old, 12), word(addr, 12));
        if !udp_nocsum && ctx.l4_csum_replace(csum_off, from_ip, to_ip, BPF_F_PSEUDO_HDR | 4).is_err() {
            return false;
        }
        if ctx.l3_csum_replace(t.l3_off + 10, from_ip, to_ip, 4).is_err() {
            return false;
        }
        let v4: [u8; 4] = [addr[12], addr[13], addr[14], addr[15]];
        if ctx.store(t.l3_off + if dst { 16 } else { 12 }, &v4, 0).is_err() {
            return false;
        }
    }
    if !udp_nocsum {
        let (fp, tp) = (u16::from_ne_bytes(old_port) as u64, u16::from_ne_bytes(port) as u64);
        if ctx.l4_csum_replace(csum_off, fp, tp, 2).is_err() {
            return false;
        }
    }
    ctx.store(port_off, &port, 0).is_ok()
}

#[inline(always)]
fn deny(now: u64, t: &Tuple, len: u32) -> i32 {
    let key = ct_key(t, false);
    emit_net(now, NET_EV_DENY, VERDICT_DROP, 0, len, &key, 0, 0);
    TC_ACT_SHOT
}

// ---- backend selection -------------------------------------------------------

#[inline(always)]
fn mix(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(0x85eb_ca6b).rotate_left(13)
}

/// 5-tuple hash, identical on every node so DSR / ECMP stay consistent.
#[inline(always)]
pub fn flow_hash(t: &Tuple) -> u32 {
    let mut h: u32 = 0x9e37_79b9 ^ t.proto as u32;
    let mut i = 0;
    while i < ADDR_LEN {
        h = mix(h, word(&t.src, i) as u32);
        h = mix(h, word(&t.dst, i) as u32);
        i += 4;
    }
    h = mix(h, ((t.sport as u32) << 16) | t.dport as u32);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^ (h >> 15)
}

/// Backend index for `svc`: sticky ClientIP affinity first, then the
/// service's Maglev table (single-backend services skip the lookup).
#[inline(never)]
pub fn select_backend(svc: &SvcVal, client: &[u8; ADDR_LEN], hash: u32, out: &mut Backend) -> bool {
    let n = svc.backend_count;
    if n == 0 {
        return false;
    }
    let now = now_ns();
    let akey = AffinityKey { client: *client, svc_id: svc.svc_id };
    let mut idx = u32::MAX;
    if svc.flags & SVC_F_AFFINITY != 0 {
        if let Some(a) = CNI_AFFINITY.get_ptr_mut(&akey) {
            let ttl = svc.affinity_secs as u64 * 1_000_000_000;
            unsafe {
                if (*a).index < n && now.wrapping_sub((*a).last_ns) < ttl {
                    (*a).last_ns = now;
                    idx = (*a).index;
                }
            }
        }
    }
    if idx == u32::MAX {
        idx = if n == 1 {
            0
        } else {
            let mk = MaglevKey { svc_id: svc.svc_id, slot: hash % MAGLEV_M };
            match unsafe { CNI_MAGLEV.get(&mk) } {
                Some(&i) if i < n => i,
                _ => hash % n,
            }
        };
        if svc.flags & SVC_F_AFFINITY != 0 {
            let v = AffinityVal { index: idx, _pad: 0, last_ns: now };
            let _ = CNI_AFFINITY.insert(&akey, &v, 0);
        }
    }
    match unsafe { CNI_BACKENDS.get(&BackendKey { svc_id: svc.svc_id, index: idx }) } {
        Some(b) => {
            *out = *b;
            true
        }
        None => false,
    }
}

// ---- pod veth ----------------------------------------------------------------

/// TC ingress on a pod's host-side veth (traffic leaving the pod).
#[classifier]
pub fn mn_cni_from_pod(ctx: TcContext) -> i32 {
    let mut t = Tuple::zero();
    if parse_tc(&ctx, &mut t) == 0 || is_v6_control(&t) {
        return TC_ACT_UNSPEC;
    }
    let now = now_ns();
    let len = ctx.len();
    let Some(src_ep) = (unsafe { CNI_ENDPOINTS.get(&t.src) }).copied() else {
        return TC_ACT_UNSPEC;
    };

    // NodePort reply (local DNAT or DSR): restore the frontend as source.
    if t.proto == IPPROTO_TCP || t.proto == IPPROTO_UDP {
        let rk = NatCtKey {
            backend_addr: t.src,
            client_addr: t.dst,
            backend_port: t.sport.to_be_bytes(),
            client_port: t.dport.to_be_bytes(),
            proto: t.proto,
            _pad: [0; 3],
        };
        if let Some(ct) = CNI_NODEPORT_CT.get_ptr_mut(&rk) {
            let (fa, fp) = unsafe {
                (*ct).last_ns = now;
                ((*ct).frontend_addr, (*ct).frontend_port)
            };
            if !rewrite(&ctx, &t, false, &fa, fp) {
                return TC_ACT_SHOT;
            }
            return TC_ACT_UNSPEC;
        }
    }

    let reply = is_reply(&t);
    // Plain locals initialised on both arms: a copied `Option<Endpoint>` lets
    // LLVM spill the undefined `None` payload, which the verifier rejects.
    let (local, dst_id, dst_flags, dst_ifindex, pod_mac, host_mac) = match unsafe { CNI_ENDPOINTS.get(&t.dst) } {
        Some(ep) => (true, ep.identity, ep.flags, ep.host_ifindex, ep.pod_mac, ep.host_mac),
        None => (false, identity_of(&t.dst), 0, 0, [0u8; 6], [0u8; 6]),
    };
    if !reply {
        if src_ep.flags & EP_EGRESS_ISOLATED != 0
            && !policy_allows(src_ep.identity, dst_id, POLICY_EGRESS, t.proto, t.dport)
        {
            return deny(now, &t, len);
        }
        if local
            && dst_flags & EP_INGRESS_ISOLATED != 0
            && !policy_allows(dst_id, src_ep.identity, POLICY_INGRESS, t.proto, t.dport)
        {
            return deny(now, &t, len);
        }
        ct_track(&t, now);
    }

    if !local {
        return TC_ACT_UNSPEC;
    }
    if ctx.store(0, &pod_mac, 0).is_err() || ctx.store(6, &host_mac, 0).is_err() {
        return TC_ACT_UNSPEC;
    }
    unsafe { bpf_redirect_peer(dst_ifindex, 0) as i32 }
}

/// TC egress on a pod's host-side veth (traffic entering the pod via the
/// kernel stack: other nodes, host, NodePort).
#[classifier]
pub fn mn_cni_to_pod(ctx: TcContext) -> i32 {
    let mut t = Tuple::zero();
    if parse_tc(&ctx, &mut t) == 0 || is_v6_control(&t) {
        return TC_ACT_UNSPEC;
    }
    let Some(ep) = (unsafe { CNI_ENDPOINTS.get(&t.dst) }).copied() else {
        return TC_ACT_UNSPEC;
    };
    let now = now_ns();
    if is_reply(&t) {
        return TC_ACT_UNSPEC;
    }
    if ep.flags & EP_INGRESS_ISOLATED != 0 {
        let src_id = identity_of(&t.src);
        if src_id != IDENTITY_HOST && !policy_allows(ep.identity, src_id, POLICY_INGRESS, t.proto, t.dport) {
            return deny(now, &t, ctx.len());
        }
    }
    ct_track(&t, now);
    TC_ACT_UNSPEC
}

// ---- NodePort (uplink ingress) -------------------------------------------------

#[inline(always)]
fn redirect_uplink(n: &NodeCfg) -> i32 {
    if n.uplink_ifindex == 0 {
        return TC_ACT_UNSPEC;
    }
    let r = unsafe { bpf_redirect_neigh(n.uplink_ifindex, core::ptr::null_mut(), 0, 0) } as i32;
    if r == TC_ACT_REDIRECT { r } else { TC_ACT_UNSPEC }
}

#[inline(always)]
fn ipv4_csum(h: &[u8; 20]) -> u16 {
    let mut s: u32 = 0;
    let mut i = 0;
    while i < 20 {
        s += u16::from_be_bytes([h[i], h[i + 1]]) as u32;
        i += 2;
    }
    s = (s & 0xffff) + (s >> 16);
    s = (s & 0xffff) + (s >> 16);
    !(s as u16)
}

/// DSR: push an outer IPv4 header (node → backend's node). The outer IP ID
/// carries the NodePort so the remote node can restore the frontend.
#[inline(never)]
fn dsr_encap(ctx: &TcContext, n: &NodeCfg, be: &Backend, frontend_port: u16) -> i32 {
    if ctx
        .adjust_room(20, BPF_ADJ_ROOM_MAC, BPF_F_ADJ_ROOM_ENCAP_L3_IPV4 as u64)
        .is_err()
    {
        return TC_ACT_SHOT;
    }
    let tot = (ctx.len() as u16).wrapping_sub(ETH_HLEN as u16).to_be_bytes();
    let id = frontend_port.to_be_bytes();
    let mut h: [u8; 20] = [
        0x45, 0, tot[0], tot[1], id[0], id[1], 0, 0, 64, IPPROTO_IPIP, 0, 0, n.node_addr[12], n.node_addr[13],
        n.node_addr[14], n.node_addr[15], be.node[12], be.node[13], be.node[14], be.node[15],
    ];
    let c = ipv4_csum(&h).to_be_bytes();
    h[10] = c[0];
    h[11] = c[1];
    if ctx.store(ETH_HLEN, &h, 0).is_err() {
        return TC_ACT_SHOT;
    }
    redirect_uplink(n)
}

/// DSR receive side: strip the outer header of IPIP aimed at this node and
/// remember the frontend so the backend's reply leaves with it as source.
#[inline(never)]
fn dsr_decap(ctx: &TcContext, t: &Tuple) -> i32 {
    let (Ok(outer_id), Ok(outer_src)) = (ctx.load::<[u8; 2]>(t.l3_off + 4), ctx.load::<[u8; 4]>(t.l3_off + 12)) else {
        return TC_ACT_UNSPEC;
    };
    let ihl = match ctx.load::<u8>(t.l3_off) {
        Ok(v) if v == 0x45 => 20,
        _ => return TC_ACT_UNSPEC,
    };
    let mut inner = Tuple::zero();
    if parse_inner_v4(ctx, t.l3_off + ihl, &mut inner) == 0 {
        return TC_ACT_UNSPEC;
    }
    if unsafe { CNI_ENDPOINTS.get(&inner.dst) }.is_none() {
        return TC_ACT_UNSPEC;
    }
    if ctx.adjust_room(-20, BPF_ADJ_ROOM_MAC, 0).is_err() {
        return TC_ACT_SHOT;
    }
    let rk = NatCtKey {
        backend_addr: inner.dst,
        client_addr: inner.src,
        backend_port: inner.dport.to_be_bytes(),
        client_port: inner.sport.to_be_bytes(),
        proto: inner.proto,
        _pad: [0; 3],
    };
    let v = NatCtVal {
        frontend_addr: v4_mapped(outer_src),
        frontend_port: outer_id,
        flags: 0,
        _pad: 0,
        client_addr: [0; ADDR_LEN],
        last_ns: now_ns(),
    };
    let _ = CNI_NODEPORT_CT.insert(&rk, &v, 0);
    TC_ACT_UNSPEC
}

/// Reply from a remote backend to an SNATed NodePort flow: restore client
/// and frontend, then send it back out of the uplink.
#[inline(never)]
fn snat_reverse(ctx: &TcContext, t: &Tuple, n: &NodeCfg) -> i32 {
    let rk = NatCtKey {
        backend_addr: t.src,
        client_addr: t.dst,
        backend_port: t.sport.to_be_bytes(),
        client_port: t.dport.to_be_bytes(),
        proto: t.proto,
        _pad: [0; 3],
    };
    let Some(ct) = CNI_NODEPORT_CT.get_ptr_mut(&rk) else {
        return -2;
    };
    let (flags, fa, fp, client) = unsafe {
        (*ct).last_ns = now_ns();
        ((*ct).flags, (*ct).frontend_addr, (*ct).frontend_port, (*ct).client_addr)
    };
    if flags & NAT_F_SNAT == 0 {
        return -2;
    }
    if !rewrite(ctx, t, true, &client, t.dport.to_be_bytes()) || !rewrite(ctx, t, false, &fa, fp) {
        return TC_ACT_SHOT;
    }
    redirect_uplink(n)
}

/// Forward a NodePort flow to a backend on another node through this node's
/// address (eTP=Cluster, SNAT mode).
#[inline(never)]
fn snat_forward(ctx: &TcContext, t: &Tuple, n: &NodeCfg, be: &Backend) -> i32 {
    let node = node_addr_for(n, t.v6);
    let rk = NatCtKey {
        backend_addr: be.addr,
        client_addr: node,
        backend_port: be.port,
        client_port: t.sport.to_be_bytes(),
        proto: t.proto,
        _pad: [0; 3],
    };
    let v = NatCtVal {
        frontend_addr: t.dst,
        frontend_port: t.dport.to_be_bytes(),
        flags: NAT_F_SNAT,
        _pad: 0,
        client_addr: t.src,
        last_ns: now_ns(),
    };
    if CNI_NODEPORT_CT.insert(&rk, &v, BPF_NOEXIST).is_err() {
        match CNI_NODEPORT_CT.get_ptr_mut(&rk) {
            // Another client already owns this (backend, source port) pair.
            Some(p) if unsafe { (*p).client_addr } != t.src => return TC_ACT_UNSPEC,
            Some(p) => unsafe { (*p).last_ns = v.last_ns },
            None => return TC_ACT_UNSPEC,
        }
    }
    if !rewrite(ctx, t, true, &be.addr, be.port) || !rewrite(ctx, t, false, &node, t.sport.to_be_bytes()) {
        return TC_ACT_SHOT;
    }
    redirect_uplink(n)
}

/// TC ingress on the node uplink: NodePort → backend, SNAT reverse path and
/// DSR decapsulation.
#[classifier]
pub fn mn_cni_nodeport(ctx: TcContext) -> i32 {
    let mut t = Tuple::zero();
    if parse_tc(&ctx, &mut t) == 0 {
        return TC_ACT_UNSPEC;
    }
    let Some(n) = CNI_NODE.get(0) else {
        return TC_ACT_UNSPEC;
    };
    if !is_node_addr(n, &t.dst) {
        return TC_ACT_UNSPEC;
    }
    if t.proto == IPPROTO_IPIP && !t.v6 {
        return dsr_decap(&ctx, &t);
    }
    if t.proto != IPPROTO_TCP && t.proto != IPPROTO_UDP {
        return TC_ACT_UNSPEC;
    }
    let r = snat_reverse(&ctx, &t, n);
    if r != -2 {
        return r;
    }
    // Each step is its own frame: the verifier caps the combined stack of
    // a call chain at 512 bytes.
    let mut be = Backend::default();
    if !pick_nodeport(&t, &mut be) {
        return TC_ACT_UNSPEC;
    }
    if be.flags & BE_F_REMOTE != 0 {
        if n.flags & NODE_F_DSR != 0 && !t.v6 {
            let fp = t.dport;
            if !rewrite(&ctx, &t, true, &be.addr, be.port) {
                return TC_ACT_SHOT;
            }
            return dsr_encap(&ctx, n, &be, fp);
        }
        return snat_forward(&ctx, &t, n, &be);
    }
    local_dnat(&ctx, &t, &be)
}

/// NodePort service lookup + backend pick, pinned per flow so every packet
/// of a connection (and the XDP fast path) agrees on the backend.
#[inline(never)]
pub fn pick_nodeport(t: &Tuple, be: &mut Backend) -> bool {
    let svc_key = SvcKey { addr: nodeport_addr(t.v6), port: t.dport.to_be_bytes(), proto: t.proto, _pad: 0 };
    let Some(svc) = (unsafe { CNI_NODEPORTS.get(&svc_key) }) else {
        return false;
    };
    let fwd_key = NatCtKey {
        backend_addr: t.dst,
        client_addr: t.src,
        backend_port: t.dport.to_be_bytes(),
        client_port: t.sport.to_be_bytes(),
        proto: t.proto,
        _pad: [0; 3],
    };
    if let Some(b) = unsafe { CNI_NODEPORT_FWD.get(&fwd_key) } {
        *be = *b;
        return true;
    }
    if !select_backend(svc, &t.src, flow_hash(t), be) {
        return false;
    }
    let _ = CNI_NODEPORT_FWD.insert(&fwd_key, be, 0);
    true
}

/// NodePort → backend on this node: DNAT + reverse-NAT entry for the reply.
#[inline(never)]
fn local_dnat(ctx: &TcContext, t: &Tuple, be: &Backend) -> i32 {
    let rev = NatCtKey {
        backend_addr: be.addr,
        client_addr: t.src,
        backend_port: be.port,
        client_port: t.sport.to_be_bytes(),
        proto: t.proto,
        _pad: [0; 3],
    };
    let val = NatCtVal {
        frontend_addr: t.dst,
        frontend_port: t.dport.to_be_bytes(),
        flags: 0,
        _pad: 0,
        client_addr: [0; ADDR_LEN],
        last_ns: now_ns(),
    };
    let _ = CNI_NODEPORT_CT.insert(&rev, &val, 0);
    if !rewrite(ctx, t, true, &be.addr, be.port) {
        return TC_ACT_SHOT;
    }
    TC_ACT_UNSPEC
}

// ---- socket LB --------------------------------------------------------------

/// Service lookup + backend pick for a socket destination. The client key
/// is the socket's network namespace (one per pod), which is what
/// ClientIP affinity means for in-cluster callers.
#[inline(always)]
fn pick_backend(ctx: &SockAddrContext, addr: [u8; ADDR_LEN], port: [u8; 2], out: &mut Backend) -> bool {
    let proto = unsafe { (*ctx.sock_addr).protocol } as u8;
    let key = SvcKey { addr, port, proto, _pad: 0 };
    let svc = match unsafe { CNI_SERVICES.get(&key) } {
        Some(s) => *s,
        None => {
            let Some(node) = CNI_NODE.get(0) else {
                return false;
            };
            if !is_node_addr(node, &addr) {
                return false;
            }
            let np = SvcKey { addr: nodeport_addr(!is_v4(&addr)), ..key };
            match unsafe { CNI_NODEPORTS.get(&np) } {
                Some(s) => *s,
                None => return false,
            }
        }
    };
    let netns = unsafe { bpf_get_netns_cookie(ctx.sock_addr.cast()) };
    let cookie = unsafe { bpf_get_socket_cookie(ctx.sock_addr.cast()) };
    let mut client = [0u8; ADDR_LEN];
    let nb = netns.to_ne_bytes();
    let mut i = 0;
    while i < 8 {
        client[i] = nb[i];
        i += 1;
    }
    let h = (cookie as u32) ^ ((cookie >> 32) as u32).wrapping_mul(0x9e37_79b9);
    select_backend(&svc, &client, h, out)
}

#[inline(always)]
fn sock_dst4(ctx: &SockAddrContext) -> ([u8; ADDR_LEN], [u8; 2]) {
    unsafe {
        (
            v4_mapped((*ctx.sock_addr).user_ip4.to_ne_bytes()),
            ((*ctx.sock_addr).user_port as u16).to_ne_bytes(),
        )
    }
}

#[inline(always)]
fn sock_dst6(ctx: &SockAddrContext) -> ([u8; ADDR_LEN], [u8; 2]) {
    let mut a = [0u8; ADDR_LEN];
    unsafe {
        let w = (*ctx.sock_addr).user_ip6;
        let mut i = 0;
        while i < 4 {
            let b = w[i].to_ne_bytes();
            a[i * 4] = b[0];
            a[i * 4 + 1] = b[1];
            a[i * 4 + 2] = b[2];
            a[i * 4 + 3] = b[3];
            i += 1;
        }
        (a, ((*ctx.sock_addr).user_port as u16).to_ne_bytes())
    }
}

#[inline(always)]
fn set_dest4(ctx: &SockAddrContext, addr: &[u8; ADDR_LEN], port: [u8; 2]) {
    unsafe {
        (*ctx.sock_addr).user_ip4 = u32::from_ne_bytes([addr[12], addr[13], addr[14], addr[15]]);
        (*ctx.sock_addr).user_port = u16::from_ne_bytes(port) as u32;
    }
}

#[inline(always)]
fn set_dest6(ctx: &SockAddrContext, addr: &[u8; ADDR_LEN], port: [u8; 2]) {
    unsafe {
        let mut i = 0;
        while i < 4 {
            (*ctx.sock_addr).user_ip6[i] =
                u32::from_ne_bytes([addr[i * 4], addr[i * 4 + 1], addr[i * 4 + 2], addr[i * 4 + 3]]);
            i += 1;
        }
        (*ctx.sock_addr).user_port = u16::from_ne_bytes(port) as u32;
    }
}

/// connect / sendmsg: translate a service frontend into a backend. For UDP
/// remember the frontend so recvmsg can restore it.
#[inline(always)]
fn sock_translate(ctx: &SockAddrContext, v6: bool, udp: bool) -> i32 {
    let (addr, port) = if v6 { sock_dst6(ctx) } else { sock_dst4(ctx) };
    let mut be = Backend::default();
    if !pick_backend(ctx, addr, port, &mut be) {
        return 1;
    }
    // An IPv6 socket may only be steered to an address of its own family form.
    if !v6 && !is_v4(&be.addr) {
        return 1;
    }
    if udp {
        let cookie = unsafe { bpf_get_socket_cookie(ctx.sock_addr.cast()) };
        let k = RevNatKey { cookie, addr: be.addr, port: be.port, _pad: [0; 6] };
        let frontend = Backend { addr, port, ..Backend::default() };
        let _ = CNI_UDP_REVNAT.insert(&k, &frontend, BPF_NOEXIST);
    }
    if v6 {
        set_dest6(ctx, &be.addr, be.port);
    } else {
        set_dest4(ctx, &be.addr, be.port);
    }
    1
}

#[inline(always)]
fn sock_restore(ctx: &SockAddrContext, v6: bool) -> i32 {
    let (addr, port) = if v6 { sock_dst6(ctx) } else { sock_dst4(ctx) };
    let cookie = unsafe { bpf_get_socket_cookie(ctx.sock_addr.cast()) };
    let k = RevNatKey { cookie, addr, port, _pad: [0; 6] };
    if let Some(fe) = unsafe { CNI_UDP_REVNAT.get(&k) } {
        let (a, p) = (fe.addr, fe.port);
        if v6 {
            set_dest6(ctx, &a, p);
        } else {
            set_dest4(ctx, &a, p);
        }
    }
    1
}

#[cgroup_sock_addr(connect4)]
pub fn mn_cni_connect4(ctx: SockAddrContext) -> i32 {
    sock_translate(&ctx, false, false)
}

#[cgroup_sock_addr(sendmsg4)]
pub fn mn_cni_sendmsg4(ctx: SockAddrContext) -> i32 {
    sock_translate(&ctx, false, true)
}

#[cgroup_sock_addr(recvmsg4)]
pub fn mn_cni_recvmsg4(ctx: SockAddrContext) -> i32 {
    sock_restore(&ctx, false)
}

#[cgroup_sock_addr(connect6)]
pub fn mn_cni_connect6(ctx: SockAddrContext) -> i32 {
    sock_translate(&ctx, true, false)
}

#[cgroup_sock_addr(sendmsg6)]
pub fn mn_cni_sendmsg6(ctx: SockAddrContext) -> i32 {
    sock_translate(&ctx, true, true)
}

#[cgroup_sock_addr(recvmsg6)]
pub fn mn_cni_recvmsg6(ctx: SockAddrContext) -> i32 {
    sock_restore(&ctx, true)
}
