// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, useCallback } from 'react'
import { listAllSnapshots, deleteSnapshot, revertSnapshot, SnapshotInfo } from '../api/snapshot'
import { useToastContext } from '../contexts/ToastContext'
import ConfirmDialog from '../components/ConfirmDialog'
import EmptyState from '../components/EmptyState'
import PageLayout from '../components/PageLayout'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { Trash2, RotateCcw, RefreshCw, Camera, AlertTriangle } from 'lucide-react'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses, statusBadgeClasses, statusToneClass } from '../utils/semanticColors'
import { snapshotStateSeverity } from '../utils/snapshotHealth'

function SnapshotStateBadge({ state }: { state: string }) {
  const sev = snapshotStateSeverity(state)
  return (
    <span className={`inline-flex items-center gap-1 px-2 py-0.5 rounded text-xs font-medium ${statusBadgeClasses(sev)}`}>
      {(sev === 'error' || sev === 'warn') && <AlertTriangle className="w-3 h-3" />}
      {state || '—'}
    </span>
  )
}

export default function SnapshotsPage() {
  const [snapshots, setSnapshots] = useState<SnapshotInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<SnapshotInfo | null>(null)
  const [revertTarget, setRevertTarget] = useState<SnapshotInfo | null>(null)
  const [search, setSearch] = useState('')
  const toast = useToastContext()

  const load = useCallback(async () => {
    try {
      setLoading(true)
      setLoadError(null)
      setSnapshots(await listAllSnapshots())
    } catch (e: unknown) {
      setLoadError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { load() }, [load])

  const handleRevert = async () => {
    if (!revertTarget) return
    const snap = revertTarget
    setRevertTarget(null)
    try { await revertSnapshot(snap.vm_name, snap.name); toast.success(`Reverted '${snap.vm_name}' to '${snap.name}'`); load() } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
  }

  const handleDelete = async () => {
    if (!deleteTarget) return
    try { await deleteSnapshot(deleteTarget.vm_name, deleteTarget.name); toast.success(`Deleted snapshot '${deleteTarget.name}'`); load() } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
    setDeleteTarget(null)
  }

  const filtered = snapshots.filter((s) => {
    const q = search.trim().toLowerCase()
    if (!q) return true
    return s.name.toLowerCase().includes(q) || s.vm_name.toLowerCase().includes(q) || s.state.toLowerCase().includes(q)
  })

  return (
    <PageLayout
      eyebrow="Hypervisor"
      title="Snapshots"
      icon={<Camera className="w-6 h-6" />}
      actions={
        <button onClick={load} className="p-2 hover:bg-[var(--surface-hover)] rounded transition" title="Refresh" aria-label="Refresh">
          <RefreshCw className="w-4 h-4" />
        </button>
      }
      contentLoading={loading}
      error={loadError}
      errorTitle="Failed to load snapshots"
      onErrorRetry={load}
      onErrorDismiss={() => setLoadError(null)}
    >
      <TahoeToolbar
        search={search}
        onSearchChange={setSearch}
        placeholder="Search snapshots…"
        trailing={
          search ? (
            <span aria-live="polite" className="text-sm text-[var(--text-muted)] shrink-0 pr-2">
              {filtered.length} snapshot{filtered.length !== 1 ? 's' : ''}
            </span>
          ) : null
        }
      />

      {loadError ? null : snapshots.length === 0 ? (
        <EmptyState
          icon={<Camera className="w-6 h-6" />}
          title="No snapshots"
          description="VM snapshots appear here after you create them from a guest's details page."
        />
      ) : (
        <TahoeTableWrap>
          <table className="apple-table" aria-label="VM snapshots">
            <thead><tr><th scope="col">Snapshot</th><th scope="col">VM</th><th scope="col">State</th><th scope="col" className="hidden md:table-cell">Created</th><th scope="col">Current</th><th scope="col" className="text-right">Actions</th></tr></thead>
            <tbody>
              {filtered.length === 0 && (
                <tr><td colSpan={6} className="text-center text-[var(--text-muted)]">No snapshots match your search.</td></tr>
              )}
              {filtered.map((s) => (
                <tr key={`${s.vm_name}/${s.name}`}>
                  <td className="font-medium">{s.name}</td>
                  <td className={`text-sm ${statusActionLinkClasses('info')}`}>{s.vm_name}</td>
                  <td><SnapshotStateBadge state={s.state} /></td>
                  <td className="text-sm text-[var(--text-muted)] hidden md:table-cell">{s.creation_time ? new Date(s.creation_time * 1000).toLocaleString() : '-'}</td>
                  <td>{s.is_current && <span className={`text-xs font-medium ${statusToneClass('ok')}`}>● Current</span>}</td>
                  <td>
                    <div className="flex items-center justify-end gap-1">
                      <button onClick={() => setRevertTarget(s)} className="p-1.5 hover:bg-white/10 rounded transition" title="Revert" aria-label="Revert"><RotateCcw className={`w-4 h-4 ${statusToneClass('info')}`} /></button>
                      <button onClick={() => setDeleteTarget(s)} className="p-1.5 hover:bg-red-600/20 rounded transition" title="Delete" aria-label="Delete"><Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} /></button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </TahoeTableWrap>
      )}
      <ConfirmDialog open={!!deleteTarget} title="Delete Snapshot" message={`Delete snapshot '${deleteTarget?.name}' from VM '${deleteTarget?.vm_name}'?`} confirmLabel="Delete" onConfirm={handleDelete} onCancel={() => setDeleteTarget(null)} />
      <ConfirmDialog open={!!revertTarget} variant="warning" title="Revert Snapshot" message={`Revert VM '${revertTarget?.vm_name}' to snapshot '${revertTarget?.name}'? The guest's current disk and memory state will be discarded.`} confirmLabel="Revert" onConfirm={handleRevert} onCancel={() => setRevertTarget(null)} />
    </PageLayout>
  )
}
