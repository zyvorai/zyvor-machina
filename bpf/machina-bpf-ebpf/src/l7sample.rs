// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Sampled plaintext application protocols (Redis, PostgreSQL, MySQL, Kafka,
//! HTTP/2 + gRPC) on a cgroup's sockets. Observe only: always allows the
//! packet. For a payload-bearing TCP segment to or from a configured service
//! port it copies at most `L7S_COPY` bytes to userspace, which keeps only the
//! operation name; one sample per flow and direction per `flow_gap_ns`.

use aya_ebpf::{
    helpers::generated::{bpf_skb_cgroup_id, bpf_skb_load_bytes},
    macros::{cgroup_skb, map},
    maps::{Array, HashMap, LruHashMap, PerCpuArray, RingBuf},
    programs::SkBuffContext,
};
use machina_bpf_common::*;

use crate::{
    net::now_ns,
    parse::{parse_skb, Tuple, IPPROTO_TCP},
    tls::take,
};

#[map]
pub static L7S_CFG: Array<L7sCfg> = Array::with_max_entries(1, 0);
/// Service port (host order) → L7S_* protocol id.
#[map]
pub static L7S_PORTS: HashMap<u16, u8> = HashMap::with_max_entries(64, 0);
/// Directional flow hash → last sample time.
#[map]
pub static L7S_RATE: LruHashMap<u64, u64> = LruHashMap::with_max_entries(32768, 0);
#[map]
pub static L7S_BUCKET: Array<SampleBucket> = Array::with_max_entries(1, 0);
#[map]
pub static L7S_STATS: PerCpuArray<u64> = PerCpuArray::with_max_entries(L7S_STAT_SLOTS, 0);
#[map]
pub static L7S_EVENTS: RingBuf = RingBuf::with_byte_size(1 << 20, 0);

#[inline(always)]
fn stat(idx: u32) {
    if let Some(p) = L7S_STATS.get_ptr_mut(idx) {
        unsafe { *p += 1 };
    }
}

#[inline(always)]
fn flow_hash(t: &Tuple, ingress: bool) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for i in 0..ADDR_LEN {
        h = (h ^ t.src[i] as u64).wrapping_mul(0x100_0000_01b3);
        h = (h ^ t.dst[i] as u64).wrapping_mul(0x100_0000_01b3);
    }
    h ^= (t.sport as u64) << 16 | t.dport as u64;
    h ^ (ingress as u64) << 63
}

/// `meta`: proto | to_server << 8 | ingress << 16 (BPF calls take at most
/// five arguments).
#[inline(never)]
fn emit(ctx: &SkBuffContext, t: &Tuple, meta: u32, avail: usize) {
    let (proto, to_server, ingress) = (meta as u8, (meta >> 8) as u8, (meta >> 16) as u8);
    let n = if avail > L7S_COPY { L7S_COPY } else { avail };
    let n = ((n - 1) & (L7S_COPY - 1)) + 1;
    let Some(mut e) = L7S_EVENTS.reserve::<L7sEvent>(0) else {
        stat(L7S_STAT_RINGBUF_FULL);
        return;
    };
    let ev = e.as_mut_ptr();
    unsafe {
        (*ev).ts_ns = now_ns();
        (*ev).cgroup_id = bpf_skb_cgroup_id(ctx.skb.skb);
        (*ev).v6 = t.v6 as u8;
        (*ev).ingress = ingress;
        (*ev).proto = proto;
        (*ev).to_server = to_server;
        (*ev).sport = t.sport;
        (*ev).dport = t.dport;
        (*ev).src = t.src;
        (*ev).dst = t.dst;
        (*ev)._pad = 0;
        (*ev).len = avail as u32;
        if bpf_skb_load_bytes(ctx.skb.skb.cast(), t.payload_off as u32, (*ev).data.as_mut_ptr().cast(), n as u32) != 0 {
            e.discard(0);
            stat(L7S_STAT_LOAD_FAIL);
            return;
        }
        (*ev).cap_len = n as u16;
    }
    e.submit(0);
    stat(L7S_STAT_EMITTED);
}

#[inline(always)]
fn sample(ctx: &SkBuffContext, ingress: bool) -> i32 {
    let Some(cfg) = L7S_CFG.get(0) else { return 1 };
    if cfg.enabled == 0 {
        return 1;
    }
    let (rate, gap) = (cfg.rate, cfg.flow_gap_ns);
    let mut t = Tuple::zero();
    if parse_skb(ctx, &mut t) == 0 || t.proto != IPPROTO_TCP || t.payload_off == 0 {
        return 1;
    }
    let (proto, to_server) = match unsafe { L7S_PORTS.get(&t.dport) } {
        Some(p) => (*p, true),
        None => match unsafe { L7S_PORTS.get(&t.sport) } {
            Some(p) => (*p, false),
            None => return 1,
        },
    };
    let len = ctx.len() as usize;
    if len <= t.payload_off {
        return 1;
    }
    stat(L7S_STAT_ELIGIBLE);
    let now = now_ns();
    if gap != 0 {
        let key = flow_hash(&t, ingress);
        if let Some(last) = unsafe { L7S_RATE.get(&key) } {
            if now.wrapping_sub(*last) < gap {
                stat(L7S_STAT_RATE_LIMITED);
                return 1;
            }
        }
        let _ = L7S_RATE.insert(&key, &now, 0);
    }
    if !take(&L7S_BUCKET, rate) {
        stat(L7S_STAT_RATE_LIMITED);
        return 1;
    }
    emit(ctx, &t, proto as u32 | (to_server as u32) << 8 | (ingress as u32) << 16, len - t.payload_off);
    1
}

#[cgroup_skb]
pub fn mn_l7s_egress(ctx: SkBuffContext) -> i32 {
    sample(&ctx, false)
}

#[cgroup_skb]
pub fn mn_l7s_ingress(ctx: SkBuffContext) -> i32 {
    sample(&ctx, true)
}
