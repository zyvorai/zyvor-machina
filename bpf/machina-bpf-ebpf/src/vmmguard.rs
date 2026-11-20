// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! VMM guard: BPF-LSM hooks scoped to QEMU cgroups (GUARD_POLICIES). Exec
//! outside the binary allowlist, writable+executable mappings and opens of
//! char devices outside the device allowlist are audited; they are denied
//! only while `enforce` is set and the lease deadline has not passed.

use aya_ebpf::{
    helpers::generated::{bpf_get_current_cgroup_id, bpf_get_current_comm, bpf_get_current_pid_tgid, bpf_probe_read_kernel},
    macros::{lsm, map},
    maps::{Array, HashMap, PerCpuArray, RingBuf},
    programs::LsmContext,
};
use machina_bpf_common::*;

use crate::net::now_ns;

#[map]
pub static GUARD_CFG: Array<GuardCfg> = Array::with_max_entries(1, 0);
/// QEMU cgroup id → GUARD_* flags.
#[map]
pub static GUARD_POLICIES: HashMap<u64, u32> = HashMap::with_max_entries(4096, 0);
/// Executables QEMU may exec.
#[map]
pub static GUARD_FILES: HashMap<GuardFileKey, u8> = HashMap::with_max_entries(256, 0);
/// `major << 32 | minor` (minor DEV_MINOR_ANY = any) of allowed char devices.
#[map]
pub static GUARD_DEVICES: HashMap<u64, u8> = HashMap::with_max_entries(256, 0);
#[map]
pub static GUARD_STATS: PerCpuArray<u64> = PerCpuArray::with_max_entries(4, 0);
#[map]
pub static GUARD_EVENTS: RingBuf = RingBuf::with_byte_size(256 * 1024, 0);

const EPERM: i32 = 1;
const PROT_WRITE: u64 = 2;
const PROT_EXEC: u64 = 4;
const VM_WRITE: u64 = 2;
const S_IFMT: u16 = 0o170000;
const S_IFCHR: u16 = 0o020000;

#[inline(always)]
fn rd<T: Copy + Default>(p: u64, off: u32) -> Option<T> {
    let mut v = T::default();
    let r = unsafe {
        bpf_probe_read_kernel((&mut v as *mut T).cast(), core::mem::size_of::<T>() as u32, (p + off as u64) as *const _)
    };
    (r == 0).then_some(v)
}

#[inline(always)]
fn stat(i: u32) {
    if let Some(p) = GUARD_STATS.get_ptr_mut(i) {
        unsafe { *p += 1 };
    }
}

/// (cfg, flags) when the current task is in a guarded cgroup.
#[inline(always)]
fn scope(flag: u32) -> Option<&'static GuardCfg> {
    let c = GUARD_CFG.get(0)?;
    if c.enabled == 0 {
        return None;
    }
    let cg = unsafe { bpf_get_current_cgroup_id() };
    let f = unsafe { GUARD_POLICIES.get(&cg) }?;
    (*f & flag != 0).then_some(c)
}

/// Record a violation; returns the hook's verdict.
#[inline(never)]
fn violation(c: &GuardCfg, hook: u8, a: u32, b: u64) -> i32 {
    let deny = c.enforce != 0 && now_ns() < c.lease_deadline_ns;
    stat(if deny { GUARD_STAT_DENIED } else { GUARD_STAT_AUDITED });
    match GUARD_EVENTS.reserve::<GuardEvent>(0) {
        Some(mut e) => {
            let ev = e.as_mut_ptr();
            unsafe {
                let id = bpf_get_current_pid_tgid();
                (*ev).ts_ns = now_ns();
                (*ev).cgroup_id = bpf_get_current_cgroup_id();
                (*ev).tgid = (id >> 32) as u32;
                (*ev).pid = id as u32;
                (*ev).hook = hook;
                (*ev).denied = deny as u8;
                (*ev)._pad = 0;
                (*ev).a = a;
                (*ev).b = b;
                bpf_get_current_comm((*ev).comm.as_mut_ptr().cast(), 16);
            }
            e.submit(0);
        }
        None => stat(GUARD_STAT_DROPPED),
    }
    if deny { -EPERM } else { 0 }
}

#[lsm(hook = "bprm_check_security")]
pub fn mn_guard_exec(ctx: LsmContext) -> i32 {
    let prev: i32 = unsafe { ctx.arg(1) };
    if prev != 0 {
        return prev;
    }
    let Some(c) = scope(GUARD_EXEC) else { return 0 };
    let bprm: u64 = unsafe { ctx.arg(0) };
    let Some(file) = rd::<u64>(bprm, c.off_bprm_file) else { return 0 };
    let Some(inode) = rd::<u64>(file, c.off_file_inode) else { return 0 };
    let ino = rd::<u64>(inode, c.off_inode_ino).unwrap_or(0);
    let sb = rd::<u64>(inode, c.off_inode_sb).unwrap_or(0);
    let dev = if sb != 0 { rd::<u32>(sb, c.off_sb_dev).unwrap_or(0) } else { 0 };
    let key = GuardFileKey { ino, dev, _pad: 0 };
    if unsafe { GUARD_FILES.get(&key) }.is_some() {
        return 0;
    }
    violation(c, GUARD_HOOK_EXEC, dev, ino)
}

#[lsm(hook = "file_mprotect")]
pub fn mn_guard_mprotect(ctx: LsmContext) -> i32 {
    let prev: i32 = unsafe { ctx.arg(3) };
    if prev != 0 {
        return prev;
    }
    let Some(c) = scope(GUARD_WX) else { return 0 };
    let prot: u64 = unsafe { ctx.arg(2) };
    if prot & PROT_EXEC == 0 {
        return 0;
    }
    let vma: u64 = unsafe { ctx.arg(0) };
    let flags = rd::<u64>(vma, c.off_vma_flags).unwrap_or(0);
    if prot & PROT_WRITE == 0 && flags & VM_WRITE == 0 {
        return 0;
    }
    violation(c, GUARD_HOOK_MPROTECT, prot as u32, flags)
}

#[lsm(hook = "file_open")]
pub fn mn_guard_open(ctx: LsmContext) -> i32 {
    let prev: i32 = unsafe { ctx.arg(1) };
    if prev != 0 {
        return prev;
    }
    let Some(c) = scope(GUARD_DEV) else { return 0 };
    let file: u64 = unsafe { ctx.arg(0) };
    let Some(inode) = rd::<u64>(file, c.off_file_inode) else { return 0 };
    let mode = rd::<u16>(inode, c.off_inode_mode).unwrap_or(0);
    if mode & S_IFMT != S_IFCHR {
        return 0;
    }
    let rdev = rd::<u32>(inode, c.off_inode_rdev).unwrap_or(0);
    let (major, minor) = (rdev >> 20, rdev & 0xfffff);
    let exact = (major as u64) << 32 | minor as u64;
    let any = (major as u64) << 32 | DEV_MINOR_ANY as u64;
    if unsafe { GUARD_DEVICES.get(&exact) }.is_some() || unsafe { GUARD_DEVICES.get(&any) }.is_some() {
        return 0;
    }
    violation(c, GUARD_HOOK_OPEN, major, minor as u64)
}
