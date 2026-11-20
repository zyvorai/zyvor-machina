// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Who changed the network: a kprobe on `rtnetlink_rcv_msg`, which runs in
//! the task that sent the NETLINK_ROUTE request, so the current task is the
//! requester. Records state-changing requests only (GET/dumps are dropped
//! before anything is reserved); never alters the message or its outcome.

use aya_ebpf::{
    helpers::{
        bpf_get_current_pid_tgid, bpf_get_current_uid_gid,
        generated::{bpf_get_current_cgroup_id, bpf_get_current_comm, bpf_probe_read_kernel, bpf_probe_read_kernel_str},
    },
    macros::{kprobe, map},
    maps::{Array, PerCpuArray, RingBuf},
    programs::ProbeContext,
};
use machina_bpf_common::*;

use crate::net::now_ns;

#[map]
pub static RTNL_CFG: Array<RtnlCfg> = Array::with_max_entries(1, 0);
#[map]
pub static RTNL_STATS: PerCpuArray<u64> = PerCpuArray::with_max_entries(2, 0);
#[map]
pub static RTNL_EVENTS: RingBuf = RingBuf::with_byte_size(256 * 1024, 0);

const NLMSG_HDRLEN: usize = 16;
const RTA_DST: u16 = 1;
const RTA_OIF: u16 = 4;
const IFLA_IFNAME: u16 = 3;
const ATTR_WALK: usize = 24;

#[inline(always)]
fn bump(idx: u32) {
    if let Some(p) = RTNL_STATS.get_ptr_mut(idx) {
        unsafe { *p += 1 };
    }
}

/// Read a scalar (≤ 8 bytes) from kernel memory.
#[inline(always)]
fn rd<T: Copy + Default>(p: *const u8) -> Option<T> {
    let mut v = T::default();
    let r = unsafe { bpf_probe_read_kernel((&mut v as *mut T).cast(), core::mem::size_of::<T>() as u32, p.cast()) };
    (r == 0).then_some(v)
}

/// ifinfomsg, ifaddrmsg and ndmsg carry the ifindex at payload byte 4.
#[inline(always)]
fn names_ifindex(t: u16) -> bool {
    matches!(t, 16 | 17 | 19 | 20 | 21 | 28 | 29)
}

#[inline(always)]
unsafe fn route_fields(ev: *mut RtnlEvent, nlh: *const u8, len: u32) {
    unsafe {
        (*ev).family = rd::<u8>(nlh.add(NLMSG_HDRLEN)).unwrap_or(0);
        (*ev).dst_len = rd::<u8>(nlh.add(NLMSG_HDRLEN + 1)).unwrap_or(0);
        let mut off = (NLMSG_HDRLEN + 12) as u32;
        for _ in 0..ATTR_WALK {
            if off + 4 > len || off > 4096 {
                break;
            }
            let Some(a) = rd::<u32>(nlh.add(off as usize)) else { break };
            let (alen, aty) = (a & 0xffff, (a >> 16) as u16);
            if alen < 4 {
                break;
            }
            let val = nlh.add(off as usize + 4);
            if aty == RTA_OIF && alen >= 8 {
                if let Some(idx) = rd::<i32>(val) {
                    if idx > 0 {
                        (*ev).ifindex = idx as u32;
                    }
                }
            } else if aty == RTA_DST && alen >= 8 {
                let n = if alen >= 20 { 16 } else { 4 };
                bpf_probe_read_kernel((*ev).dst.as_mut_ptr().cast(), n, val.cast());
            }
            off += (alen + 3) & !3;
        }
    }
}

#[inline(always)]
unsafe fn link_name(ev: *mut RtnlEvent, nlh: *const u8, len: u32) {
    unsafe {
        let mut off = (NLMSG_HDRLEN + 16) as u32;
        for _ in 0..ATTR_WALK {
            if off + 4 > len || off > 4096 {
                break;
            }
            let Some(a) = rd::<u32>(nlh.add(off as usize)) else { break };
            let alen = a & 0xffff;
            if alen < 4 {
                break;
            }
            if (a >> 16) as u16 == IFLA_IFNAME {
                bpf_probe_read_kernel_str((*ev).ifname.as_mut_ptr().cast(), 16, nlh.add(off as usize + 4).cast());
                break;
            }
            off += (alen + 3) & !3;
        }
    }
}

/// Inode of `sock_net(skb->sk)` via BTF-resolved offsets; 0 when unknown.
#[inline(always)]
fn request_netns(skb: *const u8, sk_off: u32, net_off: u32, inum_off: u32) -> u32 {
    if skb.is_null() || (sk_off == 0 && net_off == 0) {
        return 0;
    }
    let sk = rd::<u64>(unsafe { skb.add((sk_off & 0xffff) as usize) }).unwrap_or(0);
    if sk == 0 {
        return 0;
    }
    let net = rd::<u64>(unsafe { (sk as *const u8).add((net_off & 0xffff) as usize) }).unwrap_or(0);
    if net == 0 {
        return 0;
    }
    rd::<u32>(unsafe { (net as *const u8).add((inum_off & 0xffff) as usize) }).unwrap_or(0)
}

/// `rtnetlink_rcv_msg(struct sk_buff *skb, struct nlmsghdr *nlh, struct netlink_ext_ack *extack)`.
#[kprobe]
pub fn mn_rtnl(ctx: ProbeContext) -> u32 {
    let Some(cfg) = RTNL_CFG.get(0) else { return 0 };
    if cfg.enabled == 0 {
        return 0;
    }
    let (mask, want_ns) = (cfg.type_mask, cfg.netns);
    let (sk_off, net_off, inum_off) = (cfg.off_skb_sk, cfg.off_sk_net, cfg.off_net_inum);
    let Some(nlh) = ctx.arg::<*const u8>(1) else { return 0 };
    let Some(len) = rd::<u32>(nlh) else { return 0 };
    let Some(tf) = rd::<u32>(unsafe { nlh.add(4) }) else { return 0 };
    let ty = (tf & 0xffff) as u16;
    if !(16..80).contains(&ty) || mask & (1u64 << (ty - 16)) == 0 {
        return 0;
    }
    let skb: *const u8 = ctx.arg(0).unwrap_or(core::ptr::null());
    let netns = request_netns(skb, sk_off, net_off, inum_off);
    if want_ns != 0 && netns != 0 && netns != want_ns {
        return 0;
    }
    let Some(mut e) = RTNL_EVENTS.reserve::<RtnlEvent>(0) else {
        bump(RTNL_STAT_DROPPED);
        return 0;
    };
    let ev = e.as_mut_ptr();
    unsafe {
        let pid_tgid = bpf_get_current_pid_tgid();
        (*ev).ts_ns = now_ns();
        (*ev).cgroup_id = bpf_get_current_cgroup_id();
        (*ev).tgid = (pid_tgid >> 32) as u32;
        (*ev).pid = pid_tgid as u32;
        (*ev).uid = bpf_get_current_uid_gid() as u32;
        (*ev).nlmsg_type = ty;
        (*ev).nlmsg_flags = (tf >> 16) as u16;
        (*ev).ifindex = 0;
        (*ev).nlmsg_len = len;
        (*ev).family = 0;
        (*ev).dst_len = 0;
        (*ev)._pad = 0;
        (*ev).netns = netns;
        (*ev).dst = [0; ADDR_LEN];
        (*ev).ifname = [0; 16];
        if bpf_get_current_comm((*ev).comm.as_mut_ptr().cast(), COMM_LEN as u32) != 0 {
            (*ev).comm = [0; COMM_LEN];
        }
        if names_ifindex(ty) {
            if let Some(idx) = rd::<i32>(nlh.add(NLMSG_HDRLEN + 4)) {
                if idx > 0 {
                    (*ev).ifindex = idx as u32;
                }
            }
            if (*ev).ifindex == 0 && matches!(ty, 16 | 17 | 19) {
                link_name(ev, nlh, len);
            }
        } else if matches!(ty, 24 | 25) {
            route_fields(ev, nlh, len);
        }
    }
    e.submit(0);
    bump(RTNL_STAT_EVENTS);
    0
}
