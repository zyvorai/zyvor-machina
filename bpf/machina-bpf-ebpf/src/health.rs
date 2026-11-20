// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Network health counters: kfree_skb drop reasons, TCP retransmits and resets.
//!
//! Counters are bumped without atomics (the BPF target has no RMW atomics in
//! `core`); concurrent CPUs can lose an occasional increment.

use aya_ebpf::{macros::tracepoint, programs::TracePointContext};
use machina_bpf_common::*;

use crate::{maps::*, proc::tp_off};

const BPF_NOEXIST: u64 = 1;

#[tracepoint]
pub fn mn_tp_kfree_skb(ctx: TracePointContext) -> u32 {
    let Some(o) = tp_off(tp::KFREE_REASON) else {
        return 0;
    };
    let reason = unsafe { ctx.read_at::<u32>(o) }.unwrap_or(0);
    match DROP_REASONS.get_ptr_mut(&reason) {
        Some(p) => unsafe { *p += 1 },
        None => {
            let _ = DROP_REASONS.insert(&reason, &1, BPF_NOEXIST);
        }
    }
    0
}

#[inline(always)]
fn bump_tcp(ctx: &TracePointContext, idx: u32, kind: u32) -> u32 {
    let Some(o) = tp_off(idx) else {
        return 0;
    };
    let addr = unsafe { ctx.read_at::<[u8; ADDR_LEN]>(o) }.unwrap_or([0; ADDR_LEN]);
    let key = HealthKey {
        kind,
        _pad: 0,
        addr,
    };
    match TCP_HEALTH.get_ptr_mut(&key) {
        Some(p) => unsafe { *p += 1 },
        None => {
            let _ = TCP_HEALTH.insert(&key, &1, BPF_NOEXIST);
        }
    }
    0
}

#[tracepoint]
pub fn mn_tp_tcp_retransmit(ctx: TracePointContext) -> u32 {
    bump_tcp(&ctx, tp::RETRANS_SADDR_V6, HEALTH_RETRANSMIT)
}

#[tracepoint]
pub fn mn_tp_tcp_send_reset(ctx: TracePointContext) -> u32 {
    bump_tcp(&ctx, tp::SEND_RST_SADDR_V6, HEALTH_RST_SENT)
}

#[tracepoint]
pub fn mn_tp_tcp_receive_reset(ctx: TracePointContext) -> u32 {
    bump_tcp(&ctx, tp::RECV_RST_SADDR_V6, HEALTH_RST_RECV)
}
