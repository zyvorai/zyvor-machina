// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, useCallback, useRef } from 'react'
import { getAuditLog, exportAuditNdjson, AuditEvent } from '../api/extras'
import { useTranslation } from 'react-i18next'
import { useToastContext } from '../contexts/ToastContext'
import { FileText, RefreshCw, Search, CheckCircle, XCircle, Download, X } from 'lucide-react'
import { downloadJSON, downloadCSV } from '../utils/export'
import { formatUserError } from '../utils/apiError'
import { statusToneClass } from '../utils/semanticColors'
import PageLayout from '../components/PageLayout'
import { useExpandable } from '../hooks/useExpandable'
import { ExpandableToggle } from '../components/ui/ExpandableToggle'

export default function AuditLogPage() {
  const [events, setEvents] = useState<AuditEvent[]>([])
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [actionInp, setActionInp] = useState('')
  const [actorInp, setActorInp] = useState('')
  const [qInp, setQInp] = useState('')
  const toast = useToastContext()
  const { t } = useTranslation()
  // Server-filtered but not paginated (limit: 8000 below); render a page at a time instead of
  // up to 8000 rows at once.
  const { shown: shownEvents, hidden, expanded, toggle, listId, showToggle } = useExpandable(events, 100)
  // Last-response-wins: a slow response for a previous filter combination
  // can't overwrite the results of a filter change made after it.
  const loadSeq = useRef(0)

  const fetchLog = useCallback(async () => {
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    try {
      setLoading(true)
      setLoadError(null)
      const data = await getAuditLog({
        action: actionInp.trim() || undefined,
        actor: actorInp.trim() || undefined,
        q: qInp.trim() || undefined,
        limit: 8000,
      })
      if (alive()) setEvents(data)
    } catch (e: unknown) {
      if (!alive()) return
      const msg = formatUserError(e)
      setLoadError(msg)
      toast.error(`Failed to load audit log: ${msg}`)
    } finally {
      if (alive()) setLoading(false)
    }
  }, [toast, actionInp, actorInp, qInp])

  useEffect(() => {
    const t = window.setTimeout(() => {
      void fetchLog()
    }, 450)
    return () => clearTimeout(t)
  }, [fetchLog])

  return (
    <PageLayout
      eyebrow="Security"
      title="Audit Log"
      icon={<FileText className={`w-6 h-6 ${statusToneClass('info')}`} />}
      subtitle={`${events.length} events (server-filtered)`}
      actions={
        <>
          <button type="button" onClick={() => { setActionInp('fleet_cloud'); setQInp('') }}
            className="px-3 py-1.5 text-xs rounded-lg border border-[var(--accent)]/40 text-[var(--link)] hover:bg-[var(--accent-soft)]">
            Fleet Cloud only
          </button>
          <button type="button" onClick={() => void fetchLog()} className="px-3 py-1.5 text-xs rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)] hover:bg-[var(--surface-hover)] text-[var(--text-primary)] transition flex items-center gap-1.5">
            <RefreshCw className="w-3.5 h-3.5" /> Refresh now
          </button>
          <button type="button" onClick={() => downloadJSON(events, 'audit-log.json')} className="p-2 hover:bg-[var(--surface-hover)] rounded-lg transition" title="Export JSON" aria-label="Export JSON"><Download className="w-4 h-4" /></button>
          <button type="button" onClick={() => downloadCSV(events as unknown as Record<string, unknown>[], 'audit-log.csv')} className="p-2 hover:bg-[var(--surface-hover)] rounded-lg transition" title="Export CSV" aria-label="Export CSV"><Download className={`w-4 h-4 ${statusToneClass('ok')}`} /></button>
          <button type="button" onClick={() => void exportAuditNdjson()} className="px-3 py-1.5 text-xs rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)] hover:bg-[var(--surface-hover)] text-[var(--text-primary)]" title={t('audit.exportNdjson')}>
            {t('audit.exportNdjson')}
          </button>
        </>
      }
      error={loadError}
      errorTitle="Could not load audit log"
      onErrorRetry={() => void fetchLog()}
    >
      <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
        <div className="relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-[var(--text-muted)]" />
          <input
            type="text"
            aria-label="Filter by action"
            placeholder="Action contains…"
            value={actionInp}
            onChange={(e) => setActionInp(e.target.value)}
            className={`w-full pl-10 py-2 bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-lg text-sm focus:outline-none focus:border-[var(--accent)] focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--accent)_45%,transparent)] ${actionInp ? 'pr-8' : 'pr-4'}`}
          />
          {actionInp && (
            <button type="button" aria-label="Clear action filter" onClick={() => setActionInp('')}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-[var(--text-muted)] hover:text-[var(--text-primary)]">
              <X className="w-4 h-4" />
            </button>
          )}
        </div>
        <div className="relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-[var(--text-muted)]" />
          <input
            type="text"
            aria-label="Filter by actor"
            placeholder="Actor contains…"
            value={actorInp}
            onChange={(e) => setActorInp(e.target.value)}
            className={`w-full pl-10 py-2 bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-lg text-sm focus:outline-none focus:border-[var(--accent)] focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--accent)_45%,transparent)] ${actorInp ? 'pr-8' : 'pr-4'}`}
          />
          {actorInp && (
            <button type="button" aria-label="Clear actor filter" onClick={() => setActorInp('')}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-[var(--text-muted)] hover:text-[var(--text-primary)]">
              <X className="w-4 h-4" />
            </button>
          )}
        </div>
        <div className="relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-[var(--text-muted)]" />
          <input
            type="text"
            aria-label="Search all fields"
            placeholder="Any field (action, target, result, actor)…"
            value={qInp}
            onChange={(e) => setQInp(e.target.value)}
            className={`w-full pl-10 py-2 bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-lg text-sm focus:outline-none focus:border-[var(--accent)] focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--accent)_45%,transparent)] ${qInp ? 'pr-8' : 'pr-4'}`}
          />
          {qInp && (
            <button type="button" aria-label="Clear search" onClick={() => setQInp('')}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-[var(--text-muted)] hover:text-[var(--text-primary)]">
              <X className="w-4 h-4" />
            </button>
          )}
        </div>
      </div>
      <p className="text-xs text-[var(--text-muted)]">Filters reload after a short pause (debounced).</p>

      {loading ? (
        <div className="flex items-center justify-center h-32" aria-busy="true" aria-label="Loading audit log">
          <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-[var(--accent)]" />
        </div>
      ) : (
        <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden overflow-x-auto">
          <table className="w-full min-w-[56rem]" aria-label="Audit log">
            <thead><tr className="border-b border-[var(--apple-hairline)] text-left text-sm text-[var(--text-muted)]"><th scope="col" className="px-6 py-3">Time</th><th scope="col" className="px-6 py-3">Action</th><th scope="col" className="px-6 py-3">Target</th><th scope="col" className="px-6 py-3">Actor</th><th scope="col" className="px-6 py-3">Result</th></tr></thead>
            <tbody id={listId} className="divide-y divide-[var(--apple-hairline)]/30">
              {shownEvents.map((e) => (
                <tr key={`${e.timestamp}-${e.action}-${e.target}`} className="table-row-hover">
                  <td className="px-6 py-2 text-xs text-[var(--text-muted)] font-mono whitespace-nowrap">{e.timestamp}</td>
                  <td className="px-6 py-2 text-sm font-medium">{e.action}</td>
                  <td className="px-6 py-2 text-sm text-[var(--text-secondary)]">{e.target}</td>
                  <td className="px-6 py-2 text-sm text-[var(--text-muted)] font-mono">{e.actor?.trim() ? e.actor : '—'}</td>
                  <td className="px-6 py-2 text-sm">
                    {e.result.includes('ok') || e.result.includes('success')
                      ? <span className={`flex items-center gap-1 ${statusToneClass('ok')}`}><CheckCircle className="w-3 h-3" />{e.result}</span>
                      : <span className={`flex items-center gap-1 ${statusToneClass('error')}`}><XCircle className="w-3 h-3" />{e.result}</span>
                    }
                  </td>
                </tr>
              ))}
              {events.length === 0 && <tr><td colSpan={5} className="px-6 py-8 text-center text-[var(--text-muted)]">No audit events match these filters</td></tr>}
            </tbody>
          </table>
          {showToggle && (
            <div className="px-6 py-3 border-t border-[var(--apple-hairline)]">
              <ExpandableToggle expanded={expanded} hidden={hidden} listId={listId} onToggle={toggle} noun="events" />
            </div>
          )}
        </div>
      )}
    </PageLayout>
  )
}
