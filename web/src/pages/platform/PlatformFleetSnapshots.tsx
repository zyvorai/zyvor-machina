// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import ConfirmDialog from '../../components/ConfirmDialog'
import { Camera, Plus, Trash2 } from 'lucide-react'
import OperatingSurfaceLayout from '../../components/platform/OperatingSurfaceLayout'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import PlatformPageChrome, { PlatformBackLink } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import {
  createFleetSnapshotSchedule,
  deleteFleetSnapshotSchedule,
  listFleetSnapshotSchedules,
  type FleetSnapshotSchedule,
} from '../../api/platformFleetSnapshots'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

export default function PlatformFleetSnapshots() {
  const toast = useToastContext()
  const [rows, setRows] = useState<FleetSnapshotSchedule[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [name, setName] = useState('nightly-fleet')
  const [project, setProject] = useState('')
  const [tagFilter, setTagFilter] = useState('')
  const [diskOnly, setDiskOnly] = useState(true)
  const [quiesce, setQuiesce] = useState(false)
  const [saving, setSaving] = useState(false)
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null)

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      setRows(await listFleetSnapshotSchedules())
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const add = async () => {
    if (!name.trim()) return
    setSaving(true)
    try {
      await createFleetSnapshotSchedule({
        name: name.trim(),
        project: project.trim() || undefined,
        tag_filter: tagFilter.trim() || undefined,
        disk_only: diskOnly,
        quiesce,
      })
      toast.success('Fleet snapshot schedule created')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSaving(false)
    }
  }

  const remove = async (id: string) => {
    try {
      await deleteFleetSnapshotSchedule(id)
      toast.success('Schedule deleted')
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
      title="Fleet snapshot schedules"
      subtitle="Nightly disk snapshots across managed libvirt VMs (project or tag filter)."
      icon={<Camera className="w-6 h-6 text-[var(--text-muted)]" />}
      loading={loading && rows.length === 0}
      contentClassName="space-y-4 xl:space-y-0 xl:columns-2 xl:gap-4 [&>*]:break-inside-avoid xl:[&>*]:mb-4"
    >
      <OperatingSurfaceLayout testId="platform-fleet-snapshots-page">
      <MacGlassPanel title="New schedule">
        <div className="grid gap-3 md:grid-cols-2 max-w-2xl">
          <input className="input text-sm" aria-label="Schedule name" placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} />
          <input className="input text-sm" aria-label="Project" placeholder="Project (optional)" value={project} onChange={(e) => setProject(e.target.value)} />
          <input className="input text-sm md:col-span-2" aria-label="Tag filter" placeholder="Tag filter (optional)" value={tagFilter} onChange={(e) => setTagFilter(e.target.value)} />
          <label className="flex items-center gap-2 text-sm text-[var(--text-secondary)]">
            <input type="checkbox" checked={diskOnly} onChange={(e) => setDiskOnly(e.target.checked)} /> Disk-only snapshot
          </label>
          <label className="flex items-center gap-2 text-sm text-[var(--text-secondary)]">
            <input type="checkbox" checked={quiesce} onChange={(e) => setQuiesce(e.target.checked)} /> Guest quiesce (app-consistent)
          </label>
        </div>
        <button type="button" className="btn-primary text-sm mt-3 flex items-center gap-1.5" disabled={saving} onClick={() => void add()}>
          <Plus className="w-4 h-4" /> {saving ? 'Saving…' : 'Add schedule'}
        </button>
        <p className="text-xs text-[var(--text-muted)] mt-2">Runs about once per 23h per schedule.</p>
      </MacGlassPanel>

      <MacGlassPanel title="Active schedules" subtitle={loading ? 'Loading…' : `${rows.length} schedule(s)`}>
        {rows.length === 0 && !loading ? (
          <PlatformEmptyState
            icon={Camera}
            title="No fleet schedules yet"
            subtitle="Add a nightly schedule to snapshot managed libvirt VMs by project or tag."
          />
        ) : (
          <ul className="divide-y divide-white/[0.04] -mx-1">
            {rows.map((s) => (
              <MacListRow
                key={s.id}
                title={s.name}
                subtitle={`${s.project || 'all projects'} · tag=${s.tag_filter || '*'} · disk_only=${s.disk_only} · quiesce=${s.quiesce}`}
                badge={
                  <button type="button" className="btn-secondary text-xs p-1.5" aria-label="Delete" onClick={() => setConfirmDeleteId(s.id)}>
                    <Trash2 className="w-3.5 h-3.5" />
                  </button>
                }
              />
            ))}
          </ul>
        )}
      </MacGlassPanel>
      </OperatingSurfaceLayout>
      <ConfirmDialog
        open={confirmDeleteId !== null}
        title="Delete Snapshot Schedule"
        message={`Delete schedule "${rows.find((s) => s.id === confirmDeleteId)?.name}"? Future fleet snapshots will no longer run on this schedule.`}
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
