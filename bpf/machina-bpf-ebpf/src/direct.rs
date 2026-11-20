// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Bridge-less direct redirect. `mn_direct` (outer device ingress, first in
//! the TCX chain) sends frames for a VM's MAC or IP straight out of its tap,
//! so the tap's egress hooks still run; `mn_direct_out` (tap ingress) sends
//! the VM's frames out of the outer device. Both only act while the lease is
//! live and fall through (TC_ACT_UNSPEC) on anything unmatched.

use aya_ebpf::{
    helpers::generated::bpf_redirect,
    macros::{classifier, map},
    maps::{Array, HashMap, PerCpuArray},
    programs::TcContext,
};
use machina_bpf_common::*;

use crate::{
    net::{now_ns, TC_ACT_UNSPEC},
    parse::{parse_tc, Tuple},
};

#[map]
pub static DIRECT_CFG: Array<DirectCfg> = Array::with_max_entries(1, 0);
/// Guest MAC → tap ifindex.
#[map]
pub static DIRECT_MAC: HashMap<u64, u32> = HashMap::with_max_entries(1024, 0);
/// Guest IP → tap ifindex.
#[map]
pub static DIRECT_IP: HashMap<[u8; ADDR_LEN], u32> = HashMap::with_max_entries(4096, 0);
/// Tap ifindex → outer ifindex.
#[map]
pub static DIRECT_OUT: HashMap<u32, u32> = HashMap::with_max_entries(1024, 0);
#[map]
pub static DIRECT_STATS: PerCpuArray<u64> = PerCpuArray::with_max_entries(4, 0);

#[inline(always)]
fn stat(i: u32) {
    if let Some(p) = DIRECT_STATS.get_ptr_mut(i) {
        unsafe { *p += 1 };
    }
}

#[inline(always)]
fn live() -> bool {
    match DIRECT_CFG.get(0) {
        Some(c) if c.enabled != 0 => {
            if now_ns() < c.lease_deadline_ns {
                true
            } else {
                stat(DIRECT_STAT_IDLE);
                false
            }
        }
        _ => false,
    }
}

#[classifier]
pub fn mn_direct(ctx: TcContext) -> i32 {
    if !live() {
        return TC_ACT_UNSPEC;
    }
    let Ok(dst) = ctx.load::<[u8; 6]>(0) else { return TC_ACT_UNSPEC };
    let tap = match unsafe { DIRECT_MAC.get(&mac_key(&dst)) } {
        Some(i) => *i,
        None => {
            let mut t = Tuple::zero();
            if parse_tc(&ctx, &mut t) == 0 {
                return TC_ACT_UNSPEC;
            }
            match unsafe { DIRECT_IP.get(&t.dst) } {
                Some(i) => *i,
                None => return TC_ACT_UNSPEC,
            }
        }
    };
    stat(DIRECT_STAT_IN);
    unsafe { bpf_redirect(tap, 0) as i32 }
}

#[classifier]
pub fn mn_direct_out(ctx: TcContext) -> i32 {
    if !live() {
        return TC_ACT_UNSPEC;
    }
    let ifindex = unsafe { (*ctx.skb.skb).ifindex };
    let Some(outer) = (unsafe { DIRECT_OUT.get(&ifindex) }).copied() else { return TC_ACT_UNSPEC };
    stat(DIRECT_STAT_OUT);
    unsafe { bpf_redirect(outer, 0) as i32 }
}
