// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Sparkles } from 'lucide-react'
import { analyzeIncident, type IncidentAnalysis } from '../../api/ai'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusToneClass } from '../../utils/semanticColors'

export default function MachinaVmOutageRca({ vmId, vmName }: { vmId: string; vmName?: string }) {
  const [analysis, setAnalysis] = useState<IncidentAnalysis | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const run = async () => {
    setLoading(true)
    setError(null)
    try {
      setAnalysis(await analyzeIncident({ hours: 4, vm_id: vmId, vm_name: vmName }))
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="rounded-xl border border-orange-500/20 bg-orange-500/5 p-4 space-y-2">
      <div className="flex items-center justify-between gap-2">
        <p className="font-medium text-orange-700 flex items-center gap-2 text-sm">
          <Sparkles className="w-4 h-4" /> Explain outage
        </p>
        <button type="button" className="btn-secondary text-xs" disabled={loading} onClick={() => void run()}>
          {loading ? 'Analyzing…' : 'Run RCA'}
        </button>
      </div>
      {error && <p className={`text-xs ${statusToneClass('error')}`}>{error}</p>}
      {analysis && (
        <>
          <p className="text-sm text-[var(--text-secondary)]">{analysis.root_cause}</p>
          <p className="text-xs text-[var(--text-muted)]">{Math.round(analysis.confidence * 100)}% confidence</p>
          {(analysis.evidence ?? []).slice(0, 3).map((ev) => (
            <p key={ev} className="text-xs text-[var(--text-muted)]">Evidence: {ev}</p>
          ))}
          {(analysis.suggested_actions ?? []).slice(0, 2).map((a) => (
            <p key={a} className={`text-xs ${hubLinkClasses()}`}>→ {a}</p>
          ))}
        </>
      )}
    </div>
  )
}
