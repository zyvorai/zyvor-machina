// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import { Siren, Sparkles } from 'lucide-react'
import PlatformPageChrome, { PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import { ackIncident, analyzeIncident, generatePostmortem, getActiveIncidents, getIncidentRoom, type ActiveIncident, type IncidentAnalysis } from '../../api/ai'
import { executeOpsRunbook } from '../../api/platform'
import RunbookExecutionSheet, { type RunbookExecutionResult } from '../../components/platform/RunbookExecutionSheet'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusToneClass } from '../../utils/semanticColors'

function inferRunbookIncident(incident: ActiveIncident): string {
  const text = `${incident.title} ${incident.summary}`.toLowerCase()
  if (text.includes('offline') || text.includes('host down') || text.includes('unreachable')) return 'host-offline'
  return 'host-offline'
}

export default function PlatformIncidentCommander() {
  const toast = useToastContext()
  const [incidents, setIncidents] = useState<ActiveIncident[]>([])
  const [selected, setSelected] = useState<string | null>(null)
  const [room, setRoom] = useState<Awaited<ReturnType<typeof getIncidentRoom>> | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [rcaHours, setRcaHours] = useState(4)
  const [fleetRca, setFleetRca] = useState<IncidentAnalysis | null>(null)
  const [rcaLoading, setRcaLoading] = useState(false)
  const [runbookBusy, setRunbookBusy] = useState(false)
  const [runbookResult, setRunbookResult] = useState<RunbookExecutionResult | null>(null)
  const [runbookSheetOpen, setRunbookSheetOpen] = useState(false)
  // Last-response-wins guard: clicking through incidents quickly can otherwise
  // let a slow room fetch for a previously-selected incident land after a
  // newer one, showing a war room that doesn't match the selected incident.
  const roomSeq = useRef(0)

  const loadRca = useCallback(async (hours = rcaHours) => {
    setRcaLoading(true)
    try {
      setFleetRca(await analyzeIncident({ hours }))
    } catch {
      setFleetRca(null)
    } finally {
      setRcaLoading(false)
    }
  }, [rcaHours])

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const [inc] = await Promise.all([
        getActiveIncidents(),
        loadRca(rcaHours),
      ])
      setIncidents(inc)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [loadRca, rcaHours])

  useEffect(() => { void load() }, [load])

  const openRoom = async (id: string) => {
    const seq = ++roomSeq.current
    const alive = () => seq === roomSeq.current
    setSelected(id)
    try {
      const r = await getIncidentRoom(id)
      if (alive()) setRoom(r)
    } catch (e: unknown) {
      if (alive()) setError(formatUserError(e))
    }
  }

  const downloadPostmortem = async (id: string) => {
    try {
      const pm = await generatePostmortem(id)
      const url = URL.createObjectURL(new Blob([pm.markdown], { type: 'text/markdown' }))
      const a = document.createElement('a')
      a.href = url
      a.download = `postmortem-${id.slice(0, 8)}.md`
      a.click()
      URL.revokeObjectURL(url)
      toast.success('Postmortem draft downloaded')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const acknowledge = async (id: string) => {
    try {
      await ackIncident(id)
      toast.success('Incident acknowledged')
      void load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const runPlaybook = async () => {
    if (!room || !selected) return
    const slug = inferRunbookIncident(room.incident)
    setRunbookBusy(true)
    try {
      const r = await executeOpsRunbook(slug, { incident_id: selected })
      setRunbookResult({
        title: r.title,
        summary: r.summary,
        steps: r.steps ?? [],
        commands: r.commands ?? [],
      })
      setRunbookSheetOpen(true)
      toast.success(r.summary)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setRunbookBusy(false)
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      title="Incident Commander"
      subtitle="Correlated war room for infrastructure outages"
      icon={<Siren className="w-6 h-6" />}
      loading={loading}
      error={error}
      actions={<PlatformRefreshButton onClick={() => void load()} />}
    >
      <MacGlassPanel
        title={`Fleet RCA (last ${rcaHours}h)`}
        subtitle="Correlated signals across the fleet — same engine as Mission Control"
        action={
          <div className="flex items-center gap-2">
            <select
              className="input text-xs"
              value={rcaHours}
              onChange={(e) => {
                const hours = Number(e.target.value)
                setRcaHours(hours)
                void loadRca(hours)
              }}
              aria-label="RCA window hours"
            >
              {[1, 4, 8, 12, 24].map((h) => (
                <option key={h} value={h}>{h}h</option>
              ))}
            </select>
            <button type="button" className="btn-secondary text-xs" disabled={rcaLoading} onClick={() => void loadRca()}>
              {rcaLoading ? 'Analyzing…' : 'Refresh'}
            </button>
          </div>
        }
      >
        {rcaLoading && !fleetRca && <p className="text-sm text-[var(--text-muted)]">Analyzing fleet signals…</p>}
        {fleetRca && (
          <div className="rounded-xl border border-orange-500/20 bg-orange-500/5 p-3 text-sm -mt-1">
            <p className="font-medium text-orange-700 flex items-center gap-2">
              <Sparkles className="w-4 h-4" /> Root cause ({Math.round(fleetRca.confidence * 100)}% confidence)
            </p>
            <p className="text-[var(--text-secondary)] mt-1">{fleetRca.root_cause}</p>
            {(fleetRca.evidence ?? []).slice(0, 4).map((ev) => (
              <p key={ev} className="text-xs text-[var(--text-muted)] mt-1">Evidence: {ev}</p>
            ))}
            {(fleetRca.suggested_actions ?? []).slice(0, 3).map((a) => (
              <p key={a} className={`text-xs mt-1 ${hubLinkClasses()}`}>→ {a}</p>
            ))}
            {(fleetRca.contributing_factors ?? []).length > 0 && (
              <ul className="mt-2 text-xs text-[var(--text-muted)] list-disc pl-4">
                {(fleetRca.contributing_factors ?? []).slice(0, 4).map((f) => (
                  <li key={f}>{f}</li>
                ))}
              </ul>
            )}
          </div>
        )}
        {!rcaLoading && !fleetRca && (
          <p className={`text-sm ${statusToneClass('warn')}`}>Fleet RCA unavailable — check controller connectivity.</p>
        )}
      </MacGlassPanel>

      <div className="flex flex-col gap-4 w-full">
        <MacGlassPanel title="Active incidents">
          <ul className="space-y-2 text-sm">
            {incidents.map((inc) => (
              <li key={inc.id} className="rounded-lg border border-white/10 p-3">
                <button type="button" className="text-left w-full" onClick={() => void openRoom(inc.id)}>
                  <p className="font-medium text-[var(--text-primary)]">{inc.title}</p>
                  <p className="text-xs text-[var(--text-muted)]">{inc.severity} · {inc.status}</p>
                  <p className="text-xs text-[var(--text-muted)] mt-1">{inc.summary}</p>
                </button>
                <button type="button" className={`text-xs mt-2 ${hubLinkClasses()}`} onClick={() => void acknowledge(inc.id)}>Acknowledge</button>
                <button type="button" className={`text-xs mt-2 ml-3 ${hubLinkClasses()}`} onClick={() => void downloadPostmortem(inc.id)}>Draft postmortem</button>
              </li>
            ))}
            {incidents.length === 0 && <p className="text-[var(--text-muted)]">No active incidents.</p>}
          </ul>
        </MacGlassPanel>

        {room && selected && (
          <MacGlassPanel title="War room">
            <p className="text-sm text-[var(--text-secondary)]">{room.incident.root_cause ?? room.incident.summary}</p>
            <p className="text-xs text-[var(--text-muted)] mt-2">{room.correlated_count} correlated signals · {room.pending_approvals} pending approvals</p>
            <Link to="/platform/zyra/approvals" className={`text-xs mt-2 inline-block ${hubLinkClasses()}`}>Open Zyra approvals →</Link>
            <div className="mt-4 space-y-2 max-h-48 overflow-y-auto text-xs font-mono">
              {room.timeline.slice(0, 10).map((e, i) => (
                <div key={`${e.at}-${i}`} className="text-[var(--text-muted)]">[{e.source}] {e.message}</div>
              ))}
            </div>
            {room.runbook_steps.length > 0 && (
              <div className="mt-4">
                <p className="text-xs font-medium text-[var(--text-muted)] mb-1">Runbook</p>
                <ol className="text-xs text-[var(--text-muted)] list-decimal list-inside space-y-1">
                  {room.runbook_steps.slice(0, 5).map((s) => <li key={s}>{s}</li>)}
                </ol>
                <div className="flex flex-wrap gap-2 mt-3">
                  <button type="button" className="btn-primary text-xs" disabled={runbookBusy} onClick={() => void runPlaybook()}>
                    {runbookBusy ? 'Running…' : 'Run playbook'}
                  </button>
                  <Link to="/platform/reports?tab=runbooks" className={`text-xs ${hubLinkClasses()}`}>
                    Full runbook catalog →
                  </Link>
                </div>
              </div>
            )}
          </MacGlassPanel>
        )}
      </div>
      <RunbookExecutionSheet
        open={runbookSheetOpen}
        onClose={() => setRunbookSheetOpen(false)}
        result={runbookResult}
      />
    </PlatformPageChrome>
  )
}
