// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { AlertTriangle, CheckCheck, Lightbulb } from 'lucide-react'
import { MacGlassPanel } from './mac/PlatformMacUi'
import { acknowledgeTaskFailures, getTaskFailures, type TaskFailureSummary } from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

type Props = {
  /** Filters the task list below to this operation. */
  onShowOperation?: (operation: string) => void
  /** Called after failures were acknowledged, so the page can reload. */
  onChanged?: () => void
}

/** Failed tasks of the last 24 hours grouped by cause, each with what to do about it. */
export default function TaskFailuresPanel({ onShowOperation, onChanged }: Props) {
  const toast = useToastContext()
  const [summary, setSummary] = useState<TaskFailureSummary | null>(null)
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    try { setSummary(await getTaskFailures(24)) } catch { /* the list below still works */ }
  }, [])
  useEffect(() => { void load() }, [load])

  const ack = async (operation?: string) => {
    setBusy(true)
    try {
      const r = await acknowledgeTaskFailures(operation)
      toast.success(`${r.acknowledged} failure(s) acknowledged`)
      await load()
      onChanged?.()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  if (!summary || summary.unacknowledged === 0) return null
  return (
    <MacGlassPanel
      title="Needs attention"
      subtitle={`${summary.unacknowledged} failed task${summary.unacknowledged === 1 ? '' : 's'} in the last ${summary.window_hours} h, grouped by cause`}
    >
      <div data-testid="task-failures" className="space-y-3">
        {summary.groups.map((g) => (
          <div key={`${g.operation}|${g.cause}`} className="rounded-xl border border-white/10 p-3 space-y-1.5" data-testid="task-failure-group">
            <div className="flex flex-wrap items-center gap-2">
              <AlertTriangle className="w-4 h-4 text-[#ff9f0a]" aria-hidden />
              <span className="font-medium text-[var(--text-primary)]">{g.operation}</span>
              <span className="text-xs rounded-full px-2 py-0.5 bg-white/10 text-[var(--text-muted)]">{g.count}×</span>
              <span className="text-xs text-[var(--text-muted)]">last {g.last_at}</span>
              <span className="flex-1" />
              {onShowOperation && <button type="button" className="btn-secondary text-xs" onClick={() => onShowOperation(g.operation)}>Show tasks</button>}
              <button type="button" className="btn-secondary text-xs inline-flex items-center gap-1" disabled={busy} onClick={() => void ack(g.operation)}><CheckCheck className="w-3 h-3" /> Acknowledge</button>
            </div>
            <p className="text-xs font-mono text-[var(--text-secondary)] break-words m-0">{g.cause}</p>
            {g.hint && (
              <p className="text-sm text-[var(--text-primary)] m-0 flex gap-2"><Lightbulb className="w-4 h-4 mt-0.5 text-[#ffd60a] shrink-0" aria-hidden />{g.hint}</p>
            )}
          </div>
        ))}
        <button type="button" className="btn-secondary text-xs" disabled={busy} onClick={() => void ack()}>Acknowledge all</button>
      </div>
    </MacGlassPanel>
  )
}
