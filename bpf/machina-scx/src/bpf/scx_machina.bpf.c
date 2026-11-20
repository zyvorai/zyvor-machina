// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: GPL-2.0
//
// VM-aware sched_ext scheduler (ported from fluxvm_scx). SCX_OPS_SWITCH_PARTIAL:
// only tasks userspace explicitly puts in SCHED_EXT run here, and machina-bpfd
// limits that to recognised QEMU vCPU threads. Everything else stays on CFS.

/* Includes vmlinux.h itself, without the clashing generated kfunc prototypes. */
#include <scx/common.bpf.h>
#include <bpf/bpf_core_read.h>

/*
 * enums.autogen.bpf.h maps this to a rodata global that the scx C loader
 * fills from kernel BTF (SCX_ENUM_INIT); machina-scx does not, and the object
 * is always built against the target kernel's BTF, so use its enumerator.
 */
#undef SCX_DSQ_LOCAL

char LICENSE[] SEC("license") = "GPL";

#define MACHINA_SCX_DSQ 0x4d4e5331ULL /* "MNS1" */
#define MIN_SLICE_NS 100000ULL
#define MAX_SLICE_NS 10000000ULL
#define DEFAULT_SLICE_NS 1000000ULL
#define DEFAULT_WEIGHT 100U
#define MIN_WEIGHT 25U
#define MAX_WEIGHT 400U

/* Keep in sync with machina-scx/src/main.rs. */
struct task_profile {
	__u64 vm_key;
	__u32 tgid;
	__u32 weight;
	__u64 slice_ns;
	__u64 latency_target_ns;
};

struct vm_stats {
	__u64 enqueues;
	__u64 direct_dispatches;
	__u64 shared_dispatches;
	__u64 running_calls;
	__u64 runtime_ns;
	__u64 queue_delay_ns;
	__u64 queue_delay_max_ns;
	__u64 latency_violations;
};

struct {
	__uint(type, BPF_MAP_TYPE_HASH);
	__uint(max_entries, 4096);
	__type(key, __u32);
	__type(value, struct task_profile);
} scx_task_profiles SEC(".maps");

struct {
	__uint(type, BPF_MAP_TYPE_HASH);
	__uint(max_entries, 1024);
	__type(key, __u64);
	__type(value, struct vm_stats);
} scx_vm_stats SEC(".maps");

struct {
	__uint(type, BPF_MAP_TYPE_LRU_HASH);
	__uint(max_entries, 8192);
	__type(key, __u32);
	__type(value, __u64);
} scx_enqueue_ts SEC(".maps");

/* [0] = exit kind recorded by .exit (0 while running). */
struct {
	__uint(type, BPF_MAP_TYPE_ARRAY);
	__uint(max_entries, 1);
	__type(key, __u32);
	__type(value, __u64);
} scx_meta SEC(".maps");

static volatile __u64 vtime_now;

static __always_inline struct task_profile *profile_for(struct task_struct *p)
{
	__u32 tid = BPF_CORE_READ(p, pid);
	__u32 tgid = BPF_CORE_READ(p, tgid);
	struct task_profile *profile = bpf_map_lookup_elem(&scx_task_profiles, &tid);

	/* A reused TID falls back to default behaviour, never to a stale VM policy. */
	if (!profile || profile->tgid != tgid)
		return NULL;
	return profile;
}

static __always_inline struct vm_stats *stats_for(__u64 vm_key)
{
	return bpf_map_lookup_elem(&scx_vm_stats, &vm_key);
}

static __always_inline __u64 bounded_slice(const struct task_profile *profile)
{
	__u64 slice = profile ? profile->slice_ns : DEFAULT_SLICE_NS;

	if (slice < MIN_SLICE_NS)
		slice = MIN_SLICE_NS;
	if (slice > MAX_SLICE_NS)
		slice = MAX_SLICE_NS;
	return slice;
}

static __always_inline __u32 bounded_weight(const struct task_profile *profile)
{
	__u32 weight = profile ? profile->weight : DEFAULT_WEIGHT;

	if (weight < MIN_WEIGHT)
		weight = MIN_WEIGHT;
	if (weight > MAX_WEIGHT)
		weight = MAX_WEIGHT;
	return weight;
}

static __always_inline void record_enqueue(struct task_struct *p, struct task_profile *profile, bool direct)
{
	__u32 tid = BPF_CORE_READ(p, pid);
	__u64 now = bpf_ktime_get_ns();

	bpf_map_update_elem(&scx_enqueue_ts, &tid, &now, BPF_ANY);
	if (profile) {
		struct vm_stats *stats = stats_for(profile->vm_key);

		if (stats) {
			__sync_fetch_and_add(&stats->enqueues, 1);
			if (direct)
				__sync_fetch_and_add(&stats->direct_dispatches, 1);
			else
				__sync_fetch_and_add(&stats->shared_dispatches, 1);
		}
	}
}

s32 BPF_STRUCT_OPS(machina_select_cpu, struct task_struct *p, s32 prev_cpu, u64 wake_flags)
{
	bool direct = false;
	s32 cpu = scx_bpf_select_cpu_dfl(p, prev_cpu, wake_flags, &direct);
	struct task_profile *profile = profile_for(p);

	if (direct) {
		record_enqueue(p, profile, true);
		scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, bounded_slice(profile), 0);
	}
	return cpu;
}

void BPF_STRUCT_OPS(machina_enqueue, struct task_struct *p, u64 enq_flags)
{
	struct task_profile *profile = profile_for(p);
	__u64 slice = bounded_slice(profile);

	record_enqueue(p, profile, false);
	if (profile) {
		__u64 vtime = p->scx.dsq_vtime;
		__u64 floor = vtime_now > slice ? vtime_now - slice : 0;

		if (vtime < floor)
			vtime = floor;
		scx_bpf_dsq_insert_vtime(p, MACHINA_SCX_DSQ, slice, vtime, enq_flags);
	} else {
		scx_bpf_dsq_insert(p, MACHINA_SCX_DSQ, slice, enq_flags);
	}
}

void BPF_STRUCT_OPS(machina_dispatch, s32 cpu, struct task_struct *prev)
{
	scx_bpf_dsq_move_to_local(MACHINA_SCX_DSQ);
}

void BPF_STRUCT_OPS(machina_running, struct task_struct *p)
{
	struct task_profile *profile = profile_for(p);
	__u64 now = bpf_ktime_get_ns();
	__u32 tid = BPF_CORE_READ(p, pid);
	__u64 *enqueued = bpf_map_lookup_elem(&scx_enqueue_ts, &tid);

	if (profile) {
		struct vm_stats *stats = stats_for(profile->vm_key);
		__u64 task_vtime = p->scx.dsq_vtime;

		if (task_vtime > vtime_now)
			vtime_now = task_vtime;
		if (stats) {
			__sync_fetch_and_add(&stats->running_calls, 1);
			if (enqueued && now >= *enqueued) {
				__u64 delay = now - *enqueued;

				__sync_fetch_and_add(&stats->queue_delay_ns, delay);
				if (delay > stats->queue_delay_max_ns)
					stats->queue_delay_max_ns = delay;
				if (profile->latency_target_ns && delay > profile->latency_target_ns)
					__sync_fetch_and_add(&stats->latency_violations, 1);
			}
		}
	}
	if (enqueued)
		bpf_map_delete_elem(&scx_enqueue_ts, &tid);
}

void BPF_STRUCT_OPS(machina_stopping, struct task_struct *p, bool runnable)
{
	struct task_profile *profile = profile_for(p);
	__u64 slice = bounded_slice(profile);
	__u64 remaining = p->scx.slice;
	__u64 used = remaining < slice ? slice - remaining : slice;
	__u64 charge = used * DEFAULT_WEIGHT / bounded_weight(profile);

	if (!charge)
		charge = 1;
	p->scx.dsq_vtime += charge;
	if (profile) {
		struct vm_stats *stats = stats_for(profile->vm_key);

		if (stats)
			__sync_fetch_and_add(&stats->runtime_ns, used);
	}
}

void BPF_STRUCT_OPS(machina_enable, struct task_struct *p)
{
	p->scx.dsq_vtime = vtime_now;
}

s32 BPF_STRUCT_OPS_SLEEPABLE(machina_init)
{
	return scx_bpf_create_dsq(MACHINA_SCX_DSQ, -1);
}

void BPF_STRUCT_OPS(machina_exit, struct scx_exit_info *ei)
{
	__u32 zero = 0;
	__u64 kind = ei->kind;

	bpf_map_update_elem(&scx_meta, &zero, &kind, BPF_ANY);
}

SCX_OPS_DEFINE(machina_scx_ops,
	       .select_cpu = (void *)machina_select_cpu,
	       .enqueue = (void *)machina_enqueue,
	       .dispatch = (void *)machina_dispatch,
	       .running = (void *)machina_running,
	       .stopping = (void *)machina_stopping,
	       .enable = (void *)machina_enable,
	       .init = (void *)machina_init,
	       .exit = (void *)machina_exit,
	       .flags = SCX_OPS_SWITCH_PARTIAL,
	       .name = "machina_scx");
