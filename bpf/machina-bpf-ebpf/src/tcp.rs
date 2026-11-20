// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! TCP connect latency and per-peer pressure (`mn_sockops` on the root
//! cgroup, observe only), plus the ICMP error histogram fed from tc.

use aya_ebpf::{
    helpers::bpf_get_socket_cookie,
    macros::{map, sock_ops},
    maps::{HashMap, LruHashMap},
    programs::{SockOpsContext, TcContext},
};
use machina_bpf_common::*;

use crate::{net::now_ns, parse::Tuple};

const BPF_SOCK_OPS_TCP_CONNECT_CB: u32 = 3;
const BPF_SOCK_OPS_ACTIVE_ESTABLISHED_CB: u32 = 4;
const BPF_SOCK_OPS_PASSIVE_ESTABLISHED_CB: u32 = 5;
const BPF_SOCK_OPS_RETRANS_CB: u32 = 9;
const BPF_SOCK_OPS_STATE_CB: u32 = 10;
const BPF_SOCK_OPS_RETRANS_CB_FLAG: i32 = 1 << 1;
const BPF_SOCK_OPS_STATE_CB_FLAG: i32 = 1 << 2;
const TCP_SYN_SENT: u32 = 2;
const TCP_CLOSE: u32 = 7;
const AF_INET: u32 = 2;
const AF_INET6: u32 = 10;
const BPF_NOEXIST: u64 = 1;

/// Socket cookie → connect start (ns).
#[map]
pub static CONNECT_START: LruHashMap<u64, u64> = LruHashMap::with_max_entries(16384, 0);

#[map]
pub static CONNECT_HEALTH: LruHashMap<ConnKey, ConnStats> = LruHashMap::with_max_entries(16384, 0);

#[map]
pub static TCP_PRESSURE: LruHashMap<[u8; ADDR_LEN], TcpPressure> = LruHashMap::with_max_entries(16384, 0);

#[map]
pub static ICMP_ERRORS: HashMap<IcmpErrKey, u64> = HashMap::with_max_entries(4096, 0);

/// Context fields are read with volatile loads before any branching: the
/// verifier rejects loads through a ctx pointer LLVM has pre-offset to share
/// between the IPv4 and IPv6 paths.
#[inline(always)]
fn remote(ctx: &SockOpsContext) -> Option<[u8; ADDR_LEN]> {
    use core::ptr::{addr_of, read_volatile};
    let o = ctx.ops;
    let (family, ip4, w) = unsafe {
        (
            read_volatile(addr_of!((*o).family)),
            read_volatile(addr_of!((*o).remote_ip4)),
            [
                read_volatile(addr_of!((*o).remote_ip6[0])),
                read_volatile(addr_of!((*o).remote_ip6[1])),
                read_volatile(addr_of!((*o).remote_ip6[2])),
                read_volatile(addr_of!((*o).remote_ip6[3])),
            ],
        )
    };
    match family {
        AF_INET => Some(v4_mapped(ip4.to_ne_bytes())),
        AF_INET6 => {
            let mut a = [0u8; ADDR_LEN];
            let mut i = 0;
            while i < 4 {
                let b = w[i].to_ne_bytes();
                a[i * 4] = b[0];
                a[i * 4 + 1] = b[1];
                a[i * 4 + 2] = b[2];
                a[i * 4 + 3] = b[3];
                i += 1;
            }
            Some(a)
        }
        _ => None,
    }
}

#[inline(always)]
fn conn_key(ctx: &SockOpsContext, addr: [u8; ADDR_LEN]) -> ConnKey {
    // remote_port is the network-order port in the upper half.
    ConnKey { addr, port: u32::from_be(ctx.remote_port()) as u16, _pad: [0; 6] }
}

#[inline(never)]
fn conn_record(key: &ConnKey, us: u64, failed: bool) {
    let s = match CONNECT_HEALTH.get_ptr_mut(key) {
        Some(s) => unsafe { &mut *s },
        None => {
            let _ = CONNECT_HEALTH.insert(key, &ConnStats::default(), BPF_NOEXIST);
            match CONNECT_HEALTH.get_ptr_mut(key) {
                Some(s) => unsafe { &mut *s },
                None => return,
            }
        }
    };
    if failed {
        s.failures += 1;
        return;
    }
    s.count += 1;
    s.sum_us += us;
    if us > s.max_us {
        s.max_us = us;
    }
    let mut b = 0;
    while b < CONNECT_BUCKETS - 1 && us >= CONNECT_BUCKETS_US[b] {
        b += 1;
    }
    s.hist[b] += 1;
}

#[inline(always)]
fn snapshot(ctx: &SockOpsContext, addr: &[u8; ADDR_LEN], retrans_event: bool) {
    use core::ptr::{addr_of, read_volatile};
    let o = ctx.ops;
    let mut p = unsafe {
        if read_volatile(addr_of!((*o).is_fullsock)) == 0 {
            return;
        }
        TcpPressure {
            srtt_us8: read_volatile(addr_of!((*o).srtt_us)),
            cwnd: read_volatile(addr_of!((*o).snd_cwnd)),
            ssthresh: read_volatile(addr_of!((*o).snd_ssthresh)),
            mss: read_volatile(addr_of!((*o).mss_cache)),
            total_retrans: read_volatile(addr_of!((*o).total_retrans)),
            retrans_events: 0,
            rate_delivered: read_volatile(addr_of!((*o).rate_delivered)),
            rate_interval_us: read_volatile(addr_of!((*o).rate_interval_us)),
            last_ns: 0,
        }
    };
    let prev = unsafe { TCP_PRESSURE.get(addr) }.map(|p| p.retrans_events).unwrap_or(0);
    p.retrans_events = prev + retrans_event as u32;
    p.last_ns = now_ns();
    let _ = TCP_PRESSURE.insert(addr, &p, 0);
}

#[sock_ops]
pub fn mn_sockops(ctx: SockOpsContext) -> u32 {
    let Some(addr) = remote(&ctx) else { return 1 };
    let cookie = || unsafe { bpf_get_socket_cookie(ctx.ops as *mut _) };
    match ctx.op() {
        BPF_SOCK_OPS_TCP_CONNECT_CB => {
            let _ = CONNECT_START.insert(&cookie(), &now_ns(), 0);
            let _ = ctx.set_cb_flags(BPF_SOCK_OPS_STATE_CB_FLAG | BPF_SOCK_OPS_RETRANS_CB_FLAG);
        }
        BPF_SOCK_OPS_ACTIVE_ESTABLISHED_CB => {
            let c = cookie();
            if let Some(start) = unsafe { CONNECT_START.get(&c) } {
                let us = now_ns().saturating_sub(*start) / 1000;
                let _ = CONNECT_START.remove(&c);
                conn_record(&conn_key(&ctx, addr), us, false);
            }
            snapshot(&ctx, &addr, false);
        }
        BPF_SOCK_OPS_PASSIVE_ESTABLISHED_CB => {
            let _ = ctx.set_cb_flags(BPF_SOCK_OPS_STATE_CB_FLAG | BPF_SOCK_OPS_RETRANS_CB_FLAG);
            snapshot(&ctx, &addr, false);
        }
        BPF_SOCK_OPS_RETRANS_CB => snapshot(&ctx, &addr, true),
        BPF_SOCK_OPS_STATE_CB => {
            let (old, new) = (ctx.arg(0), ctx.arg(1));
            if new == TCP_CLOSE {
                let c = cookie();
                if old == TCP_SYN_SENT && unsafe { CONNECT_START.get(&c) }.is_some() {
                    conn_record(&conn_key(&ctx, addr), 0, true);
                }
                let _ = CONNECT_START.remove(&c);
            }
            snapshot(&ctx, &addr, false);
        }
        _ => {}
    }
    1
}

/// Count ICMP/ICMPv6 errors seen on a datapath interface.
#[inline(never)]
pub fn icmp_error(ctx: &TcContext, t: &Tuple, ifindex: u32, from_workload: bool) {
    let Ok(tc) = ctx.load::<[u8; 2]>(t.l4_off) else { return };
    let (ty, code) = (tc[0], tc[1]);
    let kind = if t.v6 {
        match ty {
            1 => ICMP_ERR_UNREACH,
            2 => ICMP_ERR_PTB,
            3 => ICMP_ERR_TIME_EXCEEDED,
            4 => ICMP_ERR_PARAM,
            _ => return,
        }
    } else {
        match (ty, code) {
            (3, 4) => ICMP_ERR_PTB,
            (3, _) => ICMP_ERR_UNREACH,
            (11, _) => ICMP_ERR_TIME_EXCEEDED,
            (12, _) => ICMP_ERR_PARAM,
            _ => return,
        }
    };
    let key = IcmpErrKey { ifindex, kind, code, from_workload: from_workload as u8, v6: t.v6 as u8 };
    match ICMP_ERRORS.get_ptr_mut(&key) {
        Some(p) => unsafe { *p += 1 },
        None => {
            let _ = ICMP_ERRORS.insert(&key, &1, BPF_NOEXIST);
        }
    }
}
