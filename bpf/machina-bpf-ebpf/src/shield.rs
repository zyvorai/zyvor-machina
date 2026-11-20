// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! XDP DDoS shield (run inline from the uplink dispatcher): per-source,
//! per-class (SYN / UDP / ICMP / other) PPS token buckets for packets towards
//! protected destinations, plus deny/allow source CIDRs. Over-rate packets
//! are dropped only in enforce mode with the enforcement lease live;
//! otherwise they are counted as audited.

use aya_ebpf::{
    bindings::xdp_action,
    macros::map,
    maps::{lpm_trie::Key, Array, HashMap, LpmTrie, LruHashMap, PerCpuArray},
    programs::XdpContext,
};
use machina_bpf_common::*;

use crate::{
    net::{enforce_active, now_ns},
    parse::{ETH_HLEN, ETH_P_IP, ETH_P_IPV6, IPPROTO_TCP, IPPROTO_UDP},
};

const IPPROTO_ICMP: u8 = 1;
const IPPROTO_ICMPV6: u8 = 58;
const ETH_P_8021Q: u16 = 0x8100;
const ETH_P_8021AD: u16 = 0x88a8;
const NSEC: u64 = 1_000_000_000;

#[map]
pub static SHIELD_CFG: Array<ShieldCfg> = Array::with_max_entries(1, 0);

/// Protected destinations (IPv4-mapped); ignored with `protect_all`.
#[map]
pub static SHIELD_PROTECTED: HashMap<[u8; ADDR_LEN], u8> = HashMap::with_max_entries(8192, 0);

/// Source CIDRs (IPv4-mapped, prefix + 96 for IPv4).
#[map]
pub static SHIELD_DENY: LpmTrie<[u8; ADDR_LEN], u8> = LpmTrie::with_max_entries(8192, 0);

#[map]
pub static SHIELD_ALLOW: LpmTrie<[u8; ADDR_LEN], u8> = LpmTrie::with_max_entries(8192, 0);

#[map]
pub static SHIELD_SOURCES: LruHashMap<ShieldSrcKey, ShieldSrcState> = LruHashMap::with_max_entries(65536, 0);

#[map]
pub static SHIELD_STATS: PerCpuArray<ShieldStats> = PerCpuArray::with_max_entries(1, 0);

/// Verdict inputs from the packet (all on the stack, so the decision can
/// live in its own frame).
struct Pkt {
    src: [u8; ADDR_LEN],
    dst: [u8; ADDR_LEN],
    class: u8,
    malformed: bool,
}

#[inline(always)]
fn ptr<T>(ctx: &XdpContext, off: usize) -> Option<*const T> {
    let start = ctx.data();
    if start + off + core::mem::size_of::<T>() > ctx.data_end() {
        return None;
    }
    Some((start + off) as *const T)
}

#[inline(always)]
pub fn shield(ctx: &XdpContext) -> u32 {
    let mut p = Pkt { src: [0; ADDR_LEN], dst: [0; ADDR_LEN], class: SHIELD_CLASS_OTHER, malformed: false };
    if !parse(ctx, &mut p) {
        return xdp_action::XDP_PASS;
    }
    decide(&p, (ctx.data_end() - ctx.data()) as u64)
}

/// Ethernet (up to two VLAN tags) → IPv4/IPv6 → class. False = not IP.
#[inline(always)]
fn parse(ctx: &XdpContext, p: &mut Pkt) -> bool {
    let mut off = 12;
    let Some(et) = ptr::<[u8; 2]>(ctx, off) else { return false };
    let mut proto = u16::from_be_bytes(unsafe { *et });
    let mut i = 0;
    while i < 2 && (proto == ETH_P_8021Q || proto == ETH_P_8021AD) {
        off += 4;
        let Some(et) = ptr::<[u8; 2]>(ctx, off) else { return false };
        proto = u16::from_be_bytes(unsafe { *et });
        i += 1;
    }
    let l3 = off + 2;
    let (l4proto, l4) = match proto {
        ETH_P_IP => {
            let Some(h) = ptr::<[u8; 20]>(ctx, l3) else { return false };
            let h = unsafe { &*h };
            p.src = v4_mapped([h[12], h[13], h[14], h[15]]);
            p.dst = v4_mapped([h[16], h[17], h[18], h[19]]);
            let ihl = ((h[0] & 0x0f) as usize) * 4;
            if h[0] >> 4 != 4 || ihl < 20 {
                p.malformed = true;
                return true;
            }
            // Non-first fragments carry no L4 header.
            if u16::from_be_bytes([h[6], h[7]]) & 0x1fff != 0 {
                return true;
            }
            (h[9], l3 + ihl)
        }
        ETH_P_IPV6 => {
            let Some(h) = ptr::<[u8; 40]>(ctx, l3) else { return false };
            let h = unsafe { &*h };
            let mut j = 0;
            while j < ADDR_LEN {
                p.src[j] = h[8 + j];
                p.dst[j] = h[24 + j];
                j += 1;
            }
            (h[6], l3 + 40)
        }
        _ => return false,
    };
    match l4proto {
        IPPROTO_TCP => {
            if l4 > ETH_HLEN + 8 + 60 {
                p.malformed = true;
                return true;
            }
            match ptr::<[u8; 14]>(ctx, l4) {
                // SYN without ACK.
                Some(t) => {
                    if unsafe { (*t)[13] } & 0x12 == 0x02 {
                        p.class = SHIELD_CLASS_SYN;
                    }
                }
                None => p.malformed = true,
            }
        }
        IPPROTO_UDP => {
            if ptr::<[u8; 8]>(ctx, l4).is_none() {
                p.malformed = true;
            }
            p.class = SHIELD_CLASS_UDP;
        }
        IPPROTO_ICMP | IPPROTO_ICMPV6 => p.class = SHIELD_CLASS_ICMP,
        _ => {}
    }
    true
}

#[inline(always)]
fn stats() -> Option<&'static mut ShieldStats> {
    SHIELD_STATS.get_ptr_mut(0).map(|s| unsafe { &mut *s })
}

#[inline(never)]
fn decide(p: &Pkt, len: u64) -> u32 {
    let Some(cfg) = SHIELD_CFG.get(0) else { return xdp_action::XDP_PASS };
    if cfg.mode == SHIELD_OFF {
        return xdp_action::XDP_PASS;
    }
    if cfg.protect_all == 0 && unsafe { SHIELD_PROTECTED.get(&p.dst) }.is_none() {
        return xdp_action::XDP_PASS;
    }
    let Some(st) = stats() else { return xdp_action::XDP_PASS };
    st.checked += 1;
    let now = now_ns();
    let over = if p.malformed {
        st.malformed += 1;
        true
    } else if SHIELD_DENY.get(&Key::new(128, p.src)).is_some() {
        st.denied += 1;
        true
    } else if SHIELD_ALLOW.get(&Key::new(128, p.src)).is_some() {
        false
    } else {
        let class = (p.class as usize) & (SHIELD_CLASSES - 1);
        let rate = cfg.pps[class] as u64;
        if rate > 0 && !take(p, rate, cfg.burst_secs as u64, now) {
            st.limited[class] += 1;
            true
        } else {
            false
        }
    };
    if !over {
        st.passed += 1;
        return xdp_action::XDP_PASS;
    }
    if cfg.mode == SHIELD_ENFORCE && enforce_active(now) {
        st.dropped += 1;
        st.dropped_bytes += len;
        xdp_action::XDP_DROP
    } else {
        st.audited += 1;
        xdp_action::XDP_PASS
    }
}

/// One token from the source's class bucket (milli-tokens, depth
/// `rate * burst_secs`). False = over the rate; counted on the source.
#[inline(always)]
fn take(p: &Pkt, rate: u64, burst_secs: u64, now: u64) -> bool {
    let key = ShieldSrcKey { addr: p.src, class: p.class, _pad: [0; 3] };
    let cap = rate * burst_secs.max(1) * 1000;
    let Some(s) = SHIELD_SOURCES.get_ptr_mut(&key) else {
        let v = ShieldSrcState { tokens: cap.saturating_sub(1000), last_ns: now, hits: 0 };
        let _ = SHIELD_SOURCES.insert(&key, &v, 0);
        return true;
    };
    let s = unsafe { &mut *s };
    let elapsed = now.saturating_sub(s.last_ns).min(NSEC * 60);
    let tokens = (s.tokens + elapsed * rate / 1_000_000).min(cap);
    s.last_ns = now;
    if tokens < 1000 {
        s.tokens = tokens;
        s.hits += 1;
        false
    } else {
        s.tokens = tokens - 1000;
        true
    }
}
