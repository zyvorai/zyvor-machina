// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! AF_XDP fast path for a dedicated interface: frames on a queue whose gate
//! is open go to the AF_XDP socket registered for that queue; everything
//! else (gate closed, no socket bound) passes to the kernel stack.

use aya_ebpf::{
    bindings::xdp_action,
    macros::{map, xdp},
    maps::{Array, PerCpuArray, XskMap},
    programs::XdpContext,
};
use machina_bpf_common::*;

#[map]
pub static AFXDP_XSKS: XskMap = XskMap::with_max_entries(AFXDP_MAX_QUEUES, 0);
#[map]
pub static AFXDP_QUEUE_ENABLED: Array<u32> = Array::with_max_entries(AFXDP_MAX_QUEUES, 0);
#[map]
pub static AFXDP_STATS: PerCpuArray<u64> = PerCpuArray::with_max_entries(AFXDP_MAX_QUEUES * AFXDP_STAT_SLOTS, 0);

#[inline(always)]
fn stat(q: u32, i: u32) {
    if let Some(p) = AFXDP_STATS.get_ptr_mut(q * AFXDP_STAT_SLOTS + i) {
        unsafe { *p += 1 };
    }
}

#[xdp]
pub fn mn_xdp_afxdp(ctx: XdpContext) -> u32 {
    let q = ctx.rx_queue_index();
    if q >= AFXDP_MAX_QUEUES {
        return xdp_action::XDP_PASS;
    }
    match AFXDP_QUEUE_ENABLED.get(q) {
        Some(v) if *v != 0 => {}
        _ => return xdp_action::XDP_PASS,
    }
    if AFXDP_XSKS.get(q).is_none() {
        stat(q, AFXDP_STAT_NOSOCK);
        return xdp_action::XDP_PASS;
    }
    match AFXDP_XSKS.redirect(q, xdp_action::XDP_PASS as u64) {
        Ok(a) => {
            stat(q, AFXDP_STAT_REDIRECT);
            a
        }
        Err(a) => a,
    }
}
