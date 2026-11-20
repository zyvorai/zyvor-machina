// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! QUIC connection-ID load balancer, tail-called from `mn_xdp_uplink` under
//! `XDP_F_QUICLB`. UDP to a configured VIP:port goes to the backend whose
//! server id is encoded in the destination CID (QUIC-LB plaintext layout);
//! client Initials and CIDs without a known server id use the service's
//! Maglev table on the 5-tuple. Delivery is L2 DSR or IPIP, both XDP_TX.
//! Anything else continues to the NodePort slot or passes.

use aya_ebpf::{
    bindings::xdp_action,
    helpers::generated::bpf_xdp_adjust_head,
    macros::{map, xdp},
    maps::{HashMap, PerCpuArray},
    programs::XdpContext,
};
use machina_bpf_common::*;

use crate::{
    cni::flow_hash,
    parse::{ETH_HLEN, IPPROTO_UDP, Tuple},
    xdp::{parse, ptr, XDP_CFG, XDP_PROGS},
};

#[map]
pub static QLB_SVCS: HashMap<QlbSvcKey, QlbSvc> = HashMap::with_max_entries(QLB_MAX_SVCS, 0);
#[map]
pub static QLB_BACKENDS: HashMap<QlbBeKey, QlbBackend> = HashMap::with_max_entries(QLB_MAX_SVCS * QLB_MAX_BACKENDS, 0);
#[map]
pub static QLB_SID: HashMap<QlbSidKey, u32> = HashMap::with_max_entries(QLB_MAX_SVCS * QLB_MAX_BACKENDS, 0);
#[map]
pub static QLB_MAGLEV: HashMap<MaglevKey, u32> = HashMap::with_max_entries(QLB_MAX_SVCS * MAGLEV_M, 0);
#[map]
pub static QLB_STATS: PerCpuArray<u64> = PerCpuArray::with_max_entries(QLB_MAX_SVCS * QLB_STAT_SLOTS, 0);

#[inline(always)]
fn stat(svc: u32, i: u32) {
    if let Some(p) = QLB_STATS.get_ptr_mut(svc * QLB_STAT_SLOTS + i) {
        unsafe { *p += 1 };
    }
}

#[xdp]
pub fn mn_xdp_quiclb(ctx: XdpContext) -> u32 {
    if let Some(action) = try_quiclb(&ctx) {
        return action;
    }
    if let Some(cfg) = XDP_CFG.get(0) {
        if cfg.flags & XDP_F_NODEPORT != 0 {
            let _ = unsafe { XDP_PROGS.tail_call(&ctx, XDP_SLOT_NODEPORT) };
        }
    }
    xdp_action::XDP_PASS
}

/// `None`: not a configured service, continue down the dispatcher.
#[inline(always)]
fn try_quiclb(ctx: &XdpContext) -> Option<u32> {
    let mut t = Tuple::zero();
    if parse(ctx, &mut t) == 0 || t.proto != IPPROTO_UDP {
        return None;
    }
    let key = QlbSvcKey { addr: t.dst, port: t.dport.to_be_bytes(), _pad: [0; 2] };
    let svc = *unsafe { QLB_SVCS.get(&key) }?;
    let p = t.l4_off + 8;
    if p > ETH_HLEN + 40 + 8 {
        return Some(xdp_action::XDP_PASS);
    }
    let idx = match cid(ctx, p, svc.cid_len) {
        Some(c) => match qlb_cid_sid(&c, svc.config_id)
            .and_then(|sid| unsafe { QLB_SID.get(&QlbSidKey { svc_id: svc.svc_id, sid, _pad: 0 }) }.copied())
        {
            Some(i) => {
                stat(svc.svc_id, QLB_STAT_CID);
                i
            }
            None => {
                stat(svc.svc_id, QLB_STAT_UNKNOWN_SID);
                maglev(&t, &svc)?
            }
        },
        None => {
            stat(svc.svc_id, QLB_STAT_INITIAL);
            maglev(&t, &svc)?
        }
    };
    let Some(be) = (unsafe { QLB_BACKENDS.get(&QlbBeKey { svc_id: svc.svc_id, idx }) }).copied() else {
        stat(svc.svc_id, QLB_STAT_ERR);
        return Some(xdp_action::XDP_PASS);
    };
    let ok = if svc.mode == QLB_MODE_IPIP && !t.v6 { encap(ctx, &svc, &be) } else { set_macs(ctx, &svc, &be) };
    if ok.is_some() {
        stat(svc.svc_id, QLB_STAT_TX);
        Some(xdp_action::XDP_TX)
    } else {
        stat(svc.svc_id, QLB_STAT_ERR);
        Some(xdp_action::XDP_PASS)
    }
}

/// First three DCID bytes of a routable packet: short headers, and long
/// headers other than Initial (whose DCID the client picked at random).
#[inline(always)]
fn cid(ctx: &XdpContext, p: usize, cid_len: u8) -> Option<[u8; 3]> {
    let b = unsafe { *ptr::<[u8; 4]>(ctx, p)? };
    if b[0] & 0x40 == 0 {
        return None;
    }
    if b[0] & 0x80 == 0 {
        return (cid_len >= 3).then_some([b[1], b[2], b[3]]);
    }
    if (b[0] >> 4) & 3 == 0 {
        return None;
    }
    let l = unsafe { *ptr::<[u8; 9]>(ctx, p)? };
    (l[5] >= 3).then_some([l[6], l[7], l[8]])
}

#[inline(always)]
fn maglev(t: &Tuple, svc: &QlbSvc) -> Option<u32> {
    stat(svc.svc_id, QLB_STAT_MAGLEV);
    if svc.backend_count <= 1 {
        return Some(0);
    }
    let slot = flow_hash(t) % MAGLEV_M;
    unsafe { QLB_MAGLEV.get(&MaglevKey { svc_id: svc.svc_id, slot }) }.copied()
}

#[inline(always)]
fn set_macs(ctx: &XdpContext, svc: &QlbSvc, be: &QlbBackend) -> Option<()> {
    let eth = ptr::<[u8; 12]>(ctx, 0)?;
    let e = unsafe { &mut *eth };
    let mut i = 0;
    while i < 6 {
        e[i] = be.mac[i];
        e[6 + i] = svc.src_mac[i];
        i += 1;
    }
    Some(())
}

/// Push an outer IPv4 header (proto 4) between Ethernet and the inner IP.
#[inline(always)]
fn encap(ctx: &XdpContext, svc: &QlbSvc, be: &QlbBackend) -> Option<()> {
    let inner = unsafe { *ptr::<[u8; 4]>(ctx, ETH_HLEN)? };
    let tot = u16::from_be_bytes([inner[2], inner[3]]).checked_add(20)?;
    if unsafe { bpf_xdp_adjust_head(ctx.ctx, -20) } != 0 {
        return None;
    }
    set_macs(ctx, svc, be)?;
    unsafe { *ptr::<[u8; 2]>(ctx, 12)? = [0x08, 0x00] };
    let ip = ptr::<[u8; 20]>(ctx, ETH_HLEN)?;
    let h = unsafe { &mut *ip };
    let t = tot.to_be_bytes();
    *h = [
        0x45, 0, t[0], t[1], 0, 0, 0, 0, 64, 4, 0, 0, svc.encap_src[0], svc.encap_src[1], svc.encap_src[2],
        svc.encap_src[3], be.addr[12], be.addr[13], be.addr[14], be.addr[15],
    ];
    let mut sum: u32 = 0;
    let mut j = 0;
    while j < 20 {
        sum += u16::from_be_bytes([h[j], h[j + 1]]) as u32;
        j += 2;
    }
    sum = (sum & 0xffff) + (sum >> 16);
    sum = (sum & 0xffff) + (sum >> 16);
    let c = (!(sum as u16)).to_be_bytes();
    h[10] = c[0];
    h[11] = c[1];
    Some(())
}
