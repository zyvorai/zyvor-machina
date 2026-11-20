// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { Sparkles } from 'lucide-react'
import { getFleetSummary } from '../../../api/ai'
import { ZYRA_ASSISTANT_NAME } from '../../../config/aiBrand'
import { statusChipClasses } from '../../../utils/semanticColors'
import { dispatchOpenSpotlight } from '../../../utils/platformJarvisShell'
import type { MachineFinderState } from './useMachineFinder'

type Props = {
  state: MachineFinderState
}

export default function MachineFinderBriefing({ state }: Props) {
  const [summaryText, setSummaryText] = useState<string | null>(null)
  const [alerts, setAlerts] = useState<string[]>([])
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    setLoading(true)
    void getFleetSummary()
      .then((r) => {
        setSummaryText(
          `${r.aggregate_vm_count} VMs across ${r.reachable_peers}/${r.peer_count} peers · $${r.aggregate_monthly_usd.toFixed(0)}/mo est.`,
        )
        setAlerts([])
      })
      .catch(() => {
        setSummaryText(null)
        setAlerts([])
      })
      .finally(() => setLoading(false))
  }, [state.vms.length, state.finder?.summary])

  return (
    <section className="machine-finder-briefing rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] px-4 py-3 shadow-[var(--shadow-1)]" data-testid="machine-finder-briefing">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
        <div className="flex items-start gap-2 min-w-0">
          <Sparkles className="w-4 h-4 text-[var(--machina-accent-ai)] mt-0.5 shrink-0" />
          <div className="min-w-0">
            <p className="text-xs font-medium text-[var(--machina-accent-ai)]">{ZYRA_ASSISTANT_NAME} Briefing</p>
            <p className="text-sm text-[var(--text-secondary)] mt-0.5">
              {loading ? 'Scanning fleet…' : summaryText ?? state.finder?.summary ?? 'Fleet overview loading…'}
            </p>
          </div>
        </div>
        {alerts.length > 0 && (
          <div className="flex flex-wrap gap-1.5 shrink-0">
            {alerts.slice(0, 4).map((alert) => (
              <button
                key={alert}
                type="button"
                className={`text-xs px-2 py-1 rounded-full border border-white/10 ${statusChipClasses('warn')}`}
                onClick={() => dispatchOpenSpotlight(alert)}
              >
                {alert}
              </button>
            ))}
          </div>
        )}
      </div>
    </section>
  )
}
