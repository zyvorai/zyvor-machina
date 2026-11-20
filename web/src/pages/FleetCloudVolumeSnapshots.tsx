// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import {
  deleteVolumeSnapshot,
  listAllVolumeSnapshots,
  type NativeVolumeSnapshotWithVolume,
} from '../api/nativeVolumes'
import ConfirmDialog from '../components/ConfirmDialog'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses } from '../utils/semanticColors'
import { Camera, Loader2, RefreshCw } from 'lucide-react'

// Native volume snapshots (controller::api::volumes) -- like the other rewired
// /fleet-cloud/* pages, this no longer depends on a wired external cloud.
// There's no old external-cloud gate component wrapping it any more either:
// the daemon's external-cloud-client integration has since been fully
// removed. Snapshots require an Atlas-backed volume (ATLAS_ENABLED=1) -- see
// api/nativeVolumes.ts.
export default function FleetCloudVolumeSnapshotsPage() {
  return <FleetCloudVolumeSnapshotsContent />
}

function FleetCloudVolumeSnapshotsContent() {
  const toast = useToastContext()
  const [snapshots, setSnapshots] = useState<NativeVolumeSnapshotWithVolume[]>([])
  const [loading, setLoading] = useState(true)
  const [search, setSearch] = useState('')
  const [pendingDeleteSnapshot, setPendingDeleteSnapshot] = useState<NativeVolumeSnapshotWithVolume | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const list = await listAllVolumeSnapshots()
      setSnapshots(list)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => { void load() }, [load])

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return snapshots
    return snapshots.filter(
      (s) =>
        (s.name?.toLowerCase().includes(q) ?? false) ||
        s.id.toLowerCase().includes(q) ||
        s.volume_name.toLowerCase().includes(q) ||
        s.volume_id.toLowerCase().includes(q),
    )
  }, [snapshots, search])

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <div className="flex items-center justify-between gap-3">
        <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
          <Camera className="w-7 h-7 text-[var(--accent)]" />
          Storage volume snapshots
        </h1>
        <button type="button" onClick={() => void load()} className="btn-secondary text-sm inline-flex items-center gap-1">
          <RefreshCw className="w-4 h-4" /> Refresh
        </button>
      </div>
      {loading ? (
        <Loader2 className="w-8 h-8 animate-spin text-[var(--accent)] mx-auto" />
      ) : (
        <>
          <TahoeToolbar
            search={search}
            onSearchChange={setSearch}
            placeholder="Search name, volume, or ID…"
          />
          <TahoeTableWrap>
            <table className="apple-table" aria-label="Volume snapshots">
              <thead>
                <tr>
                  <th scope="col">Name</th>
                  <th scope="col">Volume</th>
                  <th scope="col">Status</th>
                  <th scope="col">Actions</th>
                </tr>
              </thead>
              <tbody className="font-mono text-xs">
                {filtered.length === 0 && (
                  <tr>
                    <td colSpan={4} className="text-center text-[var(--text-muted)]">
                      {search.trim() ? 'No snapshots match your search.' : 'No snapshots.'}
                    </td>
                  </tr>
                )}
                {filtered.map((s) => (
                  <tr key={s.id}>
                    <td className="text-[var(--text-primary)]">
                      <Link to={`/fleet-cloud/volume-snapshots/${s.id}`} className="apple-link">{s.name || s.id.slice(0, 8)}</Link>
                    </td>
                    <td>
                      <Link to={`/fleet-cloud/volumes/${s.volume_id}`} className="text-[var(--accent)] hover:underline">{s.volume_name}</Link>
                    </td>
                    <td className="text-[var(--text-muted)]">{s.status}</td>
                    <td className="flex flex-wrap gap-2">
                      <Link to={`/fleet-cloud/volume-snapshots/${s.id}`} className="text-[var(--accent)] hover:underline">Detail</Link>
                      <button type="button" className={statusActionLinkClasses('error')} onClick={() => setPendingDeleteSnapshot(s)}>Delete</button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        </>
      )}
      <ConfirmDialog
        open={!!pendingDeleteSnapshot}
        title="Delete snapshot"
        message={pendingDeleteSnapshot ? `Delete snapshot ${pendingDeleteSnapshot.name || pendingDeleteSnapshot.id}?` : ''}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setPendingDeleteSnapshot(null)}
        onConfirm={async () => {
          if (!pendingDeleteSnapshot) return
          const target = pendingDeleteSnapshot
          setPendingDeleteSnapshot(null)
          try {
            await deleteVolumeSnapshot(target.id)
            toast.success('Deleted')
            void load()
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}
      />
      <FleetCloudFooter />
    </PageLayout>
  )
}
