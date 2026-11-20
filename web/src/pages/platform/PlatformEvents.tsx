// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ScrollText, Terminal } from 'lucide-react'
import DetailTabs from '../../components/platform/DetailTabs'
import OperatingSurfaceLayout from '../../components/platform/OperatingSurfaceLayout'
import PlatformPageChrome, { PlatformRefreshButton, platformStatSubtitle } from '../../components/platform/PlatformPageChrome'
import { TahoeListEmpty, TahoeTableWrap, TahoeToolbar } from '../../components/platform/tahoe/TahoeListKit'
import { getFleetConsole, listAuditLogs, listPlatformEvents, type AuditLog, type FleetConsoleOverview, type PlatformEvent } from '../../api/platform'
import { formatUserError } from '../../utils/apiError'
import { statusBadgeClasses, statusBorderClass } from '../../utils/semanticColors'
import { useExpandable } from '../../hooks/useExpandable'
import { ExpandableToggle } from '../../components/ui/ExpandableToggle'

type SourceFilter = 'all' | 'audit' | 'event' | 'task'

const SOURCE_FILTERS: { id: SourceFilter; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'audit', label: 'Audit' },
  { id: 'event', label: 'Events' },
  { id: 'task', label: 'Tasks' },
]

function severityClass(severity: string) {
  const tone = severity === 'error' ? 'error' : severity === 'warn' ? 'warn' : 'neutral'
  return `border uppercase ${statusBadgeClasses(tone)} ${statusBorderClass(tone)}`
}

function formatTime(iso: string) {
  const d = new Date(iso)
  return d.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

export default function PlatformEvents({ embedded }: { embedded?: boolean } = {}) {
  const [fleet, setFleet] = useState<FleetConsoleOverview | null>(null)
  const [controllerAudit, setControllerAudit] = useState<AuditLog[]>([])
  const [platformEvents, setPlatformEvents] = useState<PlatformEvent[]>([])
  const [eventKind, setEventKind] = useState('')
  const [source, setSource] = useState<SourceFilter>('all')
  const [query, setQuery] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  // Last-response-wins guard: eventKind changes on every keystroke, so a slow
  // response for an earlier filter value could otherwise land after a newer
  // one and show stale filtered results.
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setError(null)
    try {
      const [console, audit, events] = await Promise.all([
        getFleetConsole(),
        listAuditLogs().catch(() => []),
        listPlatformEvents(eventKind || undefined).catch(() => []),
      ])
      if (!alive()) return
      setFleet(console)
      setControllerAudit(audit)
      setPlatformEvents(events)
    } catch (e: unknown) {
      if (!alive()) return
      setError(formatUserError(e))
      setFleet(null)
    } finally {
      if (alive()) setLoading(false)
    }
  }, [eventKind])

  useEffect(() => { void load() }, [load])

  const entries = useMemo(() => {
    const rows = fleet?.entries ?? []
    const q = query.trim().toLowerCase()
    return rows.filter((e) => {
      if (source !== 'all' && e.source !== source) return false
      if (!q) return true
      return (
        e.message.toLowerCase().includes(q)
        || e.action.toLowerCase().includes(q)
        || (e.actor?.toLowerCase().includes(q) ?? false)
      )
    })
  }, [fleet, source, query])

  const streamList = useExpandable(entries, 100)
  const auditList = useExpandable(controllerAudit, 100)

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      hideHeader={embedded}
      compact={embedded}
      loading={loading && !fleet && !error}
      error={error}
      onErrorRetry={() => void load()}
      title={embedded ? undefined : 'Logs & Audit'}
      subtitle={embedded ? undefined : (
        fleet
          ? (
            <span className="flex flex-col gap-1">
              <span className="text-[var(--text-muted)]">Unified fleet log tail — audit trail, platform events, and task failures in one stream.</span>
              {platformStatSubtitle([
                { label: '24h total', value: fleet.total_24h },
                { label: 'Audit', value: fleet.audit_24h },
                { label: 'Events', value: fleet.events_24h },
                { label: 'Failed tasks', value: fleet.tasks_failed_24h },
              ])}
            </span>
          )
          : 'Unified fleet log tail — audit trail, platform events, and task failures in one stream.'
      )}
      icon={embedded ? undefined : <Terminal className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={embedded ? undefined : <PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-4"
    >
      <OperatingSurfaceLayout testId="platform-events-page">
      {fleet && (
        <p className="text-sm text-[var(--text-muted)]">{fleet.summary}</p>
      )}

      <DetailTabs
        primary={SOURCE_FILTERS.map((f) => ({ id: f.id, label: f.label }))}
        active={source}
        onChange={setSource}
      />

      <TahoeToolbar
        search={query}
        onSearchChange={setQuery}
        placeholder="Filter messages…"
        trailing={
          <div className="flex flex-wrap items-center gap-2">
            <input
              id="platform-event-kind-filter"
              name="event_kind"
              className="input text-sm max-w-xs font-mono"
              aria-label="Filter by event kind"
              placeholder="Event kind (e.g. host.sync)"
              value={eventKind}
              onChange={(e) => setEventKind(e.target.value)}
            />
            <button type="button" className="btn-secondary text-xs" onClick={() => void load()}>
              Apply kind
            </button>
          </div>
        }
      />

      <section>
        <h2 className="text-sm font-semibold text-[var(--text-secondary)] mb-2">Platform events</h2>
        <p className="text-xs text-[var(--text-muted)] mb-3">GET /api/v1/events — controller event bus</p>
        {platformEvents.length === 0 ? (
          <TahoeListEmpty
            icon={ScrollText}
            title="No platform events"
            description="Events appear when controller operations occur — try clearing the kind filter."
          />
        ) : (
          <TahoeTableWrap>
            <table className="apple-table w-full text-sm" aria-label="Platform events">
              <thead>
                <tr>
                  <th scope="col">Time</th>
                  <th scope="col">Kind</th>
                  <th scope="col">Message</th>
                </tr>
              </thead>
              <tbody>
                {platformEvents.slice(0, 50).map((e) => (
                  <tr key={e.id}>
                    <td className="text-[var(--text-muted)] whitespace-nowrap font-mono text-xs">{formatTime(e.created_at)}</td>
                    <td><span className="text-[10px] uppercase px-1.5 py-0.5 rounded border border-white/[0.08] text-[var(--link)]">{e.kind}</span></td>
                    <td className="text-[var(--text-secondary)]">{e.message}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        )}
      </section>

      <section>
        <h2 className="text-sm font-semibold text-[var(--text-secondary)] mb-2">Log stream</h2>
        <p className="text-xs text-[var(--text-muted)] mb-3">Newest first — merged audit, events, and tasks.</p>
        {!fleet ? (
          <p className="text-sm text-[var(--text-muted)] py-8 text-center">Loading fleet console…</p>
        ) : entries.length === 0 ? (
          <TahoeListEmpty
            icon={ScrollText}
            title="No log entries match"
            description="Adjust the source tab or message filter to broaden the stream."
          />
        ) : (
          <TahoeTableWrap>
            <table className="apple-table w-full font-mono text-xs" aria-label="Fleet log stream">
              <thead>
                <tr>
                  <th scope="col">Time</th>
                  <th scope="col">Severity</th>
                  <th scope="col">Source</th>
                  <th scope="col">Message</th>
                </tr>
              </thead>
              <tbody id={streamList.listId}>
                {streamList.shown.map((e) => (
                  <tr key={`${e.source}-${e.id}`}>
                    <td className="text-[var(--text-muted)] whitespace-nowrap">{formatTime(e.created_at)}</td>
                    <td><span className={`px-1.5 py-0.5 rounded border uppercase text-[10px] tracking-wide ${severityClass(e.severity)}`}>{e.severity}</span></td>
                    <td className="text-orange-600/90 capitalize">{e.source}</td>
                    <td className="text-[var(--text-primary)] break-all">
                      {e.actor ? <span className="text-[var(--link)]">{e.actor} </span> : null}
                      <span className="text-[var(--text-muted)]">{e.action}</span>
                      {' — '}
                      {e.message}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        )}
        {streamList.showToggle && (
          <div className="pt-3">
            <ExpandableToggle expanded={streamList.expanded} hidden={streamList.hidden} listId={streamList.listId} onToggle={streamList.toggle} noun="entries" />
          </div>
        )}
      </section>

      {controllerAudit.length > 0 && (
        <section>
          <h2 className="text-sm font-semibold text-[var(--text-secondary)] mb-2">Controller audit log</h2>
          <p className="text-xs text-[var(--text-muted)] mb-3">GET /api/v1/audit — {controllerAudit.length} entries</p>
          <TahoeTableWrap>
            <table className="apple-table w-full font-mono text-xs" aria-label="Controller audit log">
              <thead>
                <tr>
                  <th scope="col">Time</th>
                  <th scope="col">Actor</th>
                  <th scope="col">Action</th>
                  <th scope="col">Resource</th>
                </tr>
              </thead>
              <tbody id={auditList.listId}>
                {auditList.shown.map((a) => (
                  <tr key={a.id}>
                    <td className="text-[var(--text-muted)] whitespace-nowrap">{formatTime(a.created_at)}</td>
                    <td className="text-[var(--link)]">{a.actor}</td>
                    <td className="text-[var(--text-muted)]">{a.action}</td>
                    <td className="text-[var(--text-muted)]">{a.resource_type ?? '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
          {auditList.showToggle && (
            <div className="pt-3">
              <ExpandableToggle expanded={auditList.expanded} hidden={auditList.hidden} listId={auditList.listId} onToggle={auditList.toggle} noun="entries" />
            </div>
          )}
        </section>
      )}
      </OperatingSurfaceLayout>
    </PlatformPageChrome>
  )
}
