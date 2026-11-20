// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Node isolation: emergency drop-all on the uplink with an allowlist.
//! Ingress runs inline from the XDP uplink dispatcher (ahead of the shield
//! and the NodePort fast path); egress is `mn_nodeiso` on TCX, first in the
//! chain. Non-IP frames, IPv6 neighbour discovery and DHCP always pass;
//! everything else passes only for exempt peers or allowlisted ports (local
//! or remote, so both server and client sessions survive). Drops need the
//! node-isolation lease (`NodeIsoCfg::deadline_ns`) to be live.

use aya_ebpf::{
    bindings::xdp_action,
    macros::{classifier, map},
    maps::{lpm_trie::Key, Array, HashMap, LpmTrie, PerCpuArray},
    programs::{TcContext, XdpContext},
};
use machina_bpf_common::*;

use crate::{
    net::{now_ns, TC_ACT_SHOT, TC_ACT_UNSPEC},
    parse::{parse_tc, parse_xdp, Pkt, Tuple, IPPROTO_TCP, IPPROTO_UDP},
};

const IPPROTO_ICMP: u8 = 1;
const IPPROTO_ICMPV6: u8 = 58;

#[map]
pub static NODEISO_CFG: Array<NodeIsoCfg> = Array::with_max_entries(1, 0);

/// Allowlisted ports, keyed by [`nodeiso_port_key`].
#[map]
pub static NODEISO_PORTS: HashMap<u32, u8> = HashMap::with_max_entries(256, 0);

/// Exempt peer CIDRs (IPv4-mapped, prefix + 96 for IPv4).
#[map]
pub static NODEISO_EXEMPT: LpmTrie<[u8; ADDR_LEN], u8> = LpmTrie::with_max_entries(1024, 0);

#[map]
pub static NODEISO_STATS: PerCpuArray<NodeIsoStats> = PerCpuArray::with_max_entries(1, 0);

/// True when the packet must be dropped. `icmp_type` is the first L4 byte.
#[inline(never)]
fn verdict(t: &Tuple, icmp_type: u8, ingress: bool) -> bool {
    let Some(cfg) = NODEISO_CFG.get(0) else { return false };
    if cfg.enabled == 0 {
        return false;
    }
    let Some(st) = NODEISO_STATS.get_ptr_mut(0) else { return false };
    let st = unsafe { &mut *st };
    st.checked += 1;
    let (peer, lport, rport) = if ingress { (&t.src, t.dport, t.sport) } else { (&t.dst, t.sport, t.dport) };
    let allowed = match t.proto {
        // Router/neighbour solicitation and advertisement, redirect.
        IPPROTO_ICMPV6 if (133..=137).contains(&icmp_type) => true,
        IPPROTO_ICMP | IPPROTO_ICMPV6 => cfg.allow_icmp != 0,
        // DHCPv4 / DHCPv6 keep the address lease.
        IPPROTO_UDP if matches!(lport, 67 | 68 | 546 | 547) => true,
        IPPROTO_TCP | IPPROTO_UDP => {
            unsafe { NODEISO_PORTS.get(&nodeiso_port_key(t.proto, lport)) }.is_some()
                || unsafe { NODEISO_PORTS.get(&nodeiso_port_key(t.proto, rport)) }.is_some()
        }
        _ => false,
    } || NODEISO_EXEMPT.get(&Key::new(128, *peer)).is_some();
    if allowed {
        st.passed += 1;
        return false;
    }
    if cfg.dry_run != 0 || now_ns() >= cfg.deadline_ns {
        st.would_drop += 1;
        return false;
    }
    if ingress {
        st.dropped_in += 1;
    } else {
        st.dropped_out += 1;
    }
    true
}

#[inline(always)]
fn icmp_type<P: Pkt>(p: &P, t: &Tuple) -> u8 {
    if t.proto == IPPROTO_ICMP || t.proto == IPPROTO_ICMPV6 {
        p.ld::<u8>(t.l4_off).unwrap_or(0)
    } else {
        0
    }
}

/// Ingress check from `mn_xdp_uplink`.
#[inline(always)]
pub fn nodeiso_xdp(ctx: &XdpContext) -> u32 {
    let mut t = Tuple::zero();
    if parse_xdp(ctx, &mut t) == 0 {
        return xdp_action::XDP_PASS;
    }
    let ty = icmp_type(ctx, &t);
    if verdict(&t, ty, true) {
        xdp_action::XDP_DROP
    } else {
        xdp_action::XDP_PASS
    }
}

#[classifier]
pub fn mn_nodeiso(ctx: TcContext) -> i32 {
    let mut t = Tuple::zero();
    if parse_tc(&ctx, &mut t) == 0 {
        return TC_ACT_UNSPEC;
    }
    let ty = icmp_type(&ctx, &t);
    if verdict(&t, ty, false) {
        TC_ACT_SHOT
    } else {
        TC_ACT_UNSPEC
    }
}
