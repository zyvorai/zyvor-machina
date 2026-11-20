// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useMemo, useRef, useState } from 'react'
import { GitBranch } from 'lucide-react'
import { getInfraGraphAt } from '../../api/ai'
import { formatUserError } from '../../utils/apiError'
import { statusToneClass } from '../../utils/semanticColors'

type InfraGraphDiff = {
  summary: string
  added: string[]
  removed: string[]
  nodeDelta: number
}

export default function InfraGraphScrubberPanel({
  hours = 4,
  showReplayWindow = true,
}: {
  hours?: number
  showReplayWindow?: boolean
}) {
  const [replayHours, setReplayHours] = useState(hours)
  const [scrubPct, setScrubPct] = useState(100)
  const [graphDiff, setGraphDiff] = useState<InfraGraphDiff | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // Freeze the window end per replay-window change — Date.now() in render made
  // scrubTs new every commit, re-arming the debounced fetch forever (perpetual
  // self-polling the user never triggered).
  const windowEnd = useMemo(() => Date.now(), [replayHours])
  const windowStart = windowEnd - replayHours * 3600_000
  const scrubTs = new Date(windowStart + ((windowEnd - windowStart) * scrubPct) / 100).toISOString()

  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current)
    debounceRef.current = setTimeout(() => {
      setBusy(true)
      setError(null)
      void getInfraGraphAt(scrubTs)
        .then((at) => {
          setGraphDiff({
            summary: at.diff_summary,
            added: at.added_nodes ?? [],
            removed: at.removed_nodes ?? [],
            nodeDelta: at.node_delta ?? 0,
          })
        })
        .catch((e: unknown) => {
          setGraphDiff(null)
          setError(formatUserError(e))
        })
        .finally(() => setBusy(false))
    }, 300)
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current)
    }
  }, [scrubTs, replayHours])

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-4 text-xs text-[var(--text-muted)]">
        {showReplayWindow && (
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
        )}
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
          {busy && <span className="text-[var(--text-faint)]">…</span>}
        </label>
      </div>
      {error && <p className={`text-xs ${statusToneClass('error')}`}>{error}</p>}
      {graphDiff && (
        <div className="rounded-lg border border-[var(--accent)]/40 bg-[var(--accent)]/5 p-2 text-xs space-y-1">
          <p className="text-[var(--accent)] flex items-center gap-1"><GitBranch className="w-3.5 h-3.5" /> Graph at scrubber</p>
          <p className="text-[var(--text-muted)]">{graphDiff.summary}</p>
          <p className="text-[var(--text-muted)]">Node delta: {graphDiff.nodeDelta >= 0 ? '+' : ''}{graphDiff.nodeDelta}</p>
          {graphDiff.added.length > 0 && <p className="text-emerald-400/90">+ {graphDiff.added.join(', ')}</p>}
          {graphDiff.removed.length > 0 && <p className="text-amber-400/90">− {graphDiff.removed.join(', ')}</p>}
        </div>
      )}
    </div>
  )
}
