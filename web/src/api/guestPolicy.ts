// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Per-container eBPF policy inside a guest, relayed by the daemon to the
// in-guest GuestKit agent (guestkit.netpolicy.* / guestkit.lsm.*).

import { apiGet, apiPut } from './client'

const API = '/api/v1'

export type GuestPolicyMode = 'audit' | 'enforce' | 'off'

export interface GuestNetRule {
  direction: 'egress' | 'ingress'
  cidr: string
  proto?: string
  port?: number
}

export interface GuestPolicyEvent {
  ts_ns: number
  cgroup_id: number
  kind: 'net' | 'lsm'
  action: string
  denied: boolean
  pid: number
  comm?: string
  peer?: string
  proto?: number
  port?: number
  detail?: string
  target?: string | null
}

interface TargetBase {
  target: string
  cgroup: string
  cgroup_ids: number[]
  mode: 'audit' | 'enforce'
  lease_remaining_secs: number | null
  lease_expired: boolean
}

export interface GuestNetTarget extends TargetBase {
  egress: boolean
  ingress: boolean
  rules: GuestNetRule[]
  attached: boolean
  counters: { allowed: number; audited: number; denied: number }
}

export interface GuestLsmTarget extends TargetBase {
  deny_exec: boolean
  allow_exec: string[]
  deny_wx: boolean
  restrict_devices: boolean
  allow_devices: string[]
  restrict_writes: boolean
  writable_paths: string[]
  counters: { action: string; denied: boolean; count: number }[]
}

interface StatusBase {
  available: boolean
  programs_compiled: boolean
  reason?: string
}

export interface GuestNetStatus extends StatusBase {
  targets: GuestNetTarget[]
  events: GuestPolicyEvent[]
}

export interface GuestLsmStatus extends StatusBase {
  lsm_active: boolean
  lsm_list: string
  hooks_attached?: boolean
  note?: string | null
  targets: GuestLsmTarget[]
  events: GuestPolicyEvent[]
}

export interface GuestNetApply {
  container?: string
  cgroup?: string
  mode: GuestPolicyMode
  lease_secs?: number
  egress: boolean
  ingress: boolean
  rules: GuestNetRule[]
}

export interface GuestLsmApply {
  container?: string
  cgroup?: string
  mode: GuestPolicyMode
  lease_secs?: number
  deny_exec: boolean
  allow_exec: string[]
  deny_wx: boolean
  restrict_devices: boolean
  allow_devices: string[]
  restrict_writes: boolean
  writable_paths: string[]
}

const vmPath = (vm: string, leaf: string) => `${API}/vms/${encodeURIComponent(vm)}/${leaf}`

export const getGuestNetpolicy = (vm: string) => apiGet<GuestNetStatus>(vmPath(vm, 'guest-policy'))
export const applyGuestNetpolicy = (vm: string, body: GuestNetApply) =>
  apiPut<Record<string, unknown>>(vmPath(vm, 'guest-policy'), body)
export const getGuestLsm = (vm: string) => apiGet<GuestLsmStatus>(vmPath(vm, 'guest-lsm'))
export const applyGuestLsm = (vm: string, body: GuestLsmApply) =>
  apiPut<Record<string, unknown>>(vmPath(vm, 'guest-lsm'), body)

/** `egress 10.0.0.0/8 tcp 443` per line (proto and port optional). */
export function parseGuestNetRules(text: string): GuestNetRule[] {
  const rules: GuestNetRule[] = []
  for (const raw of text.split('\n')) {
    const line = raw.trim()
    if (!line || line.startsWith('#')) continue
    const [direction, cidr, proto, port] = line.split(/\s+/)
    if ((direction !== 'egress' && direction !== 'ingress') || !cidr) {
      throw new Error(`Rule "${line}": expected "egress|ingress CIDR [proto] [port]"`)
    }
    const rule: GuestNetRule = { direction, cidr }
    if (proto && proto !== 'any') rule.proto = proto
    if (port) {
      const n = Number(port)
      if (!Number.isInteger(n) || n < 1 || n > 65535) throw new Error(`Rule "${line}": bad port`)
      rule.port = n
    }
    rules.push(rule)
  }
  return rules
}

export const formatGuestNetRules = (rules: GuestNetRule[]) =>
  rules.map((r) => [r.direction, r.cidr, r.proto ?? '', r.port ?? ''].join(' ').trim()).join('\n')
