// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Native VM lifecycle (controller::api::vms) used as the "instance" backend for
// the Fleet Cloud compute pages — Phase 4 of the external-cloud-client replacement.
// Goes through the platform controller proxy, not the daemon's external-cloud
// client. Actions enqueue a task and return immediately (no synchronous
// completion), matching every other native create/action handler in this app.

import { platformFetch } from './platform'

export interface NativeVm {
  id: string
  name: string
  host_id: string | null
  desired_state: string
  observed_state: string
  lifecycle_phase: string
  last_error: string
  managed: boolean
  uuid: string | null
  vcpus: number
  memory_mib: number
  ha_enabled: boolean
  project: string | null
  tags: string[]
  inventory_source: string
  guest_ip: string | null
  guest_tools_status: string | null
  /** EC2-style id, e.g. i-0123456789abcdef0 (controller 0.2+). */
  ec2_id?: string
  /** The flavor (instance type) the machine was launched as or last changed to, when known. */
  flavor_id?: string | null
}

interface TaskResponse {
  task_id: string
  status: string
  operation: string
}

/** Best-effort display status derived from observed_state/lifecycle_phase — native
 * VMs don't have a single Nova-style status enum (ACTIVE/SHUTOFF/BUILD/ERROR). */
export function vmDisplayStatus(
  vm: Pick<NativeVm, 'observed_state' | 'lifecycle_phase' | 'last_error'> & Partial<Pick<NativeVm, 'desired_state'>>,
): string {
  if (vm.last_error) return 'ERROR'
  if (vm.desired_state === 'sleeping' && !['running', 'blocked'].includes(vm.observed_state.toLowerCase())) return 'SLEEPING'
  if (vm.lifecycle_phase && vm.lifecycle_phase !== 'ready') return vm.lifecycle_phase.toUpperCase()
  const s = vm.observed_state.toLowerCase()
  if (s === 'running') return 'ACTIVE'
  if (s === 'shutoff' || s === 'stopped') return 'SHUTOFF'
  if (s === 'paused') return 'PAUSED'
  return vm.observed_state.toUpperCase() || 'UNKNOWN'
}

interface CreateFromTemplateBody {
  template_ref: string
  name: string
  memory?: string
  flavor_id?: string
  network?: string
  cloud_init_user?: string
  cloud_init_password?: string
  cloud_init_ssh_pubkey?: string
  /** Scale to zero after this many idle minutes; 0 = never, omit = project default. */
  sleep_after_minutes?: number
  /** Name of a saved key pair (EC2 KeyName); use instead of a pasted key. */
  key_name?: string
  /** Free-form cloud-init user-data (a #cloud-config document or a script), at most 16 KiB. */
  cloud_init_user_data?: string
  /** Sleeps (memory saved) instead of running when capacity is short. */
  preemptible?: boolean
  /** 0..100; lower is preempted first. */
  preempt_priority?: number
}

/** POST /api/v1/vms/from-template — boots an instance from a native image
 * (template) with a flavor (vcpus/memory) and network resolved server-side. */
export function createFromTemplate(body: CreateFromTemplateBody): Promise<TaskResponse> {
  return platformFetch<TaskResponse>('/api/v1/vms/from-template', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export interface RunInstancesResult {
  requested: number
  created: number
  instances: { name: string; task_id: string }[]
  error?: string | null
}

/** EC2 RunInstances: `count` machines from one image; names become `name-1 … name-N`. */
export function runInstances(
  body: CreateFromTemplateBody & { count: number; min_count?: number },
): Promise<RunInstancesResult> {
  return platformFetch<RunInstancesResult>('/api/v1/vms/run-instances', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function listVms(params?: { project?: string }): Promise<NativeVm[]> {
  const qs = params?.project ? `?project=${encodeURIComponent(params.project)}` : ''
  return platformFetch<NativeVm[]>(`/api/v1/vms${qs}`)
}

export function getVm(id: string): Promise<NativeVm> {
  return platformFetch<NativeVm>(`/api/v1/vms/${encodeURIComponent(id)}`)
}

export function patchVmTags(id: string, tags: string[]): Promise<NativeVm> {
  return platformFetch<NativeVm>(`/api/v1/vms/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    body: JSON.stringify({ tags }),
  })
}

export function startVm(id: string): Promise<TaskResponse> {
  return platformFetch<TaskResponse>(`/api/v1/vms/${encodeURIComponent(id)}/start`, { method: 'POST' })
}

export function stopVm(id: string): Promise<TaskResponse> {
  return platformFetch<TaskResponse>(`/api/v1/vms/${encodeURIComponent(id)}/stop`, { method: 'POST' })
}

export function rebootVm(id: string): Promise<TaskResponse> {
  return platformFetch<TaskResponse>(`/api/v1/vms/${encodeURIComponent(id)}/reboot`, { method: 'POST' })
}

/** Scale-to-zero: managed-save the guest; traffic to its addresses restores it. */
export function sleepVm(id: string): Promise<TaskResponse> {
  return platformFetch<TaskResponse>(`/api/v1/vms/${encodeURIComponent(id)}/sleep`, { method: 'POST' })
}

export function wakeVm(id: string): Promise<TaskResponse> {
  return platformFetch<TaskResponse>(`/api/v1/vms/${encodeURIComponent(id)}/wake`, { method: 'POST' })
}

export function isSleeping(vm: Pick<NativeVm, 'desired_state'>): boolean {
  return vm.desired_state === 'sleeping'
}

export interface SleepEvent {
  kind: 'sleep' | 'wake' | string
  reason: string
  at: string
}

export interface SleepPolicy {
  vm_id: string
  desired_state: string
  observed_state: string
  sleep_after_minutes: number | null
  project: string | null
  project_default: number | null
  effective_minutes: number
  last_active_at: string | null
  idle_minutes: number | null
  slept_at: string | null
  wakeable: boolean
  events: SleepEvent[]
}

export function getSleepPolicy(id: string): Promise<SleepPolicy> {
  return platformFetch<SleepPolicy>(`/api/v1/vms/${encodeURIComponent(id)}/sleep-policy`)
}

/** `null` inherits the project default, `0` never sleeps. */
export function setSleepPolicy(id: string, sleepAfterMinutes: number | null): Promise<SleepPolicy> {
  return platformFetch<SleepPolicy>(`/api/v1/vms/${encodeURIComponent(id)}/sleep-policy`, {
    method: 'PUT',
    body: JSON.stringify({ sleep_after_minutes: sleepAfterMinutes }),
  })
}

export interface SleepSummary {
  sleeping: { id: string; name: string; host_id: string | null; project: string | null; memory_mib: number; vcpus: number; slept_at: string | null }[]
  memory_freed_mib: number
  vcpus_freed: number
  auto_sleep_vms: number
  wakes_24h: number
  sleeps_24h: number
  recent: { vm_id: string; vm_name: string; kind: string; reason: string; at: string }[]
}

export function getSleepSummary(): Promise<SleepSummary> {
  return platformFetch<SleepSummary>('/api/v1/sleep/summary')
}

export interface RestorePoint {
  id: string
  label: string
  kind: 'manual' | 'scheduled' | 'fork' | string
  note: string | null
  quiesced: boolean
  created_at: string
  layers: { target: string; file: string }[]
  /** Forks whose disks sit on this point. */
  forks: string[]
}

export interface TimeTravel {
  vm_id: string
  every_minutes: number | null
  keep: number
  /** Oldest first. */
  points: RestorePoint[]
  forks: { vm_id: string; name: string | null; restore_point_id: string | null; memory: boolean; isolated: boolean; created_at: string }[]
  fork_of: { vm_id: string; name: string | null; restore_point_id: string | null; memory: boolean; isolated: boolean } | null
}

const vmPath = (id: string) => `/api/v1/vms/${encodeURIComponent(id)}`

export function getTimeTravel(id: string): Promise<TimeTravel> {
  return platformFetch<TimeTravel>(`${vmPath(id)}/restore-points`)
}

export function createRestorePoint(id: string, note?: string): Promise<TaskResponse> {
  return platformFetch<TaskResponse>(`${vmPath(id)}/restore-points`, {
    method: 'POST',
    body: JSON.stringify({ note: note || null }),
  })
}

/** `everyMinutes` 0 turns scheduled restore points off. */
export function setRestorePointPolicy(id: string, everyMinutes: number, keep?: number): Promise<TimeTravel> {
  return platformFetch<TimeTravel>(`${vmPath(id)}/restore-points/policy`, {
    method: 'PUT',
    body: JSON.stringify({ every_minutes: everyMinutes, keep }),
  })
}

export function rewindVm(id: string, pointId: string): Promise<TaskResponse> {
  return platformFetch<TaskResponse>(`${vmPath(id)}/restore-points/${encodeURIComponent(pointId)}/rewind`, { method: 'POST' })
}

export interface ForkOptions {
  name: string
  /** Omit to fork the instance as it is right now. */
  restore_point_id?: string
  /** Copy RAM too; the fork runs on an isolated network with the same addresses. */
  memory?: boolean
  isolate?: boolean
  reseed?: boolean
  start?: boolean
}

export function forkVm(id: string, opts: ForkOptions): Promise<TaskResponse> {
  return platformFetch<TaskResponse>(`${vmPath(id)}/fork`, { method: 'POST', body: JSON.stringify(opts) })
}

export function detachFork(id: string): Promise<TaskResponse> {
  return platformFetch<TaskResponse>(`${vmPath(id)}/fork/detach`, { method: 'POST' })
}

export function deleteVm(id: string): Promise<TaskResponse> {
  return platformFetch<TaskResponse>(`/api/v1/vms/${encodeURIComponent(id)}/delete`, {
    method: 'POST',
    body: JSON.stringify({ confirmed: true }),
  })
}

export interface NativeVmDisk {
  id: string
  name: string
  size_gib: number
  storage_class: string
  path: string | null
}

export interface NativeVmNic {
  mac_address: string
  network: string
  model: string
  ip?: string
}

export interface NativePortForward {
  id: string
  protocol: string
  host_port: number
  vm_ip: string
  vm_port: number
  description: string
}

/** Native equivalent of a "floating IP": a host_port -> vm_ip:vm_port NAT rule,
 * scoped to whichever VM currently holds that guest IP. There is no separate
 * allocatable floating-IP pool — see api/nativeVms.ts module doc. */
export function listVmPortForwards(id: string): Promise<NativePortForward[]> {
  return platformFetch<NativePortForward[]>(`/api/v1/vms/${encodeURIComponent(id)}/port-forwards`)
}

export async function createVmPortForward(
  id: string,
  body: { protocol: string; host_port: number; vm_port: number; description?: string },
): Promise<void> {
  await platformFetch(`/api/v1/vms/${encodeURIComponent(id)}/port-forwards`, {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export async function deleteVmPortForward(
  id: string,
  body: { protocol: string; host_port: number; vm_port: number },
): Promise<void> {
  await platformFetch(`/api/v1/vms/${encodeURIComponent(id)}/port-forwards/delete`, {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function listVmDisks(id: string): Promise<NativeVmDisk[]> {
  return platformFetch<NativeVmDisk[]>(`/api/v1/vms/${encodeURIComponent(id)}/disks`)
}

export function listVmNics(id: string): Promise<NativeVmNic[]> {
  return platformFetch<NativeVmNic[]>(`/api/v1/vms/${encodeURIComponent(id)}/nics`)
}

export function attachVmNic(id: string, network: string, model = 'virtio'): Promise<TaskResponse> {
  return platformFetch<TaskResponse>(`/api/v1/vms/${encodeURIComponent(id)}/nics/attach`, {
    method: 'POST',
    body: JSON.stringify({ network, model }),
  })
}

export function detachVmNic(id: string, mac: string): Promise<TaskResponse> {
  return platformFetch<TaskResponse>(`/api/v1/vms/${encodeURIComponent(id)}/nics/detach/${encodeURIComponent(mac)}`, {
    method: 'POST',
  })
}

/** Change the instance type: the guest is shut down cleanly, resized to the flavor and started again if it was running. */
export function changeVmType(id: string, flavorId: string): Promise<TaskResponse> {
  return platformFetch<TaskResponse>(`/api/v1/vms/${encodeURIComponent(id)}/change-type`, {
    method: 'POST',
    body: JSON.stringify({ flavor_id: flavorId }),
  })
}
