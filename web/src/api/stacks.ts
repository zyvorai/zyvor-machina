// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Native declarative stacks (controller::api::stacks) — Phase 4 of the
// external-cloud-client replacement, the Heat equivalent. Goes through the platform
// controller proxy.
//
// Unlike Heat, a stack template here is NOT an arbitrary YAML resource graph —
// it's a fixed JSON shape describing security groups, volumes, VMs, and (v2)
// instance groups with who-talks-to-whom policies (see StackTemplate on the
// controller). There is no free-form resource-type system.

import { platformFetch } from './platform'
import type { ZyraActionRow } from './ai'

interface StackSecurityGroupRule {
  direction?: string
  protocol?: string
  port_min?: number
  port_max?: number
  remote_cidr?: string
}

export interface StackInstances {
  name: string
  count?: number
  flavor?: string
  cpu_cores?: number
  memory?: string
  disk_gib?: number
  image?: string
  network?: string
  labels?: Record<string, string>
  anti_affinity?: boolean
  ha?: boolean
  sleep_after_minutes?: number
  restore_points?: { every_minutes: number; keep?: number }
  backup?: { interval_hours: number; retain?: number }
  scaling?: { min: number; max: number; metric?: string; target?: number }
}

export interface StackPolicy {
  from: string
  to: string
  ports?: number[]
  protocol?: string
}

export interface StackTemplate {
  security_groups?: { name: string; rules?: StackSecurityGroupRule[] }[]
  volumes?: { name: string; size_gib: number; volume_class?: string }[]
  vms?: {
    name: string
    memory?: string
    cpu_cores?: number
    disk_gib?: number
    network?: string
    attach_volumes?: string[]
  }[]
  instances?: StackInstances[]
  policies?: StackPolicy[]
}

interface StackResourceRef {
  kind: string
  id: string
  name: string
}

export interface DriftItem {
  kind: string
  name: string
  detail: string
  fixed: boolean
}

export interface StackDrift {
  in_sync?: boolean
  open?: number
  items?: DriftItem[]
}

export interface NativeStack {
  id: string
  project_id: string | null
  name: string
  status: string
  last_error: string | null
  template_json: StackTemplate
  resources_json: StackResourceRef[]
  drift_json?: StackDrift
  checked_at?: string | null
  auto_heal?: boolean
  updated_at?: string | null
}

export interface PlanVm {
  name: string
  group: string
  vcpus: number
  memory_mib: number
  disk_gib: number
  image: string | null
  host: string | null
  monthly_usd: number
  action: 'create' | 'keep' | 'resize' | 'delete'
}

export interface StackPlan {
  name: string
  project: string
  errors: string[]
  vms: PlanVm[]
  totals: { vms: number; vcpus: number; memory_mib: number; storage_gib: number }
  monthly_usd: number
  quota: { ok: boolean; detail: string }
  placement: { ok: boolean; detail: string }
  policies: StackPolicy[]
  policy_yaml: string
  replay: unknown
  replay_summary: string
  diff: {
    create: string[]
    delete: string[]
    resize: [string, string][]
    relabel: string[]
    policies_upsert: string[]
    policies_delete: string[]
  } | null
}

export interface StackDraft {
  template: StackTemplate
  source: 'llm' | 'rules'
  notes: string[]
  plan: StackPlan
}

const base = '/api/v1/stacks'
const one = (id: string) => `${base}/${encodeURIComponent(id)}`
const post = <T>(path: string, body: unknown, method = 'POST') =>
  platformFetch<T>(path, { method, body: JSON.stringify(body) })

export function listStacks(): Promise<NativeStack[]> {
  return platformFetch<NativeStack[]>(base)
}

export function getStack(id: string): Promise<NativeStack> {
  return platformFetch<NativeStack>(one(id))
}

export function createStack(body: { name: string; template: StackTemplate }): Promise<NativeStack> {
  return post<NativeStack>(base, body)
}

export async function deleteStack(id: string): Promise<void> {
  await platformFetch<{ deleted: boolean }>(one(id), { method: 'DELETE' })
}

export const draftStack = (body: { name: string; prompt: string; rules_only?: boolean }) =>
  post<StackDraft>(`${base}/draft`, body)

export const planStack = (body: { name: string; template: StackTemplate; stack_id?: string }) =>
  post<StackPlan>(`${base}/plan`, body)

export const proposeStack = (body: { name: string; template: StackTemplate; stack_id?: string; prompt?: string }) =>
  post<ZyraActionRow>(`${base}/propose`, body)

export const updateStack = (id: string, template: StackTemplate) =>
  post<NativeStack>(one(id), { template }, 'PUT')

export const getStackDrift = (id: string) => platformFetch<StackDrift>(`${one(id)}/drift`)

export const convergeStack = (id: string) => post<StackDrift>(`${one(id)}/converge`, {})

export const setStackAutoHeal = (id: string, enabled: boolean) =>
  post<NativeStack>(`${one(id)}/auto-heal`, { enabled }, 'PUT')
