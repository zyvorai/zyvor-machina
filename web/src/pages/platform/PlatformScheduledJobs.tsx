// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import ConfirmDialog from '../../components/ConfirmDialog'
import { CalendarClock, Plus, Trash2 } from 'lucide-react'
import OperatingSurfaceLayout from '../../components/platform/OperatingSurfaceLayout'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import {
  createScheduledJob,
  deleteScheduledJob,
  listScheduledJobs,
  setScheduledJobEnabled,
  SCHEDULED_JOB_OPERATIONS,
  type ScheduledJob,
} from '../../api/day2'
import { listPlatformHosts, type PlatformHost } from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

export default function PlatformScheduledJobs() {
  const toast = useToastContext()
  const [rows, setRows] = useState<ScheduledJob[]>([])
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [name, setName] = useState('nightly-inventory')
  const [operation, setOperation] = useState<string>(SCHEDULED_JOB_OPERATIONS[0])
  const [intervalMinutes, setIntervalMinutes] = useState('60')
  const [targetHostId, setTargetHostId] = useState('')
  const [saving, setSaving] = useState(false)
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null)
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setError(null)
    try {
      const [jobs, hostList] = await Promise.all([
        listScheduledJobs(),
        listPlatformHosts().catch(() => [] as PlatformHost[]),
      ])
      if (!alive()) return
      setRows(jobs)
      setHosts(hostList)
    } catch (e: unknown) {
      if (alive()) setError(formatUserError(e))
    } finally {
      if (alive()) setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const add = async () => {
    if (!name.trim() || saving) return
    setSaving(true)
    try {
      await createScheduledJob({
        name: name.trim(),
        operation,
        target_host_id: targetHostId || undefined,
        interval_minutes: Number(intervalMinutes) || 60,
      })
      toast.success('Scheduled job created')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSaving(false)
    }
  }

  const remove = async (id: string) => {
    try {
      await deleteScheduledJob(id)
      toast.success('Scheduled job deleted')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const hostName = (id: string) => hosts.find((h) => h.id === id)?.hostname || id

  const toggleEnabled = async (j: ScheduledJob) => {
    try {
      await setScheduledJobEnabled(j.id, !j.enabled)
      toast.success(j.enabled ? `Disabled '${j.name}'` : `Enabled '${j.name}'`)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/operations" label="Operations" />}
      title="Scheduled jobs"
      subtitle="Recurring controller operations (whitelisted) — e.g. periodic host inventory refresh."
      icon={<CalendarClock className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={<PlatformRefreshButton onClick={() => void load()} />}
      loading={loading && rows.length === 0}
      contentClassName="space-y-4 xl:space-y-0 xl:columns-2 xl:gap-4 [&>*]:break-inside-avoid xl:[&>*]:mb-4"
    >
      <OperatingSurfaceLayout testId="platform-scheduled-jobs-page">
        <MacGlassPanel title="New scheduled job">
          <div className="grid gap-3 md:grid-cols-2 max-w-2xl">
            <input className="input text-sm" aria-label="Job name" placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} />
            <select className="input text-sm" aria-label="Operation" value={operation} onChange={(e) => setOperation(e.target.value)}>
              {SCHEDULED_JOB_OPERATIONS.map((op) => (
                <option key={op} value={op}>{op}</option>
              ))}
            </select>
            <input className="input text-sm" aria-label="Interval minutes" type="number" min={1} placeholder="Interval (minutes)" value={intervalMinutes} onChange={(e) => setIntervalMinutes(e.target.value)} />
            <select className="input text-sm" aria-label="Target host" value={targetHostId} onChange={(e) => setTargetHostId(e.target.value)}>
              <option value="">All hosts (optional)</option>
              {hosts.map((h) => (
                <option key={h.id} value={h.id}>{h.hostname}</option>
              ))}
            </select>
          </div>
          <button type="button" className="btn-primary text-sm mt-3 flex items-center gap-1.5" disabled={saving || !name.trim()} onClick={() => void add()}>
            <Plus className="w-4 h-4" /> {saving ? 'Saving…' : 'Add job'}
          </button>
          <p className="text-xs text-[var(--text-muted)] mt-2">Only whitelisted operations may be scheduled.</p>
        </MacGlassPanel>

        <MacGlassPanel title="Active jobs" subtitle={loading ? 'Loading…' : `${rows.length} job(s)`}>
          {rows.length === 0 && !loading ? (
            <PlatformEmptyState
              icon={CalendarClock}
              title="No scheduled jobs yet"
              subtitle="Add a recurring job such as periodic host inventory refresh."
            />
          ) : (
            <ul className="divide-y divide-white/[0.04] -mx-1">
              {rows.map((j) => (
                <MacListRow
                  key={j.id}
                  title={j.name}
                  subtitle={`${j.operation} · every ${j.interval_minutes}m · ${
                    j.target_host_id ? hostName(j.target_host_id) : 'all hosts'
                  }${j.enabled ? '' : ' · disabled'} · last run ${j.last_run_at ? new Date(j.last_run_at).toLocaleString() : 'never'}`}
                  badge={
                    <div className="flex items-center gap-1.5">
                      <button type="button" className="btn-secondary text-xs px-2 py-1" onClick={() => void toggleEnabled(j)}>
                        {j.enabled ? 'Disable' : 'Enable'}
                      </button>
                      <button type="button" className="btn-secondary text-xs p-1.5" aria-label="Delete" onClick={() => setConfirmDeleteId(j.id)}>
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  }
                />
              ))}
            </ul>
          )}
        </MacGlassPanel>
      </OperatingSurfaceLayout>
      <ConfirmDialog
        open={confirmDeleteId !== null}
        title="Delete Scheduled Job"
        message={`Delete scheduled job "${rows.find((j) => j.id === confirmDeleteId)?.name}"? It will stop running on its schedule.`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setConfirmDeleteId(null)}
        onConfirm={async () => {
          await remove(confirmDeleteId!)
          setConfirmDeleteId(null)
        }}
      />
    </PlatformPageChrome>
  )
}
