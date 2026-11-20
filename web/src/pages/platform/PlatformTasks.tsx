// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useSearchParams } from 'react-router'
import { ClipboardList, X } from 'lucide-react'
import ExplainButton from '../../components/ai/ExplainButton'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import TaskFailuresPanel from '../../components/platform/TaskFailuresPanel'
import PlatformFilterPills from '../../components/platform/PlatformFilterPills'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import { statusBgClass, taskStatusTone } from '../../utils/semanticColors'
import { cancelTask, getPlatformHealth, getPlatformTask, listPlatformTasks, retryTask, type PlatformTask } from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

function statusColor(status: string) {
  return statusBgClass(taskStatusTone(status))
}

export default function PlatformTasks() {
  const toast = useToastContext()
  const [searchParams, setSearchParams] = useSearchParams()
  const highlightTaskId = searchParams.get('task')?.trim() || null
  const highlightRef = useRef<HTMLLIElement | null>(null)
  const [rows, setRows] = useState<PlatformTask[]>([])
  const [filter, setFilter] = useState('')
  const [opFilter, setOpFilter] = useState('')
  const [debouncedOpFilter, setDebouncedOpFilter] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [controllerHealth, setControllerHealth] = useState<{ status: string; leader?: boolean; controller_id?: string } | null>(null)
  const [taskDetail, setTaskDetail] = useState<PlatformTask | null>(null)
  const [detailBusy, setDetailBusy] = useState(false)
  const loadSeq = useRef(0)

  // Debounce the free-text operation filter so we don't fetch on every keystroke.
  useEffect(() => {
    const t = window.setTimeout(() => setDebouncedOpFilter(opFilter), 300)
    return () => window.clearTimeout(t)
  }, [opFilter])

  const load = useCallback(async () => {
    const seq = ++loadSeq.current
    setError(null)
    try {
      const [tasks, health] = await Promise.all([
        listPlatformTasks({ status: filter || undefined, operation: debouncedOpFilter || undefined }),
        getPlatformHealth().catch(() => null),
      ])
      if (seq !== loadSeq.current) return // a newer load superseded this one
      setRows(tasks)
      setControllerHealth(health)
    } catch (e: unknown) {
      if (seq !== loadSeq.current) return
      setError(formatUserError(e))
    } finally {
      if (seq === loadSeq.current) setLoading(false)
    }
  }, [filter, debouncedOpFilter])

  useEffect(() => { void load() }, [load])

  const openTaskDetail = useCallback((taskId: string) => {
    setDetailBusy(true)
    void getPlatformTask(taskId)
      .then(setTaskDetail)
      .catch((e: unknown) => toast.error(formatUserError(e)))
      .finally(() => setDetailBusy(false))
  }, [toast])

  useEffect(() => {
    if (!highlightTaskId) return
    openTaskDetail(highlightTaskId)
  }, [highlightTaskId, openTaskDetail])

  const highlightRow = useMemo(() => {
    if (!highlightTaskId) return null
    const needle = highlightTaskId.toLowerCase()
    return rows.find((t) => t.id === highlightTaskId || t.id.toLowerCase().startsWith(needle)) ?? null
  }, [rows, highlightTaskId])

  useEffect(() => {
    if (!highlightRow) return
    highlightRef.current?.scrollIntoView({ behavior: 'smooth', block: 'nearest' })
    const t = window.setTimeout(() => {
      const next = new URLSearchParams(searchParams)
      next.delete('task')
      setSearchParams(next, { replace: true })
    }, 8000)
    return () => window.clearTimeout(t)
  }, [highlightRow, searchParams, setSearchParams])

  const statusCounts = {
    '': rows.length,
    pending: rows.filter((t) => t.status === 'pending').length,
    running: rows.filter((t) => t.status === 'running').length,
    completed: rows.filter((t) => t.status === 'completed' || t.status === 'succeeded').length,
    failed: rows.filter((t) => t.status === 'failed').length,
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      loading={loading && rows.length === 0}
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/operations" label="Operations" />}
      title="Tasks"
      subtitle="Orchestration queue — like Activity Monitor for your datacenter."
      icon={<ClipboardList className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={<PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-4"
    >
      <TaskFailuresPanel onShowOperation={(op) => { setOpFilter(op); setFilter('failed') }} onChanged={() => void load()} />
      {controllerHealth && (
        <MacGlassPanel title="Controller health" subtitle="GET /api/v1/health">
          <dl className="grid gap-2 text-sm sm:grid-cols-3">
            <div>
              <dt className="text-xs text-[var(--text-muted)]">Status</dt>
              <dd className="text-[var(--text-primary)] capitalize">{controllerHealth.status}</dd>
            </div>
            <div>
              <dt className="text-xs text-[var(--text-muted)]">Leader</dt>
              <dd className="text-[var(--text-primary)]">{controllerHealth.leader == null ? '—' : controllerHealth.leader ? 'yes' : 'no'}</dd>
            </div>
            <div>
              <dt className="text-xs text-[var(--text-muted)]">Controller ID</dt>
              <dd className="font-mono text-[var(--text-primary)] text-xs">{controllerHealth.controller_id ?? '—'}</dd>
            </div>
          </dl>
        </MacGlassPanel>
      )}

      {taskDetail && (
        <MacGlassPanel
          title="Task detail"
          subtitle={taskDetail.id}
          action={
            <button type="button" className="btn-secondary text-xs" onClick={() => setTaskDetail(null)}>Close</button>
          }
        >
          <dl className="grid gap-2 text-sm sm:grid-cols-2">
            <div>
              <dt className="text-xs text-[var(--text-muted)]">Operation</dt>
              <dd className="text-[var(--text-primary)]">{taskDetail.operation}</dd>
            </div>
            <div>
              <dt className="text-xs text-[var(--text-muted)]">Status</dt>
              <dd className="text-[var(--text-primary)] capitalize">{taskDetail.status}</dd>
            </div>
            <div>
              <dt className="text-xs text-[var(--text-muted)]">Progress</dt>
              <dd className="text-[var(--text-primary)]">{taskDetail.progress}%</dd>
            </div>
            <div>
              <dt className="text-xs text-[var(--text-muted)]">Created</dt>
              <dd className="text-[var(--text-primary)]">{new Date(taskDetail.created_at).toLocaleString()}</dd>
            </div>
            {taskDetail.message && (
              <div className="sm:col-span-2">
                <dt className="text-xs text-[var(--text-muted)]">Message</dt>
                <dd className="text-[var(--text-secondary)]">{taskDetail.message}</dd>
              </div>
            )}
          </dl>
        </MacGlassPanel>
      )}

      <PlatformFilterPills
        value={filter}
        onChange={setFilter}
        options={[
          { id: '', label: 'All', count: statusCounts[''] },
          { id: 'pending', label: 'Pending', count: statusCounts.pending },
          { id: 'running', label: 'Running', count: statusCounts.running },
          { id: 'completed', label: 'Completed', count: statusCounts.completed },
          { id: 'failed', label: 'Failed', count: statusCounts.failed },
        ]}
      />
      <div className="relative max-w-xs">
        <input className={`input w-full ${opFilter ? 'pr-8' : 'pr-4'}`} aria-label="Filter by operation" placeholder="Filter by operation" value={opFilter} onChange={(e) => setOpFilter(e.target.value)} />
        {opFilter && (
          <button type="button" aria-label="Clear filter" onClick={() => setOpFilter('')}
            className="absolute right-2 top-1/2 -translate-y-1/2 text-[var(--text-muted)] hover:text-[var(--text-primary)]">
            <X className="w-4 h-4" />
          </button>
        )}
      </div>
      {rows.length === 0 && !error ? (
        <PlatformEmptyState title="No tasks" subtitle="Operations like VM create, migrate, and backup appear here." />
      ) : (
        <ul className="space-y-2">
          {rows.map((t) => {
            const highlighted = highlightRow?.id === t.id
            return (
            <li
              key={t.id}
              ref={highlighted ? highlightRef : undefined}
              className={`platform-mac-stat rounded-xl border bg-[var(--apple-surface)] p-4 transition ${
                highlighted ? 'border-[var(--accent)]/50 ring-1 ring-[var(--accent)]/40' : 'border-white/[0.06]'
              }`}
            >
              <div className="flex flex-wrap items-center justify-between gap-3 mb-2">
                <div>
                  <p className="font-medium text-[var(--text-primary)]">{t.operation}</p>
                  <p className="text-xs text-[var(--text-muted)] font-mono">{t.id.slice(0, 8)}</p>
                </div>
                <span className="text-xs capitalize text-[var(--text-muted)]">{t.status}</span>
              </div>
              <div className="h-1.5 rounded-full bg-[var(--apple-fill-tertiary)] overflow-hidden mb-2">
                <div className={`h-full rounded-full transition-all ${statusColor(t.status)}`} style={{ width: `${Math.min(100, t.progress)}%` }} />
              </div>
              <div className="flex justify-between items-center text-xs text-[var(--text-muted)]">
                <span>{t.progress}% {t.message || ''}</span>
                <div className="flex gap-2">
                  <button
                    type="button"
                    className="btn-secondary text-xs"
                    disabled={detailBusy}
                    onClick={() => openTaskDetail(t.id)}
                  >
                    Details
                  </button>
                  {t.status === 'pending' && (
                    <button type="button" className="btn-secondary text-xs" onClick={async () => {
                      try { await cancelTask(t.id); toast.success('Cancelled'); await load() } catch (e: unknown) { toast.error(formatUserError(e)) }
                    }}>Cancel</button>
                  )}
                  {t.status === 'failed' && (
                    <>
                      <ExplainButton screen="failed_task" objectRef={{ task_id: t.id, operation: t.operation, message: t.message }} />
                      <button type="button" className="btn-secondary text-xs" onClick={async () => {
                      try { await retryTask(t.id); toast.success('Retry queued'); await load() } catch (e: unknown) { toast.error(formatUserError(e)) }
                    }}>Retry</button>
                    </>
                  )}
                </div>
              </div>
            </li>
          )})}
        </ul>
      )}
    </PlatformPageChrome>
  )
}
