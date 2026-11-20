// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { DollarSign } from 'lucide-react'
import PlatformPageChrome, { PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import { createZyraAction, getRightsizingReport, type RightsizingRecommendation } from '../../api/ai'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

export default function PlatformRightsizing() {
  const toast = useToastContext()
  const [recs, setRecs] = useState<RightsizingRecommendation[]>([])
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [savings, setSavings] = useState(0)
  const [idle, setIdle] = useState(0)
  const [oversized, setOversized] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [queueing, setQueueing] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const r = await getRightsizingReport()
      setRecs(r.recommendations)
      setSavings(r.estimated_monthly_savings_usd)
      setIdle(r.idle_vm_count)
      setOversized(r.oversized_vm_count)
      setSelected(new Set())
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const recKey = (r: RightsizingRecommendation) => `${r.vm_id}-${r.action}`

  const toggle = (r: RightsizingRecommendation) => {
    const k = recKey(r)
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(k)) next.delete(k)
      else next.add(k)
      return next
    })
  }

  const queueAction = async (r: RightsizingRecommendation) => {
    await createZyraAction({
      action_type: r.action,
      label: `${r.action} ${r.vm_name}`,
      review: r.detail,
      risk: r.risk,
      object_ref: { vm_id: r.vm_id, vm_name: r.vm_name },
      source: 'rightsizing',
    })
    toast.success(`Queued ${r.action} for ${r.vm_name}`)
  }

  const queueSingle = async (r: RightsizingRecommendation) => {
    try {
      await queueAction(r)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const queueSelected = async () => {
    const picks = recs.filter((r) => selected.has(recKey(r)))
    if (picks.length === 0) return
    setQueueing(true)
    try {
      await Promise.all(picks.map((r) => queueAction(r)))
      toast.success(`Queued ${picks.length} approval(s)`)
      setSelected(new Set())
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setQueueing(false)
    }
  }

  const selectAll = () => {
    setSelected(new Set(recs.map(recKey)))
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      title="VM Rightsizing"
      subtitle="FinOps recommendations from fleet metrics"
      icon={<DollarSign className="w-6 h-6" />}
      loading={loading}
      error={error}
      actions={<PlatformRefreshButton onClick={() => void load()} />}
    >
      <MacGlassPanel title="Summary">
        <p className="text-sm text-[var(--text-muted)]">
          {oversized} oversized · {idle} idle · est. ${savings.toFixed(0)}/mo savings
        </p>
        {recs.length > 0 && (
          <div className="flex flex-wrap gap-2 mt-3">
            <button type="button" className="btn-secondary text-xs" onClick={selectAll}>Select all</button>
            <button
              type="button"
              className="btn-primary text-xs"
              disabled={selected.size === 0 || queueing}
              onClick={() => void queueSelected()}
            >
              {queueing ? 'Queueing…' : `Queue ${selected.size} for approval`}
            </button>
          </div>
        )}
      </MacGlassPanel>
      <MacGlassPanel title="Recommendations" className="mt-4">
        <ul className="text-sm space-y-3">
          {recs.map((r) => {
            const k = recKey(r)
            const checked = selected.has(k)
            return (
              <li key={k} className="border-b border-white/5 pb-2">
                <label className="flex items-start gap-2 cursor-pointer">
                  <input type="checkbox" className="mt-1" checked={checked} onChange={() => toggle(r)} />
                  <span className="flex-1">
                    <p className="font-medium text-[var(--text-primary)]">{r.vm_name}</p>
                    <p className="text-xs text-[var(--text-muted)]">{r.detail}</p>
                    <p className="text-xs text-[var(--text-muted)] mt-1">{r.action} · risk {r.risk}{r.savings_usd > 0 ? ` · $${r.savings_usd.toFixed(0)}/mo` : ''}</p>
                  </span>
                </label>
                <button type="button" className="text-xs text-orange-400 mt-2 hover:underline ml-6" onClick={() => void queueSingle(r)}>
                  Queue single
                </button>
              </li>
            )
          })}
          {recs.length === 0 && <li className="text-[var(--text-muted)]">No rightsizing opportunities detected.</li>}
        </ul>
      </MacGlassPanel>
    </PlatformPageChrome>
  )
}
