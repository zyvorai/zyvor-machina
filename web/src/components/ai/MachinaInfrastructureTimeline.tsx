// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Clock, GitBranch, Sparkles } from 'lucide-react'
import { analyzeIncident, getInfraGraphAt, getTimelineReplay, type IncidentAnalysis } from '../../api/ai'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusToneClass } from '../../utils/semanticColors'

export default function MachinaInfrastructureTimeline({ hours = 4 }: { hours?: number }) {
  const [analysis, setAnalysis] = useState<IncidentAnalysis | null>(null)
  const [replayHours, setReplayHours] = useState(hours)
  const [scrubPct, setScrubPct] = useState(100)
  const [graphChanges, setGraphChanges] = useState<string[]>([])
  const [graphDiff, setGraphDiff] = useState<{ summary: string; added: string[]; removed: string[]; nodeDelta: number } | null>(null)
  const [error, setError] = useState<string | null>(null)
  // Dragging the replay/scrub sliders re-fires `load` on every tick with no
  // debounce; without a request-id guard a slower older response could land
  // after a newer one and overwrite the graph diff for a scrub position the
  // user has already moved past.
  const reqRef = useRef(0)

  // Freeze the window end per replay-window change. Computing Date.now() in the
  // render body made windowStart/scrubTs new every render, so `load` (and its
  // effect) fired on every commit → an infinite refetch loop hammering the API.
  const windowEnd = useMemo(() => Date.now(), [replayHours])
  const windowStart = windowEnd - replayHours * 3600_000
  const scrubTs = new Date(windowStart + ((windowEnd - windowStart) * scrubPct) / 100).toISOString()

  const load = useCallback(async () => {
    const reqId = ++reqRef.current
    setError(null)
    try {
      const fromIso = new Date(windowStart).toISOString()
      const toIso = new Date(windowEnd).toISOString()
      const [incident, replay, at] = await Promise.all([
        analyzeIncident({ hours: replayHours }),
        getTimelineReplay(fromIso, toIso),
        getInfraGraphAt(scrubTs).catch(() => null),
      ])
      if (reqRef.current !== reqId) return // a newer slider position was selected while this was in flight
      setAnalysis(incident)
      setGraphChanges(replay.graph_changes ?? [])
      if (at) {
        setGraphDiff({
          summary: at.diff_summary,
          added: at.added_nodes ?? [],
          removed: at.removed_nodes ?? [],
          nodeDelta: at.node_delta ?? 0,
        })
      }
    } catch (e: unknown) {
      if (reqRef.current !== reqId) return
      setError(formatUserError(e))
    }
  }, [replayHours, scrubTs, windowStart, windowEnd])

  useEffect(() => { void load() }, [load])

  if (error) return <p className={`text-xs ${statusToneClass('error')}`}>{error}</p>
  if (!analysis) return <p className="text-[var(--text-muted)] text-xs">Loading infrastructure timeline…</p>

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-4 text-xs text-[var(--text-muted)]">
        <label className="flex items-center gap-2">
          Replay window
          <input
            type="range"
            min={1}
            max={24}
            value={replayHours}
            onChange={(e) => setReplayHours(Number(e.target.value))}
            className="w-24"
          />
          <span>{replayHours}h</span>
        </label>
        <label className="flex items-center gap-2">
          Time scrubber
          <input
            type="range"
            min={0}
            max={100}
            value={scrubPct}
            onChange={(e) => setScrubPct(Number(e.target.value))}
            className="w-32"
          />
          <span>{new Date(scrubTs).toLocaleTimeString()}</span>
        </label>
      </div>
      <div className="rounded-xl border border-orange-500/20 bg-orange-500/5 p-3 text-sm">
        <p className="font-medium text-orange-700 flex items-center gap-2">
          <Sparkles className="w-4 h-4" /> AI Root Cause ({Math.round(analysis.confidence * 100)}% confidence)
        </p>
        <p className="text-[var(--text-secondary)] mt-1">{analysis.root_cause}</p>
        {(analysis.evidence ?? []).slice(0, 3).map((ev) => (
          <p key={ev} className="text-xs text-[var(--text-muted)] mt-1">Evidence: {ev}</p>
        ))}
        {(analysis.suggested_actions ?? []).slice(0, 2).map((a) => (
          <p key={a} className={`text-xs mt-1 ${hubLinkClasses()}`}>→ {a}</p>
        ))}
      </div>
      {graphDiff && (
        <div className="rounded-lg border border-[var(--accent)]/40 bg-[var(--accent)]/5 p-2 text-xs space-y-1">
          <p className="text-[var(--accent)] flex items-center gap-1"><GitBranch className="w-3.5 h-3.5" /> Graph at scrubber</p>
          <p className="text-[var(--text-muted)]">{graphDiff.summary}</p>
          <p className="text-[var(--text-muted)]">Node delta: {graphDiff.nodeDelta >= 0 ? '+' : ''}{graphDiff.nodeDelta}</p>
          {graphDiff.added.length > 0 && <p className="text-emerald-400/90">+ {graphDiff.added.join(', ')}</p>}
          {graphDiff.removed.length > 0 && <p className="text-amber-400/90">− {graphDiff.removed.join(', ')}</p>}
        </div>
      )}
      {graphChanges.length > 0 && (
        <div className="rounded-lg border border-white/5 bg-[var(--apple-surface)] p-2 text-xs">
          <p className="text-[var(--text-muted)] mb-1">Graph changes in window</p>
          {graphChanges.slice(0, 4).map((c) => (
            <p key={c} className="text-[var(--text-muted)] font-mono">{c}</p>
          ))}
        </div>
      )}
      <div className="max-h-48 overflow-y-auto space-y-1.5 text-xs font-mono">
        {analysis.timeline.slice(0, 12).map((e, i) => (
          <div key={`${e.at}-${i}`} className="flex gap-2 text-[var(--text-muted)]">
            <Clock className="w-3 h-3 shrink-0 mt-0.5 opacity-60" />
            <span className="text-[var(--text-muted)] shrink-0">{new Date(e.at).toLocaleTimeString()}</span>
            <span className="text-[var(--text-faint)]">[{e.source}]</span>
            <span className="text-[var(--text-secondary)]">{e.message}</span>
          </div>
        ))}
      </div>
    </div>
  )
}
