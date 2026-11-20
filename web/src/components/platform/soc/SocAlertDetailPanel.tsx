// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { getSocAlert, patchSocAlert, type SocAlert, type SocAlertDetail } from '../../../api/soc'
import { useToastContext } from '../../../contexts/ToastContext'
import { formatUserError } from '../../../utils/apiError'
import { statusBadgeClasses, statusToneClass } from '../../../utils/semanticColors'
import { MacGlassPanel } from '../mac/PlatformMacUi'

function severityTone(sev: string): 'error' | 'warn' | 'ok' | 'neutral' {
  const s = sev.toLowerCase()
  if (s === 'critical' || s === 'high') return 'error'
  if (s === 'medium') return 'warn'
  if (s === 'low') return 'ok'
  return 'neutral'
}

export default function SocAlertDetailPanel({
  alert,
  onUpdated,
}: {
  alert: SocAlert
  onUpdated: () => void
}) {
  const toast = useToastContext()
  const [detail, setDetail] = useState<SocAlertDetail | null>(null)
  const [loading, setLoading] = useState(true)
  const [assignee, setAssignee] = useState(alert.assigned_to ?? '')

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const d = await getSocAlert(alert.id)
      setDetail(d)
      setAssignee(d.assigned_to ?? '')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
      setDetail(null)
    } finally {
      setLoading(false)
    }
  }, [alert.id, toast])

  useEffect(() => { void load() }, [load])

  const patch = async (body: { status?: string; assigned_to?: string }) => {
    try {
      await patchSocAlert(alert.id, body)
      toast.success('Alert updated')
      await load()
      onUpdated()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  if (loading && !detail) {
    return <MacGlassPanel title="Alert detail"><p className="text-sm text-[var(--text-muted)] p-3">Loading…</p></MacGlassPanel>
  }

  if (!detail) {
    return <MacGlassPanel title="Alert detail"><p className="text-sm text-[var(--text-muted)] p-3">Could not load alert.</p></MacGlassPanel>
  }

  return (
    <MacGlassPanel title="Alert detail">
      <div className="p-3 space-y-4 text-sm">
        <div>
          <p className="font-medium text-[var(--text-primary)]">{detail.title}</p>
          <p className="text-xs text-[var(--text-muted)] mt-1">
            <span className={statusBadgeClasses(severityTone(detail.severity))}>{detail.severity}</span>
            {' · '}
            <span className={statusToneClass(detail.status === 'open' ? 'warn' : detail.status === 'closed' ? 'neutral' : 'ok')}>
              {detail.status}
            </span>
            {detail.rule_name && <> · rule: {detail.rule_name}</>}
          </p>
        </div>

        <div className="grid gap-2 sm:grid-cols-2 text-xs text-[var(--text-muted)]">
          <span>First seen: {new Date(detail.first_seen).toLocaleString()}</span>
          <span>Last seen: {new Date(detail.last_seen).toLocaleString()}</span>
          <span>Events: {detail.event_count}</span>
          <span className="truncate" title={detail.dedupe_key}>Dedupe: {detail.dedupe_key}</span>
        </div>

        <label className="block">
          <span className="text-[var(--text-muted)] text-xs">Assigned to</span>
          <div className="flex gap-2 mt-1">
            <input
              className="input flex-1 text-sm"
              value={assignee}
              onChange={(e) => setAssignee(e.target.value)}
              placeholder="analyst@team"
            />
            <button type="button" className="btn-secondary text-xs" onClick={() => void patch({ assigned_to: assignee })}>
              Save
            </button>
          </div>
        </label>

        <div className="flex flex-wrap gap-2">
          {detail.status === 'open' && (
            <button type="button" className="btn-secondary text-xs" onClick={() => void patch({ status: 'acknowledged' })}>
              Acknowledge
            </button>
          )}
          {detail.status !== 'closed' && (
            <button type="button" className="btn-secondary text-xs" onClick={() => void patch({ status: 'closed' })}>
              Close
            </button>
          )}
        </div>

        {(detail.mitre_tags?.length ?? 0) > 0 && (
          <div>
            <p className="text-xs text-[var(--text-muted)] mb-1">MITRE ATT&CK</p>
            <div className="flex flex-wrap gap-1">
              {detail.mitre_tags!.map((t) => (
                <span key={`${t.id}-${t.name}`} className={statusBadgeClasses('info')}>
                  {t.id}{t.name !== t.id ? ` · ${t.name}` : ''}
                </span>
              ))}
            </div>
          </div>
        )}

        {Object.keys(detail.detail_json ?? {}).length > 0 && (
          <div>
            <p className="text-xs text-[var(--text-muted)] mb-1">Detection context</p>
            <pre className="text-xs bg-[var(--apple-fill-tertiary)] rounded p-2 overflow-auto max-h-28 text-[var(--text-secondary)]">
              {JSON.stringify(detail.detail_json, null, 2)}
            </pre>
          </div>
        )}

        <div>
          <p className="text-xs text-[var(--text-muted)] mb-1">Linked events ({detail.linked_events?.length ?? 0})</p>
          {(detail.linked_events?.length ?? 0) === 0 ? (
            <p className="text-xs text-[var(--text-muted)]">No linked events stored for this alert.</p>
          ) : (
            <ul className="divide-y divide-white/5 max-h-48 overflow-auto">
              {detail.linked_events.map((e) => (
                <li key={e.id} className="py-2">
                  <p className="text-[var(--text-primary)] truncate">{e.summary}</p>
                  <p className={`text-[10px] mt-0.5 ${statusToneClass(severityTone(e.severity))}`}>
                    {e.source} · {e.severity} · {new Date(e.occurred_at).toLocaleString()}
                  </p>
                </li>
              ))}
            </ul>
          )}
        </div>

        {(detail.playbook_runs?.length ?? 0) > 0 && (
          <div>
            <p className="text-xs text-[var(--text-muted)] mb-1">Playbook runs</p>
            <ul className="space-y-1 text-xs">
              {detail.playbook_runs!.map((r) => (
                <li key={r.id} className="text-[var(--text-muted)]">
                  {r.playbook_name ?? r.playbook_id.slice(0, 8)} —{' '}
                  <span className={statusToneClass(r.status === 'completed' ? 'ok' : r.status === 'failed' ? 'error' : 'neutral')}>
                    {r.status}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </MacGlassPanel>
  )
}
