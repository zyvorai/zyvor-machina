// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Uplink XDP: one dispatcher (`mn_xdp_uplink`) that runs the DDoS shield
//! and then tail-calls the NodePort fast path. XDP allows one program per
//! interface, so every uplink feature hangs off this entry point.

use aya_ebpf::{
    bindings::xdp_action,
    macros::{map, xdp},
    maps::{Array, ProgramArray},
    programs::XdpContext,
};
use machina_bpf_common::*;

use crate::{
    cni::pick_nodeport,
    maps::*,
    net::now_ns,
    parse::{ETH_HLEN, ETH_P_IP, ETH_P_IPV6, IPPROTO_TCP, IPPROTO_UDP, Tuple},
};

/// Tail-call slots of the uplink dispatcher.
#[map]
pub static XDP_PROGS: ProgramArray = ProgramArray::with_max_entries(XDP_SLOTS, 0);

#[map]
pub static XDP_CFG: Array<XdpCfg> = Array::with_max_entries(1, 0);

#[inline(always)]
pub(crate) fn ptr<T>(ctx: &XdpContext, off: usize) -> Option<*mut T> {
    let start = ctx.data();
    if start + off + core::mem::size_of::<T>() > ctx.data_end() {
        return None;
    }
    Some((start + off) as *mut T)
}

#[xdp]
pub fn mn_xdp_uplink(ctx: XdpContext) -> u32 {
    let Some(cfg) = XDP_CFG.get(0) else {
        return xdp_action::XDP_PASS;
    };
    let flags = cfg.flags;
    if flags & XDP_F_NODEISO != 0 && crate::nodeiso::nodeiso_xdp(&ctx) == xdp_action::XDP_DROP {
        return xdp_action::XDP_DROP;
    }
    if flags & XDP_F_SHIELD != 0 && crate::shield::shield(&ctx) == xdp_action::XDP_DROP {
        return xdp_action::XDP_DROP;
    }
    if flags & XDP_F_QUICLB != 0 {
        let _ = unsafe { XDP_PROGS.tail_call(&ctx, XDP_SLOT_QUICLB) };
    }
    if flags & XDP_F_NODEPORT != 0 {
        let _ = unsafe { XDP_PROGS.tail_call(&ctx, XDP_SLOT_NODEPORT) };
    }
    xdp_action::XDP_PASS
}

/// RFC 1624 incremental update of a ones'-complement checksum for one
/// 16-bit word (all values host-order numbers of big-endian fields).
#[inline(always)]
fn csum16(csum: u16, old: u16, new: u16) -> u16 {
    let mut s = (!csum as u32) + (!old as u32 & 0xffff) + new as u32;
    s = (s & 0xffff) + (s >> 16);
    s = (s & 0xffff) + (s >> 16);
    !(s as u16)
}

#[inline(always)]
fn be16(b: &[u8], i: usize) -> u16 {
    u16::from_be_bytes([b[i], b[i + 1]])
}

/// NodePort → local backend DNAT in XDP. Anything else (remote backends,
/// fragments, IPv4 options, IPv6 extension headers) passes to the tc path,
/// which shares the flow-pinning and conntrack maps.
#[xdp]
pub fn mn_xdp_nodeport(ctx: XdpContext) -> u32 {
    match try_nodeport(&ctx) {
        Some(()) | None => xdp_action::XDP_PASS,
    }
}

#[inline(always)]
fn try_nodeport(ctx: &XdpContext) -> Option<()> {
    let mut t = Tuple::zero();
    if parse(ctx, &mut t) == 0 {
        return None;
    }
    let n = CNI_NODE.get(0)?;
    let local = n.node_addr == t.dst || (n.node_addr6[0] != 0 && n.node_addr6 == t.dst);
    if !local {
        return None;
    }
    // Separate frames: the verifier caps a call chain's stack at 512 bytes.
    let mut be = Backend::default();
    if !pick_nodeport(&t, &mut be) || be.flags & BE_F_REMOTE != 0 {
        return None;
    }
    record_ct(&t, &be);
    dnat(ctx, &t, &be)
}

/// Plain IPv4 (no options, not fragmented) or IPv6 without extension
/// headers, TCP/UDP only. Fills addresses, ports and offsets of `t`.
/// Inline: packet pointers cannot be passed to BPF subprograms.
#[inline(always)]
pub(crate) fn parse(ctx: &XdpContext, t: &mut Tuple) -> u32 {
    let Some(et) = ptr::<[u8; 2]>(ctx, 12) else { return 0 };
    let l4 = match u16::from_be_bytes(unsafe { *et }) {
        ETH_P_IP => {
            let Some(h) = ptr::<[u8; 20]>(ctx, ETH_HLEN) else { return 0 };
            let h = unsafe { &*h };
            if h[0] != 0x45 || be16(h, 6) & 0x3fff != 0 {
                return 0;
            }
            t.proto = h[9];
            t.src = v4_mapped([h[12], h[13], h[14], h[15]]);
            t.dst = v4_mapped([h[16], h[17], h[18], h[19]]);
            ETH_HLEN + 20
        }
        ETH_P_IPV6 => {
            let Some(h) = ptr::<[u8; 40]>(ctx, ETH_HLEN) else { return 0 };
            let h = unsafe { &*h };
            t.v6 = true;
            t.proto = h[6];
            let mut i = 0;
            while i < ADDR_LEN {
                t.src[i] = h[8 + i];
                t.dst[i] = h[24 + i];
                i += 1;
            }
            ETH_HLEN + 40
        }
        _ => return 0,
    };
    if t.proto != IPPROTO_TCP && t.proto != IPPROTO_UDP {
        return 0;
    }
    let Some(ports) = ptr::<[u8; 4]>(ctx, l4) else { return 0 };
    let ports = unsafe { &*ports };
    t.sport = be16(ports, 0);
    t.dport = be16(ports, 2);
    t.l3_off = ETH_HLEN;
    t.l4_off = l4;
    1
}

#[inline(never)]
fn record_ct(t: &Tuple, be: &Backend) {
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
}

/// Rewrite destination address + port with incremental checksums.
#[inline(always)]
fn dnat(ctx: &XdpContext, t: &Tuple, be: &Backend) -> Option<()> {
    let l4 = t.l4_off;
    if l4 > ETH_HLEN + 40 {
        return None;
    }
    let csum_off = if t.proto == IPPROTO_TCP { l4 + 16 } else { l4 + 6 };
    let ip_csum_off = if t.v6 { None } else { Some(ETH_HLEN + 10) };
    let csum_p = ptr::<[u8; 2]>(ctx, csum_off)?;
    let mut l4c = u16::from_be_bytes(unsafe { *csum_p });
    let udp_nocsum = t.proto == IPPROTO_UDP && !t.v6 && l4c == 0;
    let (first, dst_off) = if t.v6 { (0, ETH_HLEN + 24) } else { (12, ETH_HLEN + 16) };
    let mut i = first;
    while i < ADDR_LEN {
        let (o, nw) = (be16(&t.dst, i), be16(&be.addr, i));
        l4c = csum16(l4c, o, nw);
        if let Some(off) = ip_csum_off {
            let p = ptr::<[u8; 2]>(ctx, off)?;
            unsafe { *p = csum16(u16::from_be_bytes(*p), o, nw).to_be_bytes() };
        }
        i += 2;
    }
    l4c = csum16(l4c, t.dport, u16::from_be_bytes(be.port));
    if !udp_nocsum {
        if t.proto == IPPROTO_UDP && l4c == 0 {
            l4c = 0xffff;
        }
        unsafe { *csum_p = l4c.to_be_bytes() };
    }
    if t.v6 {
        let p = ptr::<[u8; 16]>(ctx, dst_off)?;
        unsafe { *p = be.addr };
    } else {
        let p = ptr::<[u8; 4]>(ctx, dst_off)?;
        unsafe { *p = [be.addr[12], be.addr[13], be.addr[14], be.addr[15]] };
    }
    let pp = ptr::<[u8; 2]>(ctx, l4 + 2)?;
    unsafe { *pp = be.port };
    Some(())
}
