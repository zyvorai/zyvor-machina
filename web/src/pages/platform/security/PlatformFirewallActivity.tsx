// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Shield } from 'lucide-react'
import { MacListRow } from '../../../components/platform/mac/PlatformMacUi'
import SecurityLensLayout from '../../../components/platform/SecurityLensLayout'
import { getFirewallActivity, getFirewallOverview } from '../../../api/zeusFirewall'
import { formatUserError } from '../../../utils/apiError'
import { statusToneClass } from '../../../utils/semanticColors'

type ActivityEvent = Record<string, unknown> & { target?: string; group?: string }

function field(obj: unknown, key: string): unknown {
  if (!obj || typeof obj !== 'object') return undefined
  return (obj as Record<string, unknown>)[key]
}

// Some events nest details under process/network/dns objects, others put
// flat fields at the top level — try both.
function processBinary(e: ActivityEvent): unknown {
  return field(e.process, 'binary') ?? e.process_name ?? (typeof e.process === 'string' ? e.process : undefined)
}
function eventPort(e: ActivityEvent): unknown {
  return field(e.network, 'port') ?? e.destination_port ?? e.port ?? e.target_port
}
function dnsQuery(e: ActivityEvent): unknown {
  return field(e.dns, 'query') ?? e.dns_query ?? e.domain
}
function sourceIp(e: ActivityEvent): unknown {
  return field(e.network, 'src_ip') ?? e.source_ip ?? e.source
}
function destinationIp(e: ActivityEvent): unknown {
  return field(e.network, 'dst_ip') ?? e.destination_ip ?? e.destination
}

function eventTitle(e: ActivityEvent): string {
  const verdict = e.verdict ?? e.action ?? e.kind
  const port = eventPort(e)
  const proc = processBinary(e)
  if (proc && port) return `${String(proc)} · ${String(verdict || 'flow')} port ${port}`
  if (proc) return `${String(proc)} · ${String(verdict || 'flow')}`
  if (port) return `${String(verdict || 'flow')} · port ${port}`
  return String(e.summary ?? e.message ?? verdict ?? 'Network event')
}

function eventSubtitle(e: ActivityEvent): string {
  const parts = [
    e.target,
    processBinary(e),
    dnsQuery(e),
    sourceIp(e),
    destinationIp(e),
  ].filter((p) => p !== undefined && p !== null && p !== '' && typeof p !== 'object')
  return parts.map(String).join(' · ')
}

export default function PlatformFirewallActivity() {
  const [blocked, setBlocked] = useState<ActivityEvent[]>([])
  const [allowed, setAllowed] = useState<ActivityEvent[]>([])
  const [note, setNote] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const load = useCallback(async () => {
    setError(null)
    setNote(null)
    setLoading(true)
    try {
      const ov = await getFirewallOverview()
      // Each target's activity query is independent — fire concurrently. allSettled so one
      // unreachable/erroring target doesn't blank the whole page; its events are just omitted.
      const results = await Promise.allSettled(ov.targets.map((t) => getFirewallActivity(t.id)))
      const failedTargets: string[] = []
      const blockedEv: ActivityEvent[] = []
      const allowedEv: ActivityEvent[] = []
      ov.targets.forEach((t, i) => {
        const result = results[i]
        if (result.status === 'rejected') {
          failedTargets.push(t.name)
          return
        }
        const act = result.value
        if (typeof act.note === 'string') setNote(act.note)
        const ev = act.events
        if (!Array.isArray(ev)) return
        for (const raw of ev) {
          if (!raw || typeof raw !== 'object') continue
          const e = { ...(raw as Record<string, unknown>), target: t.name } as ActivityEvent
          const v = String(e.verdict ?? e.action ?? '').toLowerCase()
          if (v.includes('drop') || v.includes('block') || v.includes('deny')) {
            blockedEv.push({ ...e, group: 'blocked' })
          } else {
            allowedEv.push({ ...e, group: 'allowed' })
          }
        }
      })
      setBlocked(blockedEv)
      setAllowed(allowedEv)
      if (failedTargets.length > 0) {
        setNote((prev) => {
          const failMsg = `Activity unavailable for ${failedTargets.length} target(s): ${failedTargets.join(', ')}`
          return prev ? `${prev} · ${failMsg}` : failMsg
        })
      }
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const totalEvents = blocked.length + allowed.length

  return (
    <SecurityLensLayout
      testId="platform-firewall-activity-page"
      backHref="/platform/zeus/security"
      backLabel="Security Center"
      title="Firewall Activity"
      subtitle={`${blocked.length} blocked · ${allowed.length} allowed connection events`}
      icon={<Shield className="w-6 h-6 text-[var(--text-muted)]" />}
      loading={loading && totalEvents === 0}
      error={error}
      onRefresh={() => void load()}
      stats={[
        { label: 'Blocked', value: String(blocked.length), tone: blocked.length > 0 ? 'warn' : 'default', icon: <Shield className="w-4 h-4" /> },
        { label: 'Allowed', value: String(allowed.length), tone: allowed.length > 0 ? 'ok' : 'default' },
        { label: 'Total events', value: String(totalEvents) },
      ]}
      panelTitle="Recent"
      isEmpty={totalEvents === 0}
      emptyTitle="No connection events yet"
      emptySubtitle="Run machina-bpfd on the target host for live blocked flows and connection telemetry."
    >
      <p className="text-xs text-[var(--text-muted)] mb-4">{note || 'Live flows come from machina-bpfd on the target host'}</p>
      <div className="space-y-4">
        {blocked.length > 0 && (
          <div>
            <p className={`text-xs font-semibold uppercase tracking-wide mb-2 px-1 ${statusToneClass('error')}`}>Blocked</p>
            <div className="rounded-xl border border-white/[0.06] overflow-hidden">
              {blocked.slice(0, 25).map((e, i) => (
                <MacListRow key={`b-${i}`} title={eventTitle(e)} subtitle={eventSubtitle(e)} />
              ))}
            </div>
            {blocked.length > 25 && (
              <p className="text-xs text-[var(--text-muted)] mt-1 px-1">+{blocked.length - 25} more</p>
            )}
          </div>
        )}
        {allowed.length > 0 && (
          <div>
            <p className={`text-xs font-semibold uppercase tracking-wide mb-2 px-1 ${statusToneClass('ok')}`}>Allowed</p>
            <div className="rounded-xl border border-white/[0.06] overflow-hidden">
              {allowed.slice(0, 15).map((e, i) => (
                <MacListRow key={`a-${i}`} title={eventTitle(e)} subtitle={eventSubtitle(e)} />
              ))}
            </div>
            {allowed.length > 15 && (
              <p className="text-xs text-[var(--text-muted)] mt-1 px-1">+{allowed.length - 15} more</p>
            )}
          </div>
        )}
      </div>
    </SecurityLensLayout>
  )
}
