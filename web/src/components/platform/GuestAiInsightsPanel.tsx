// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { Loader2, Sparkles } from 'lucide-react'
import {
  getVmGuestAiInsights,
  installGuestTools,
  type GuestAiInsightsReport,
} from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { GUEST_TOAST_CHANNEL_ATTACH } from '../../utils/guestAgentUx'
import { MacGlassPanel } from './mac/PlatformMacUi'
import type { RunGuestActionFn } from './GuestAgentDiagnosticsPanel'
import { statusPillClasses, statusToneClass } from '../../utils/semanticColors'

type Props = {
  vmId: string
  focus?: string
  autoLoad?: boolean
  onApplied?: () => void
  onRunAction?: RunGuestActionFn
}

function severityTone(s: string): 'ok' | 'warn' | 'error' | 'info' | 'neutral' {
  if (s === 'error') return 'error'
  if (s === 'warn') return 'warn'
  if (s === 'ok' || s === 'info') return 'info'
  return 'neutral'
}

/** Actions handled on the diagnostics panel — Apply only offers install and approval-gated ops. */
const DIAGNOSTICS_ACTIONS = new Set(['guest.sync_time', 'guest.fstrim'])

export default function GuestAiInsightsPanel({
  vmId,
  focus,
  autoLoad = false,
  onApplied,
  onRunAction,
}: Props) {
  const toast = useToastContext()
  const [loading, setLoading] = useState(false)
  const [report, setReport] = useState<GuestAiInsightsReport | null>(null)
  const [applying, setApplying] = useState<string | null>(null)

  const load = async (refresh = false) => {
    setLoading(true)
    try {
      setReport(await getVmGuestAiInsights(vmId, { refresh, focus }))
    } catch (e: unknown) {
      toast.error(formatUserError(e))
      setReport(null)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    if (autoLoad) void load(false)
    // eslint-disable-next-line react-hooks/exhaustive-deps -- load on mount / tab enter only
  }, [vmId, autoLoad])

  const applyAction = async (action: string) => {
    if (DIAGNOSTICS_ACTIONS.has(action)) return
    setApplying(action)
    try {
      if (action === 'guest.install_tools') {
        if (onRunAction) {
          await onRunAction('install', () => installGuestTools(vmId), GUEST_TOAST_CHANNEL_ATTACH)
        } else {
          await installGuestTools(vmId)
          toast.success(GUEST_TOAST_CHANNEL_ATTACH)
        }
        onApplied?.()
        await load(true)
        return
      }
      toast.error(`Action ${action} requires approval in Zyra hub`)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setApplying(null)
    }
  }

  return (
    <MacGlassPanel
      title="AI guest intelligence"
      subtitle="Guest-agent telemetry interpreted by Zyra"
      action={
        <button type="button" className="btn-secondary text-xs" disabled={loading} onClick={() => void load(true)}>
          {loading ? <Loader2 className="w-3 h-3 animate-spin inline" /> : <Sparkles className="w-3 h-3 inline" />}
          {report ? 'Refresh' : 'Ask AI'}
        </button>
      }
    >
      {!report && !loading && (
        <p className="text-sm text-[var(--text-muted)]">
          {autoLoad
            ? 'Insights will load automatically when this tab is open.'
            : 'Generate a natural-language summary with security and operations recommendations from live guest-agent data.'}
        </p>
      )}
      {loading && !report && (
        <p className="text-sm text-[var(--text-muted)] flex items-center gap-2">
          <Loader2 className="w-4 h-4 animate-spin" /> Analyzing guest telemetry…
        </p>
      )}
      {report && (
        <div className="space-y-3 text-sm">
          <p className="text-[var(--text-primary)]">{report.summary}</p>
          {report.llm_powered && (
            <span className={statusPillClasses('info')}>LLM-powered</span>
          )}
          {report.insights.length > 0 ? (
            <ul className="space-y-2">
              {report.insights.map((i) => (
                <li key={i.title} className="rounded-lg border border-white/[0.06] bg-[var(--apple-surface)] px-3 py-2">
                  <p className={`font-medium ${statusToneClass(severityTone(i.severity))}`}>{i.title}</p>
                  <p className="text-xs text-[var(--text-muted)] mt-0.5">{i.detail}</p>
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-xs text-[var(--text-muted)]">No specific insights — guest telemetry looks nominal.</p>
          )}
          {report.recommendations.length > 0 && (
            <div className="space-y-2">
              <p className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">Recommendations</p>
              {report.recommendations.map((r) => (
                <div key={r.label} className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-white/[0.06] px-3 py-2">
                  <div>
                    <p className="text-[var(--text-primary)]">{r.label}</p>
                    <p className="text-xs text-[var(--text-muted)]">{r.rationale}</p>
                  </div>
                  {r.action === 'guest.install_tools' && (
                    <button
                      type="button"
                      className="btn-secondary text-xs"
                      disabled={!!applying}
                      onClick={() => void applyAction(r.action)}
                    >
                      {applying === r.action ? <Loader2 className="w-3 h-3 animate-spin" /> : null}
                      Apply
                    </button>
                  )}
                  {!DIAGNOSTICS_ACTIONS.has(r.action) && r.action !== 'guest.install_tools' && r.action !== 'none' && (
                    <span className="text-[10px] text-[var(--text-muted)]">Zyra approval</span>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </MacGlassPanel>
  )
}
