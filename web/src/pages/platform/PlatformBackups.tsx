// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { Camera } from 'lucide-react'
import { Archive, CalendarClock, Clock, Database, Plus, RotateCcw, Trash2 } from 'lucide-react'
import { MacGlassPanel, MacListRow, MacSegmentedControl } from '../../components/platform/mac/PlatformMacUi'
import PlatformStandardView from '../../components/platform/tahoe/PlatformStandardView'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import ConfirmDialog from '../../components/ConfirmDialog'
import {
  createBackupTarget,
  createVmBackupWithTarget,
  getFleetBackups,
  listBackupTargets,
  listBackupTimeline,
  restoreVmBackup,
  type BackupTarget,
  type BackupTimelineEntry,
  type FleetBackupOverview,
} from '../../api/platform'
import {
  createBackupSchedule,
  deleteBackupSchedule,
  listBackupSchedules,
  type BackupSchedule,
} from '../../api/day2'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import {hostStateTone, httpStatusTone, migrationReadinessTone, riskTone, statusBadgeClasses, statusPillClasses, statusToneClass, taskStatusTone, webhookDeliveryTone, hubLinkClasses} from '../../utils/semanticColors'
import { useExpandable } from '../../hooks/useExpandable'
import { ExpandableToggle } from '../../components/ui/ExpandableToggle'

function dayLabel(iso: string) {
  const d = new Date(iso)
  const today = new Date()
  const start = new Date(today.getFullYear(), today.getMonth(), today.getDate())
  const day = new Date(d.getFullYear(), d.getMonth(), d.getDate())
  // Round: on DST-transition days the local-midnight delta is 23h or 25h, so a
  // raw division yields 0.958 / 1.041 and the strict === 1 check would miss
  // "Yesterday". Rounding maps each calendar day to a whole-number offset.
  const diff = Math.round((start.getTime() - day.getTime()) / 86_400_000)
  if (diff === 0) return 'Today'
  if (diff === 1) return 'Yesterday'
  return d.toLocaleDateString(undefined, { weekday: 'long', month: 'short', day: 'numeric' })
}

type TabId = 'timeline' | 'destinations' | 'schedules'

function tabFromParam(value: string | null): TabId {
  if (value === 'destinations' || value === 'schedules') return value
  return 'timeline'
}

export default function PlatformBackups() {
  const toast = useToastContext()
  const [searchParams, setSearchParams] = useSearchParams()
  const tab: TabId = tabFromParam(searchParams.get('tab'))
  const [timeline, setTimeline] = useState<BackupTimelineEntry[]>([])
  const [targets, setTargets] = useState<BackupTarget[]>([])
  const [fleet, setFleet] = useState<FleetBackupOverview | null>(null)
  const [schedules, setSchedules] = useState<BackupSchedule[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [restoring, setRestoring] = useState<string | null>(null)
  const [pendingRestore, setPendingRestore] = useState<BackupTimelineEntry | null>(null)
  const [targetName, setTargetName] = useState('nfs-primary')
  const [targetKind, setTargetKind] = useState('nfs')
  const [addingTarget, setAddingTarget] = useState(false)
  const [backupVmId, setBackupVmId] = useState('')
  const [backupTargetId, setBackupTargetId] = useState('')
  const [backupType, setBackupType] = useState<'full' | 'incremental'>('full')
  const [schedName, setSchedName] = useState('nightly-backup')
  const [schedProject, setSchedProject] = useState('')
  const [schedTag, setSchedTag] = useState('')
  const [schedIntervalHours, setSchedIntervalHours] = useState('24')
  const [schedRetainCount, setSchedRetainCount] = useState('7')
  const [schedEnabled, setSchedEnabled] = useState(true)
  const [addingSchedule, setAddingSchedule] = useState(false)
  const [deleteScheduleId, setDeleteScheduleId] = useState<string | null>(null)
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setError(null)
    try {
      const [t, f, tg, sc] = await Promise.all([
        listBackupTimeline(),
        getFleetBackups(),
        listBackupTargets(),
        listBackupSchedules().catch(() => [] as BackupSchedule[]),
      ])
      if (!alive()) return
      setTimeline(t)
      setFleet(f)
      setTargets(tg)
      setSchedules(sc)
    } catch (e: unknown) {
      if (alive()) setError(formatUserError(e))
    } finally {
      if (alive()) setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  // Capped before grouping, not after — capping per-day groups would need a second, nested
  // "show more" per day; timeline entries already arrive newest-first, so this is just "show the
  // N most recent events" the same way it would read without grouping.
  const timelineList = useExpandable(timeline, 40)

  const grouped = useMemo(() => {
    const map = new Map<string, BackupTimelineEntry[]>()
    for (const e of timelineList.shown) {
      const key = dayLabel(e.created_at)
      if (!map.has(key)) map.set(key, [])
      map.get(key)!.push(e)
    }
    return [...map.entries()]
  }, [timelineList.shown])

  const restore = async (entry: BackupTimelineEntry) => {
    if (entry.kind !== 'backup' || entry.status !== 'completed') return
    setRestoring(entry.id)
    try {
      await restoreVmBackup(entry.vm_id, entry.id)
      toast.success(`Restore queued for ${entry.vm_name}`)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setRestoring(null)
    }
  }

  const addTarget = async () => {
    if (!targetName.trim() || addingTarget) return
    setAddingTarget(true)
    try {
      await createBackupTarget({ name: targetName.trim(), kind: targetKind })
      toast.success('Backup destination added')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setAddingTarget(false)
    }
  }

  const queueBackup = async () => {
    if (!backupVmId.trim()) return
    try {
      await createVmBackupWithTarget(backupVmId.trim(), {
        target_id: backupTargetId || undefined,
        backup_type: backupType,
      })
      toast.success('Backup queued')
      setBackupVmId('')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const addSchedule = async () => {
    if (!schedName.trim() || addingSchedule) return
    setAddingSchedule(true)
    try {
      await createBackupSchedule({
        name: schedName.trim(),
        project: schedProject.trim() || undefined,
        tag_filter: schedTag.trim() || undefined,
        interval_hours: Number(schedIntervalHours) || 24,
        retain_count: Number(schedRetainCount) || 0,
        enabled: schedEnabled,
      })
      toast.success('Backup schedule created')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setAddingSchedule(false)
    }
  }

  const removeSchedule = async (id: string) => {
    try {
      await deleteBackupSchedule(id)
      toast.success('Backup schedule deleted')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  return (
    <PlatformStandardView
      className="space-y-6"
      title="Backups"
      description="Fleet backup timeline, S3/MinIO destinations, and scheduled snapshots."
      icon={Archive}
      loading={loading}
      error={error}
      stats={
        fleet
          ? [
              { label: 'Today', value: String(fleet.backups_completed_24h ?? 0), tone: 'emerald' },
              { label: 'Failed 24h', value: String(fleet.backups_failed_24h ?? 0), tone: fleet.backups_failed_24h ? 'amber' : 'sky' },
              { label: 'Snapshots', value: String(fleet.snapshots_total ?? 0), tone: 'violet' },
              { label: 'Destinations', value: String(targets.length), tone: 'sky' },
            ]
          : undefined
      }
    >
      <p className="text-xs text-[var(--text-muted)] flex flex-wrap gap-3">
        <Link to="/platform/fleet-snapshots" className={`inline-flex items-center gap-1 ${hubLinkClasses()}`}>
          <Camera className="w-3.5 h-3.5" /> Fleet snapshot schedules
        </Link>
      </p>
      <MacSegmentedControl
        options={[
          { value: 'timeline' as TabId, label: 'Timeline', icon: <Archive className="w-3.5 h-3.5" /> },
          { value: 'destinations' as TabId, label: 'Destinations', icon: <Database className="w-3.5 h-3.5" /> },
          { value: 'schedules' as TabId, label: 'Schedules', icon: <CalendarClock className="w-3.5 h-3.5" /> },
        ]}
        value={tab}
        onChange={(id) => setSearchParams(id === 'timeline' ? {} : { tab: id })}
      />

      {tab === 'destinations' && (
        <div className="grid gap-4 xl:grid-cols-2 xl:items-start">
          <MacGlassPanel title="Backup destinations" subtitle="Register NFS, S3, or local targets for fleet backups.">
            <div className="grid gap-3 md:grid-cols-3 mb-4">
              <input className="input text-sm" aria-label="Backup destination name" placeholder="Name" value={targetName} onChange={(e) => setTargetName(e.target.value)} />
              <select className="input text-sm" aria-label="Destination type" value={targetKind} onChange={(e) => setTargetKind(e.target.value)}>
                <option value="nfs">nfs</option>
                <option value="s3">s3</option>
                <option value="local">local</option>
              </select>
              <button type="button" disabled={addingTarget || !targetName.trim()} className="tahoe-btn-primary text-sm disabled:opacity-40 disabled:cursor-not-allowed" onClick={() => void addTarget()}>{addingTarget ? 'Adding…' : 'Add destination'}</button>
            </div>
            <ul className="divide-y divide-white/[0.04]">
              {targets.map((t) => (
                <li key={t.id} className="py-2 flex justify-between gap-2 text-sm">
                  <span className="text-[var(--text-primary)]">{t.name}</span>
                  <span className="text-[var(--text-muted)] uppercase text-xs">{t.kind}</span>
                </li>
              ))}
              {targets.length === 0 && <li className="text-sm text-[var(--text-muted)] py-2">No destinations — add one above.</li>}
            </ul>
          </MacGlassPanel>
          <MacGlassPanel title="Queue VM backup" subtitle="Full qcow2 or incremental (chains prior completed backup on host).">
            <div className="grid gap-3 md:grid-cols-2 lg:grid-cols-4">
              <input className="input text-sm" aria-label="VM ID" placeholder="VM id" value={backupVmId} onChange={(e) => setBackupVmId(e.target.value)} />
              <select className="input text-sm" aria-label="Backup type" value={backupType} onChange={(e) => setBackupType(e.target.value as 'full' | 'incremental')}>
                <option value="full">Full backup</option>
                <option value="incremental">Incremental</option>
              </select>
              <select className="input text-sm" aria-label="Backup destination" value={backupTargetId} onChange={(e) => setBackupTargetId(e.target.value)}>
                <option value="">Default target</option>
                {targets.map((t) => <option key={t.id} value={t.id}>{t.name} ({t.kind})</option>)}
              </select>
              <button type="button" className="btn-secondary text-sm" onClick={() => void queueBackup()}>Queue backup</button>
            </div>
          </MacGlassPanel>
        </div>
      )}

      {tab === 'schedules' && (
        <div className="grid gap-4 xl:grid-cols-2 xl:items-start">
          <MacGlassPanel title="New backup schedule" subtitle="Recurring fleet backups by project or tag, with retention.">
            <div className="grid gap-3 md:grid-cols-2">
              <input className="input text-sm" aria-label="Schedule name" placeholder="Name" value={schedName} onChange={(e) => setSchedName(e.target.value)} />
              <input className="input text-sm" aria-label="Project" placeholder="Project (optional)" value={schedProject} onChange={(e) => setSchedProject(e.target.value)} />
              <input className="input text-sm" aria-label="Tag filter" placeholder="Tag filter (optional)" value={schedTag} onChange={(e) => setSchedTag(e.target.value)} />
              <input className="input text-sm" aria-label="Interval hours" type="number" min={1} max={720} placeholder="Interval (hours)" value={schedIntervalHours} onChange={(e) => setSchedIntervalHours(e.target.value)} />
              <input className="input text-sm" aria-label="Retain count" type="number" min={0} max={1000} placeholder="Retain count" value={schedRetainCount} onChange={(e) => setSchedRetainCount(e.target.value)} />
              <label className="flex items-center gap-2 text-sm text-[var(--text-secondary)]">
                <input type="checkbox" checked={schedEnabled} onChange={(e) => setSchedEnabled(e.target.checked)} /> Enabled
              </label>
            </div>
            <button type="button" className="btn-primary text-sm mt-3 flex items-center gap-1.5" disabled={addingSchedule || !schedName.trim()} onClick={() => void addSchedule()}>
              <Plus className="w-4 h-4" /> {addingSchedule ? 'Saving…' : 'Add schedule'}
            </button>
          </MacGlassPanel>
          <MacGlassPanel title="Active schedules" subtitle={loading ? 'Loading…' : `${schedules.length} schedule(s)`}>
            {schedules.length === 0 && !loading ? (
              <PlatformEmptyState
                icon={CalendarClock}
                title="No backup schedules yet"
                subtitle="Add a recurring schedule to back up managed VMs by project or tag."
              />
            ) : (
              <ul className="divide-y divide-white/[0.04] -mx-1">
                {schedules.map((s) => (
                  <MacListRow
                    key={s.id}
                    title={s.name}
                    subtitle={`${s.project || 'all projects'} · tag=${s.tag_filter || '*'} · ${s.backup_type || 'full'} · every ${s.interval_hours}h · retain ${s.retain_count}${s.enabled ? '' : ' · disabled'} · last run ${s.last_run_at ? new Date(s.last_run_at).toLocaleString() : 'never'}`}
                    badge={
                      <button type="button" className="btn-secondary text-xs p-1.5" aria-label="Delete" onClick={() => setDeleteScheduleId(s.id)}>
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                    }
                  />
                ))}
              </ul>
            )}
          </MacGlassPanel>
        </div>
      )}

      {tab === 'timeline' && (
        <>
          <p className="text-sm text-[var(--text-muted)]">
            Need per-VM legacy jobs? <Link to="/backups" className={hubLinkClasses()}>Open classic backups UI →</Link>
          </p>
          {fleet?.summary ? <p className="text-sm text-[var(--text-muted)]">{fleet.summary}</p> : null}
          <div className="space-y-6" id={timelineList.listId}>
            {grouped.length === 0 && !error && (
              <PlatformEmptyState icon={Archive} title="No backup events yet" subtitle="Create backups from VM detail pages or run fleet backup jobs.">
                <Link to="/platform/vms" className="tahoe-btn-ghost text-sm">Browse VMs</Link>
              </PlatformEmptyState>
            )}
            {grouped.map(([day, entries]) => (
              <section key={day}>
                <h2 className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)] mb-3">{day}</h2>
                <div className="relative pl-4 border-l border-[var(--apple-hairline)] space-y-3">
                  {entries.map((e) => (
                    <article key={`${e.kind}-${e.id}`} className="relative tahoe-glass-card p-4 flex gap-4">
                      <span className="absolute -left-[1.35rem] top-5 w-2.5 h-2.5 rounded-full bg-[var(--apple-fill-secondary)] border-2 border-[var(--apple-surface)]" />
                      <div className={`w-10 h-10 rounded-xl flex items-center justify-center shrink-0 ${e.status === 'completed' ? statusBadgeClasses('ok') : 'bg-[var(--apple-fill-tertiary)] text-[var(--text-muted)]'}`}>
                        <Archive className="w-5 h-5" />
                      </div>
                      <div className="flex-1 min-w-0">
                        <p className="text-xs text-[var(--text-muted)] flex items-center gap-1">
                          <Clock className="w-3 h-3" /> {new Date(e.created_at).toLocaleTimeString()}
                          <span className="text-[var(--text-faint)]">· {e.kind}</span>
                        </p>
                        <p className="text-sm font-medium text-[var(--text-primary)] mt-0.5">{e.label}</p>
                        <Link to={`/platform/vms/${e.vm_id}`} className={`text-xs hover:underline ${hubLinkClasses()}`}>{e.vm_name}</Link>
                      </div>
                      <div className="flex flex-col gap-1 shrink-0">
                        {e.kind === 'backup' && e.status === 'completed' && (
                          <button type="button" className="tahoe-btn-primary text-xs flex items-center gap-1" disabled={restoring === e.id} onClick={() => setPendingRestore(e)}>
                            <RotateCcw className="w-3 h-3" /> {restoring === e.id ? 'Queuing…' : 'Restore'}
                          </button>
                        )}
                        {e.kind === 'snapshot' && (
                          <Link to={`/platform/vms/${e.vm_id}`} className="tahoe-btn-ghost text-xs text-center">Snapshots</Link>
                        )}
                      </div>
                    </article>
                  ))}
                </div>
              </section>
            ))}
          </div>
          {timelineList.showToggle && (
            <ExpandableToggle expanded={timelineList.expanded} hidden={timelineList.hidden} listId={timelineList.listId} onToggle={timelineList.toggle} noun="events" />
          )}
        </>
      )}
      {!(tab === 'timeline' && grouped.length === 0 && !error) && (
        <MacGlassPanel title="Per-VM backups" subtitle="Full backup history and restore live on each VM detail page.">
          <Link to="/platform/vms" className={`text-sm hover:underline ${hubLinkClasses()}`}>Browse VMs →</Link>
        </MacGlassPanel>
      )}
      <ConfirmDialog
        open={pendingRestore !== null}
        title="Restore VM from backup"
        message={pendingRestore ? `Restore "${pendingRestore.vm_name}" from backup "${pendingRestore.label}"? The VM will be powered off and its disk replaced. This cannot be undone.` : ''}
        confirmLabel="Restore"
        variant="danger"
        onCancel={() => setPendingRestore(null)}
        onConfirm={async () => {
          if (!pendingRestore) return
          setPendingRestore(null)
          await restore(pendingRestore)
        }}
      />
      <ConfirmDialog
        open={deleteScheduleId !== null}
        title="Delete Backup Schedule"
        message={`Delete backup schedule "${schedules.find((s) => s.id === deleteScheduleId)?.name}"? Future backups will no longer run on this schedule.`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setDeleteScheduleId(null)}
        onConfirm={async () => {
          const id = deleteScheduleId
          setDeleteScheduleId(null)
          if (id) await removeSchedule(id)
        }}
      />
    </PlatformStandardView>
  )
}
