// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState, useEffect } from 'react'
import { Play, Rocket } from 'lucide-react'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import { executeMissionStack, getMissionStackStatus, planMissionStack, type MissionStackPlan } from '../../api/ai'
import { formatUserError } from '../../utils/apiError'
import { statusToneClass } from '../../utils/semanticColors'

export default function MachinaMissionStack() {
  const [query, setQuery] = useState('Build a GPU cluster for Llama serving')
  const [plan, setPlan] = useState<MissionStackPlan | null>(null)
  const [executeSummary, setExecuteSummary] = useState<string | null>(null)
  const [stackStatus, setStackStatus] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const refreshStatus = async () => {
    try {
      const s = await getMissionStackStatus()
      setStackStatus(s.summary)
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }

  useEffect(() => { void refreshStatus() }, [])

  const run = async () => {
    setBusy(true)
    setExecuteSummary(null)
    setError(null)
    try {
      setPlan(await planMissionStack(query))
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const previewExecute = async () => {
    setBusy(true)
    setError(null)
    try {
      const r = await executeMissionStack(query, true)
      setPlan(r.plan)
      setExecuteSummary(r.summary)
      await refreshStatus()
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <MacGlassPanel title="AI Mission Control" subtitle="NL stack builder — GPU, Kubernetes, inference">
      <div className="flex flex-wrap gap-2">
        <input aria-label="AI mission query" className="input flex-1 text-sm min-w-[12rem]" value={query} onChange={(e) => setQuery(e.target.value)} />
        <button type="button" className="btn-primary text-xs flex items-center gap-1" disabled={busy} onClick={() => void run()}>
          <Rocket className="w-3 h-3" /> Plan
        </button>
        <button type="button" className="btn-secondary text-xs flex items-center gap-1" disabled={busy} onClick={() => void previewExecute()}>
          <Play className="w-3 h-3" /> Preview infra
        </button>
      </div>
      {error && <p className={`text-xs mt-2 ${statusToneClass('error')}`}>{error}</p>}
      {executeSummary && <p className={`text-xs mt-2 ${statusToneClass('ok')}`}>{executeSummary}</p>}
      {stackStatus && <p className="text-xs text-[var(--text-muted)] mt-1">Stack status: {stackStatus}</p>}
      {plan && (
        <div className="mt-3 text-xs space-y-2 text-[var(--text-muted)]">
          <p className="text-[var(--text-primary)] font-medium">{plan.label}</p>
          <p>{plan.review}</p>
          {plan.phases.map((ph) => (
            <div key={ph.name}>
              <p className="text-orange-600/90 font-medium">{ph.name} {ph.automated ? '(automated)' : '(review)'}</p>
              <ul className="ml-3">{ph.steps.map((s) => <li key={s}>• {s}</li>)}</ul>
            </div>
          ))}
        </div>
      )}
    </MacGlassPanel>
  )
}
