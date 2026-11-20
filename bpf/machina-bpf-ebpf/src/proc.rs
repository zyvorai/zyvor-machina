// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Process / exec / file / connect telemetry from stable tracepoints.
//!
//! Field offsets come from TP_OFF (filled by the loader from tracefs
//! `format` files), so no kernel struct layout is compiled in.

use aya_ebpf::{
    EbpfContext,
    helpers::generated::{
        bpf_get_current_cgroup_id, bpf_get_current_comm, bpf_get_current_pid_tgid,
        bpf_get_current_uid_gid, bpf_probe_read_kernel_str, bpf_probe_read_user_str,
        bpf_send_signal,
    },
    macros::{kprobe, tracepoint},
    programs::{ProbeContext, TracePointContext},
};
use machina_bpf_common::*;

use crate::{
    maps::*,
    net::{current_scope, enforce_active, now_ns},
};

const SIGKILL: u32 = 9;
const TCP_SYN_SENT: i32 = 2;

#[inline(always)]
pub fn tp_off(idx: u32) -> Option<usize> {
    match TP_OFF.get(idx) {
        Some(&v) if v != tp::MISSING => Some(v as usize),
        _ => None,
    }
}

#[inline(always)]
fn global_flags() -> u32 {
    CONFIG.get(0).map(|c| c.flags).unwrap_or(0)
}

#[inline(always)]
unsafe fn fill_common(ev: *mut ProcEvent, kind: u32) {
    unsafe {
        let pid_tgid = bpf_get_current_pid_tgid();
        let uid_gid = bpf_get_current_uid_gid();
        (*ev).ts_ns = now_ns();
        (*ev).cgroup_id = bpf_get_current_cgroup_id();
        (*ev).kind = kind;
        (*ev).pid = pid_tgid as u32;
        (*ev).tgid = (pid_tgid >> 32) as u32;
        (*ev).child_pid = 0;
        (*ev).uid = uid_gid as u32;
        (*ev).gid = (uid_gid >> 32) as u32;
        (*ev).flags = 0;
        (*ev).policy_id = 0;
        (*ev).open_flags = 0;
        (*ev).family = 0;
        (*ev).proto = 0;
        (*ev).sport = 0;
        (*ev).dport = 0;
        (*ev)._pad = 0;
        (*ev).saddr = [0; ADDR_LEN];
        (*ev).daddr = [0; ADDR_LEN];
        (*ev).path[0] = 0;
        if bpf_get_current_comm((*ev).comm.as_mut_ptr().cast(), COMM_LEN as u32) != 0 {
            (*ev).comm[0] = 0;
        }
    }
}

#[tracepoint]
pub fn mn_tp_exec(ctx: TracePointContext) -> u32 {
    let flags = global_flags();
    if flags & (GLOBAL_EXEC_EVENTS | GLOBAL_EXEC_DENY) == 0 {
        return 0;
    }
    let Some(mut e) = PROC_EVENTS.reserve::<ProcEvent>(0) else {
        return 0;
    };
    let ev = e.as_mut_ptr();
    unsafe {
        fill_common(ev, PROC_EV_EXEC);
        if let Some(o) = tp_off(tp::EXEC_FILENAME_LOC) {
            if let Ok(loc) = ctx.read_at::<u32>(o) {
                let p = (ctx.as_ptr() as *const u8).add((loc & 0xffff) as usize);
                if bpf_probe_read_kernel_str((*ev).path.as_mut_ptr().cast(), PATH_LEN as u32, p.cast()) < 0 {
                    (*ev).path[0] = 0;
                }
            }
        }
        if flags & GLOBAL_EXEC_DENY != 0 {
            let h = fnv1a64(&(*ev).path);
            if let Some(&policy) = EXEC_DENY.get(&h) {
                (*ev).policy_id = policy;
                (*ev).flags |= PROC_FLAG_EXEC_DENIED;
                if enforce_active((*ev).ts_ns) {
                    bpf_send_signal(SIGKILL);
                    (*ev).flags |= PROC_FLAG_EXEC_KILLED;
                }
            }
        }
        if flags & GLOBAL_EXEC_EVENTS == 0 && (*ev).flags == 0 {
            e.discard(0);
            return 0;
        }
    }
    e.submit(0);
    0
}

#[tracepoint]
pub fn mn_tp_exit(_ctx: TracePointContext) -> u32 {
    if global_flags() & GLOBAL_EXEC_EVENTS == 0 {
        return 0;
    }
    let pid_tgid = unsafe { bpf_get_current_pid_tgid() };
    if pid_tgid as u32 != (pid_tgid >> 32) as u32 {
        return 0;
    }
    let Some(mut e) = PROC_EVENTS.reserve::<ProcEvent>(0) else {
        return 0;
    };
    unsafe { fill_common(e.as_mut_ptr(), PROC_EV_EXIT) };
    e.submit(0);
    0
}

#[tracepoint]
pub fn mn_tp_fork(ctx: TracePointContext) -> u32 {
    if global_flags() & GLOBAL_FORK_EVENTS == 0 {
        return 0;
    }
    let (Some(po), Some(co)) = (tp_off(tp::FORK_PARENT_PID), tp_off(tp::FORK_CHILD_PID)) else {
        return 0;
    };
    let Some(mut e) = PROC_EVENTS.reserve::<ProcEvent>(0) else {
        return 0;
    };
    let ev = e.as_mut_ptr();
    unsafe {
        fill_common(ev, PROC_EV_FORK);
        let parent = ctx.read_at::<i32>(po).unwrap_or(0);
        let child = ctx.read_at::<i32>(co).unwrap_or(0);
        (*ev).tgid = parent as u32;
        (*ev).child_pid = child as u32;
    }
    e.submit(0);
    0
}

#[tracepoint]
pub fn mn_tp_openat(ctx: TracePointContext) -> u32 {
    if global_flags() & GLOBAL_FILE_WATCH == 0 {
        return 0;
    }
    let Some(fo) = tp_off(tp::OPENAT_FILENAME) else {
        return 0;
    };
    let Some(mut e) = PROC_EVENTS.reserve::<ProcEvent>(0) else {
        return 0;
    };
    let ev = e.as_mut_ptr();
    unsafe {
        fill_common(ev, PROC_EV_FILE_OPEN);
        let up = ctx.read_at::<u64>(fo).unwrap_or(0);
        if up == 0
            || bpf_probe_read_user_str((*ev).path.as_mut_ptr().cast(), PATH_LEN as u32, up as *const _) <= 0
        {
            e.discard(0);
            return 0;
        }
        if let Some(o) = tp_off(tp::OPENAT_FLAGS) {
            (*ev).open_flags = ctx.read_at::<u64>(o).unwrap_or(0) as u32;
        }
        let mut matched = false;
        let mut deny_policy = 0u32;
        let mut deny = false;
        for i in 0..FILE_WATCH_SLOTS {
            let Some(w) = FILE_WATCH.get(i) else {
                continue;
            };
            let l = w.len as usize;
            if l == 0 || l > FILE_WATCH_PREFIX_LEN {
                continue;
            }
            let mut ok = true;
            for j in 0..FILE_WATCH_PREFIX_LEN {
                if j >= l {
                    break;
                }
                if (*ev).path[j] != w.prefix[j] {
                    ok = false;
                    break;
                }
            }
            if ok {
                matched = true;
                if w.flags & WATCH_DENY != 0 {
                    deny = true;
                    deny_policy = w.policy_id;
                }
                break;
            }
        }
        if !matched {
            e.discard(0);
            return 0;
        }
        if deny {
            (*ev).policy_id = deny_policy;
            (*ev).flags |= PROC_FLAG_EXEC_DENIED;
            if enforce_active((*ev).ts_ns) {
                bpf_send_signal(SIGKILL);
                (*ev).flags |= PROC_FLAG_EXEC_KILLED;
            }
        }
    }
    e.submit(0);
    0
}

/// `deny_cap`: `int cap_capable(const struct cred *, struct user_namespace *, int cap, unsigned int opts)`.
#[kprobe]
pub fn mn_kp_cap_capable(ctx: ProbeContext) -> u32 {
    if global_flags() & GLOBAL_CAP_DENY == 0 {
        return 0;
    }
    let Some(cap) = ctx.arg::<i32>(2) else {
        return 0;
    };
    let scope = current_scope().unwrap_or(0);
    let mut key = CapKey {
        scope,
        cap: cap as u32,
    };
    let mut policy = unsafe { CAP_DENY.get(&key) }.copied();
    if policy.is_none() && scope != 0 {
        key.scope = 0;
        policy = unsafe { CAP_DENY.get(&key) }.copied();
    }
    let Some(policy) = policy else {
        return 0;
    };
    let Some(mut e) = PROC_EVENTS.reserve::<ProcEvent>(0) else {
        return 0;
    };
    let ev = e.as_mut_ptr();
    unsafe {
        fill_common(ev, PROC_EV_CAP_DENIED);
        (*ev).policy_id = policy;
        (*ev).open_flags = cap as u32;
        (*ev).flags |= PROC_FLAG_EXEC_DENIED;
        if enforce_active((*ev).ts_ns) {
            bpf_send_signal(SIGKILL);
            (*ev).flags |= PROC_FLAG_EXEC_KILLED;
        }
    }
    e.submit(0);
    0
}

#[tracepoint]
pub fn mn_tp_sock_state(ctx: TracePointContext) -> u32 {
    if global_flags() & GLOBAL_CONNECT_EVENTS == 0 {
        return 0;
    }
    let Some(no) = tp_off(tp::ISS_NEWSTATE) else {
        return 0;
    };
    let newstate = unsafe { ctx.read_at::<i32>(no) }.unwrap_or(0);
    if newstate != TCP_SYN_SENT {
        return 0;
    }
    let Some(mut e) = PROC_EVENTS.reserve::<ProcEvent>(0) else {
        return 0;
    };
    let ev = e.as_mut_ptr();
    unsafe {
        fill_common(ev, PROC_EV_CONNECT);
        if let Some(o) = tp_off(tp::ISS_SPORT) {
            (*ev).sport = ctx.read_at::<u16>(o).unwrap_or(0);
        }
        if let Some(o) = tp_off(tp::ISS_DPORT) {
            (*ev).dport = ctx.read_at::<u16>(o).unwrap_or(0);
        }
        if let Some(o) = tp_off(tp::ISS_FAMILY) {
            (*ev).family = ctx.read_at::<u16>(o).unwrap_or(0);
        }
        if let Some(o) = tp_off(tp::ISS_PROTOCOL) {
            (*ev).proto = ctx.read_at::<u16>(o).unwrap_or(0);
        }
        if let Some(o) = tp_off(tp::ISS_SADDR_V6) {
            (*ev).saddr = ctx.read_at::<[u8; ADDR_LEN]>(o).unwrap_or([0; ADDR_LEN]);
        }
        if let Some(o) = tp_off(tp::ISS_DADDR_V6) {
            (*ev).daddr = ctx.read_at::<[u8; ADDR_LEN]>(o).unwrap_or([0; ADDR_LEN]);
        }
    }
    e.submit(0);
    0
}
