// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! VM runtime intelligence (opt-in, observe only). Userspace fills the
//! tracking maps with QEMU processes, threads and cgroups; every hook returns
//! early for anything untracked, and per-feature bits gate each group.

use aya_ebpf::{
    helpers::generated::{bpf_get_current_cgroup_id, bpf_get_current_pid_tgid, bpf_get_smp_processor_id},
    macros::{kprobe, kretprobe, map, tracepoint},
    maps::{Array, HashMap, LruHashMap, PerCpuArray, PerCpuHashMap},
    programs::{ProbeContext, RetProbeContext, TracePointContext},
};
use machina_bpf_common::*;

use crate::net::now_ns;

#[map]
pub static VMI_CFG: Array<VmiCfg> = Array::with_max_entries(1, 0);
/// QEMU tgid → vm key.
#[map]
pub static VMI_TGIDS: HashMap<u32, u32> = HashMap::with_max_entries(1024, 0);
/// QEMU thread id → (vm key, vCPU index).
#[map]
pub static VMI_TIDS: HashMap<u32, VmiThread> = HashMap::with_max_entries(16384, 0);
/// Cgroup id (QEMU scope and descendants) → vm key.
#[map]
pub static VMI_CGROUPS: HashMap<u64, u32> = HashMap::with_max_entries(4096, 0);
#[map]
pub static VMI_HIST: PerCpuHashMap<VmiKey, u64> = PerCpuHashMap::with_max_entries(16384, 0);
/// `VMI_TS_* << 32 | tid` → timestamp.
#[map]
pub static VMI_TS: LruHashMap<u64, u64> = LruHashMap::with_max_entries(32768, 0);
/// struct request * → (start, vm).
#[map]
pub static VMI_BLK: LruHashMap<u64, VmiBlk> = LruHashMap::with_max_entries(16384, 0);
/// vm key → first kvm_entry (monotonic ns).
#[map]
pub static VMI_BOOT: HashMap<u32, u64> = HashMap::with_max_entries(1024, 0);
/// Per-CPU hardirq (0) / softirq (1) start.
#[map]
pub static VMI_CPU_TS: PerCpuArray<u64> = PerCpuArray::with_max_entries(2, 0);

#[inline(always)]
fn cfg(feature: u32) -> Option<&'static VmiCfg> {
    let c = VMI_CFG.get(0)?;
    (c.enabled != 0 && c.features & feature != 0).then_some(c)
}

#[inline(always)]
fn add(vm: u32, kind: u16, slot: u16, v: u64) {
    let k = VmiKey { vm, kind, slot };
    match VMI_HIST.get_ptr_mut(&k) {
        Some(p) => unsafe { *p += v },
        None => {
            let _ = VMI_HIST.insert(&k, &v, 0);
        }
    }
}

#[inline(always)]
fn current() -> (u32, u32) {
    let id = unsafe { bpf_get_current_pid_tgid() };
    ((id >> 32) as u32, id as u32)
}

/// Tracked tgid, else a task already in a tracked cgroup (before userspace
/// has registered its process).
#[inline(always)]
fn current_vm() -> Option<u32> {
    let (tgid, _) = current();
    if let Some(vm) = unsafe { VMI_TGIDS.get(&tgid) } {
        return Some(*vm);
    }
    let cg = unsafe { bpf_get_current_cgroup_id() };
    unsafe { VMI_CGROUPS.get(&cg) }.copied()
}

#[inline(always)]
fn vcpu(tid: u32) -> Option<u32> {
    let t = unsafe { VMI_TIDS.get(&tid) }?;
    (t.vcpu != u32::MAX).then_some(t.vm)
}

#[inline(always)]
fn ts_key(ns: u64, tid: u32) -> u64 {
    ns << 32 | tid as u64
}

#[inline(always)]
fn take_ts(key: u64) -> Option<u64> {
    let v = unsafe { VMI_TS.get(&key) }.copied();
    if v.is_some() {
        let _ = VMI_TS.remove(&key);
    }
    v
}

// ---- flight recorder -------------------------------------------------------

#[tracepoint]
pub fn mn_vmi_kvm_exit(ctx: TracePointContext) -> u32 {
    let Some(c) = cfg(VMI_F_FLIGHT) else { return 0 };
    let Some(vm) = current_vm() else { return 0 };
    let reason = unsafe { ctx.read_at::<u32>(c.off_exit_reason as usize) }.unwrap_or(0xffff);
    add(vm, vmi_kind::EXIT, (reason & 0xffff) as u16, 1);
    0
}

/// Learns vCPU threads (QEMU only names them with debug-threads=on) and
/// records each VM's first entry.
#[tracepoint]
pub fn mn_vmi_kvm_entry(ctx: TracePointContext) -> u32 {
    let Some(c) = cfg(VMI_F_FLIGHT | VMI_F_MEM) else { return 0 };
    let (_, tid) = current();
    let vm = match unsafe { VMI_TIDS.get(&tid) } {
        Some(t) if t.vcpu != u32::MAX => t.vm,
        _ => {
            let Some(vm) = current_vm() else { return 0 };
            let id = unsafe { ctx.read_at::<u32>(c.off_entry_vcpu as usize) }.unwrap_or(0);
            let _ = VMI_TIDS.insert(&tid, &VmiThread { vm, vcpu: id }, 0);
            vm
        }
    };
    if c.features & VMI_F_MEM != 0 && unsafe { VMI_BOOT.get(&vm) }.is_none() {
        let _ = VMI_BOOT.insert(&vm, &now_ns(), 1 /* BPF_NOEXIST */);
    }
    0
}

#[tracepoint]
pub fn mn_vmi_wakeup(ctx: TracePointContext) -> u32 {
    let Some(c) = cfg(VMI_F_FLIGHT) else { return 0 };
    let Ok(pid) = (unsafe { ctx.read_at::<u32>(c.off_wakeup_pid as usize) }) else { return 0 };
    if vcpu(pid).is_some() {
        let _ = VMI_TS.insert(&ts_key(VMI_TS_RUNQ, pid), &now_ns(), 0);
    }
    0
}

#[tracepoint]
pub fn mn_vmi_switch(ctx: TracePointContext) -> u32 {
    let Some(c) = cfg(VMI_F_FLIGHT) else { return 0 };
    let (Ok(prev), Ok(next)) = (unsafe { ctx.read_at::<u32>(c.off_switch_prev_pid as usize) }, unsafe {
        ctx.read_at::<u32>(c.off_switch_next_pid as usize)
    }) else {
        return 0;
    };
    let pv = vcpu(prev);
    let nv = vcpu(next);
    if pv.is_none() && nv.is_none() {
        return 0;
    }
    let now = now_ns();
    let cpu = unsafe { bpf_get_smp_processor_id() } as u16;
    if let Some(vm) = pv {
        if let Some(start) = take_ts(ts_key(VMI_TS_RUN, prev)) {
            add(vm, vmi_kind::RESIDENCY, cpu, now.wrapping_sub(start));
        }
        // Preempted while runnable: it waits on the run queue from now.
        let state = unsafe { ctx.read_at::<u32>(c.off_switch_prev_state as usize) }.unwrap_or(1);
        if state == 0 {
            let _ = VMI_TS.insert(&ts_key(VMI_TS_RUNQ, prev), &now, 0);
        }
    }
    if let Some(vm) = nv {
        if let Some(q) = take_ts(ts_key(VMI_TS_RUNQ, next)) {
            add(vm, vmi_kind::RUNQ, log2_slot(now.wrapping_sub(q)), 1);
        }
        let _ = VMI_TS.insert(&ts_key(VMI_TS_RUN, next), &now, 0);
    }
    0
}

#[tracepoint]
pub fn mn_vmi_migrate(ctx: TracePointContext) -> u32 {
    let Some(c) = cfg(VMI_F_FLIGHT) else { return 0 };
    let Ok(pid) = (unsafe { ctx.read_at::<u32>(c.off_migrate_pid as usize) }) else { return 0 };
    if let Some(vm) = vcpu(pid) {
        add(vm, vmi_kind::MIGRATE, 0, 1);
    }
    0
}

// ---- I/O ---------------------------------------------------------------------

#[inline(always)]
fn blk_key(ctx: &TracePointContext, dev_off: u32, sector_off: u32) -> Option<u64> {
    let dev = unsafe { ctx.read_at::<u32>(dev_off as usize) }.ok()?;
    let sector = unsafe { ctx.read_at::<u64>(sector_off as usize) }.ok()?;
    Some((dev as u64) << 44 ^ sector)
}

/// block_bio_queue runs in the submitter's context (the VMM's I/O thread).
#[tracepoint]
pub fn mn_vmi_blk_start(ctx: TracePointContext) -> u32 {
    let Some(c) = cfg(VMI_F_IO) else { return 0 };
    let Some(vm) = current_vm() else { return 0 };
    let Some(k) = blk_key(&ctx, c.off_bio_dev, c.off_bio_sector) else { return 0 };
    let _ = VMI_BLK.insert(&k, &VmiBlk { ts: now_ns(), vm, _pad: 0 }, 0);
    0
}

#[tracepoint]
pub fn mn_vmi_blk_done(ctx: TracePointContext) -> u32 {
    let Some(c) = cfg(VMI_F_IO) else { return 0 };
    let Some(k) = blk_key(&ctx, c.off_rqc_dev, c.off_rqc_sector) else { return 0 };
    let Some(b) = (unsafe { VMI_BLK.get(&k) }).copied() else { return 0 };
    let _ = VMI_BLK.remove(&k);
    add(b.vm, vmi_kind::BLK, log2_slot(now_ns().wrapping_sub(b.ts)), 1);
    0
}

#[kprobe]
pub fn mn_vmi_vhost_work(_ctx: ProbeContext) -> u32 {
    if cfg(VMI_F_IO).is_none() {
        return 0;
    }
    if let Some(vm) = current_vm() {
        add(vm, vmi_kind::VHOST_WORK, 0, 1);
    }
    0
}

#[kprobe]
pub fn mn_vmi_vhost_kick(_ctx: ProbeContext) -> u32 {
    if cfg(VMI_F_IO).is_none() {
        return 0;
    }
    if let Some(vm) = current_vm() {
        add(vm, vmi_kind::VHOST_KICK, 0, 1);
    }
    0
}

// ---- memory ------------------------------------------------------------------

#[inline(always)]
fn mem_begin(ns: u64) {
    if cfg(VMI_F_MEM).is_none() || current_vm().is_none() {
        return;
    }
    let (_, tid) = current();
    let _ = VMI_TS.insert(&ts_key(ns, tid), &now_ns(), 0);
}

#[inline(always)]
fn mem_end(ns: u64, kind: u16) {
    if cfg(VMI_F_MEM).is_none() {
        return;
    }
    let Some(vm) = current_vm() else { return };
    let (_, tid) = current();
    if let Some(start) = take_ts(ts_key(ns, tid)) {
        add(vm, kind, log2_slot(now_ns().wrapping_sub(start)), 1);
    }
}

#[kprobe]
pub fn mn_vmi_fault(_ctx: ProbeContext) -> u32 {
    mem_begin(VMI_TS_FAULT);
    0
}

#[kretprobe]
pub fn mn_vmi_fault_ret(_ctx: RetProbeContext) -> u32 {
    mem_end(VMI_TS_FAULT, vmi_kind::FAULT);
    0
}

#[tracepoint]
pub fn mn_vmi_reclaim_begin(_ctx: TracePointContext) -> u32 {
    mem_begin(VMI_TS_RECLAIM);
    0
}

#[tracepoint]
pub fn mn_vmi_reclaim_end(_ctx: TracePointContext) -> u32 {
    mem_end(VMI_TS_RECLAIM, vmi_kind::RECLAIM);
    0
}

// ---- topology ----------------------------------------------------------------

#[inline(always)]
fn irq_begin(slot: u32) {
    if cfg(VMI_F_TOPO).is_none() {
        return;
    }
    if let Some(p) = VMI_CPU_TS.get_ptr_mut(slot) {
        unsafe { *p = now_ns() };
    }
}

#[inline(always)]
fn irq_end(slot: u32, kind: u16) {
    if cfg(VMI_F_TOPO).is_none() {
        return;
    }
    let Some(p) = VMI_CPU_TS.get_ptr_mut(slot) else { return };
    let start = unsafe { *p };
    if start == 0 {
        return;
    }
    unsafe { *p = 0 };
    let cpu = unsafe { bpf_get_smp_processor_id() } as u16;
    add(0, kind, cpu, now_ns().wrapping_sub(start));
}

#[tracepoint]
pub fn mn_vmi_irq_entry(_ctx: TracePointContext) -> u32 {
    irq_begin(0);
    0
}

#[tracepoint]
pub fn mn_vmi_irq_exit(_ctx: TracePointContext) -> u32 {
    irq_end(0, vmi_kind::IRQ);
    0
}

#[tracepoint]
pub fn mn_vmi_softirq_entry(_ctx: TracePointContext) -> u32 {
    irq_begin(1);
    0
}

#[tracepoint]
pub fn mn_vmi_softirq_exit(_ctx: TracePointContext) -> u32 {
    irq_end(1, vmi_kind::SOFTIRQ);
    0
}
