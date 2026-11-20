// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { PlatformHost } from '../api/platform'

export type AttentionKind = 'offline' | 'validation' | 'heartbeat' | 'maintenance' | 'fenced' | 'unschedulable'

export interface AttentionItem {
  hostId: string
  hostname: string
  kind: AttentionKind
  severity: 'error' | 'warn' | 'info'
  reason: string
  /** What the one-click button does, when there is one. */
  fix?: 'recheck'
}

/** A heartbeat older than this on an "online" host means the agent stopped talking. */
export const STALE_HEARTBEAT_SECS = 120

export function heartbeatAgeSecs(iso: string | null | undefined, now: number = Date.now()): number | null {
  if (!iso) return null
  const t = Date.parse(iso.includes('T') || iso.endsWith('Z') ? iso : iso.replace(' ', 'T') + 'Z')
  if (Number.isNaN(t)) return null
  return Math.max(0, Math.round((now - t) / 1000))
}

export function formatAge(secs: number | null): string {
  if (secs == null) return 'never'
  if (secs < 5) return 'just now'
  if (secs < 90) return `${secs} s ago`
  const m = Math.round(secs / 60)
  if (m < 90) return `${m} min ago`
  const h = Math.round(m / 60)
  if (h < 48) return `${h} h ago`
  return `${Math.round(h / 24)} d ago`
}

/** The agent of the machine the controller itself runs on is reached over loopback. */
export function isControllerHost(h: Pick<PlatformHost, 'agent_grpc_addr' | 'address'>): boolean {
  const a = (h.agent_grpc_addr || '').toLowerCase()
  return a.startsWith('127.') || a.startsWith('localhost') || a.startsWith('[::1]')
}

/** Everything about one host that needs a person, most serious first. */
export function hostAttention(h: PlatformHost, now: number = Date.now()): AttentionItem[] {
  const out: AttentionItem[] = []
  const base = { hostId: h.id, hostname: h.hostname }
  if (h.state === 'offline' || h.state === 'error') {
    out.push({ ...base, kind: 'offline', severity: 'error', reason: `Not reporting (${h.state}); last heartbeat ${formatAge(heartbeatAgeSecs(h.last_heartbeat_at, now))}`, fix: 'recheck' })
  } else if (h.state === 'online') {
    const age = heartbeatAgeSecs(h.last_heartbeat_at, now)
    if (age != null && age > STALE_HEARTBEAT_SECS) {
      out.push({ ...base, kind: 'heartbeat', severity: 'warn', reason: `Heartbeat is stale: last one ${formatAge(age)}` })
    }
  }
  if (h.validation_status === 'failed') {
    out.push({ ...base, kind: 'validation', severity: 'error', reason: 'Validation failed; see the failing check', fix: 'recheck' })
  }
  if (h.fenced) out.push({ ...base, kind: 'fenced', severity: 'error', reason: 'Fenced: no new VMs and no automatic actions on it' })
  if (h.maintenance_mode) out.push({ ...base, kind: 'maintenance', severity: 'info', reason: 'In maintenance: VMs are not scheduled here' })
  else if (!h.schedulable && !h.fenced) out.push({ ...base, kind: 'unschedulable', severity: 'warn', reason: 'Not schedulable: new VMs will not be placed here' })
  const rank = { error: 0, warn: 1, info: 2 } as const
  return out.sort((a, b) => rank[a.severity] - rank[b.severity])
}

export function fleetAttention(hosts: PlatformHost[], now: number = Date.now()): AttentionItem[] {
  return hosts.flatMap((h) => hostAttention(h, now)).sort((a, b) => {
    const rank = { error: 0, warn: 1, info: 2 } as const
    return rank[a.severity] - rank[b.severity]
  })
}

export interface FleetFacts {
  total: number
  online: number
  vms: number
  cpuPercent: number | null
  memPercent: number | null
  needAttention: number
}

/** Fleet-wide numbers for the header strip, computed from the host list only. */
export function fleetFacts(hosts: PlatformHost[], now: number = Date.now()): FleetFacts {
  const online = hosts.filter((h) => h.state === 'online').length
  const reporting = hosts.filter((h) => h.state === 'online')
  const cpuPercent = reporting.length ? Math.round(reporting.reduce((s, h) => s + (h.cpu_percent || 0), 0) / reporting.length) : null
  const memTotal = reporting.reduce((s, h) => s + (h.memory_total_mib || 0), 0)
  const memUsed = reporting.reduce((s, h) => s + (h.memory_used_mib || 0), 0)
  const needAttention = new Set(hosts.filter((h) => hostAttention(h, now).some((a) => a.severity !== 'info')).map((h) => h.id)).size
  return {
    total: hosts.length,
    online,
    vms: hosts.reduce((s, h) => s + (h.vm_count || 0), 0),
    cpuPercent,
    memPercent: memTotal > 0 ? Math.round((memUsed / memTotal) * 100) : null,
    needAttention,
  }
}
