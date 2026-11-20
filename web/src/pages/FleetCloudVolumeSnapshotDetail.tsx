// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useParams } from 'react-router'
import { ArrowLeft, Camera, Loader2 } from 'lucide-react'
import {
  createVolumeFromSnapshot,
  deleteVolumeSnapshot,
  listAllVolumeSnapshots,
  type NativeVolumeSnapshotWithVolume,
} from '../api/nativeVolumes'
import ConfirmDialog from '../components/ConfirmDialog'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import PageSkeleton from '../components/PageSkeleton'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { useBreadcrumbName } from '../contexts/BreadcrumbNameContext'

// Native volume snapshots -- like the other rewired /fleet-cloud/* pages, this
// no longer depends on a wired external cloud. There's no
// the old external-cloud gate component wrapping it any more either: the daemon's
// external-cloud-client integration has since been fully removed.
export default function FleetCloudVolumeSnapshotDetailPage() {
  return <FleetCloudVolumeSnapshotDetailContent />
}

function FleetCloudVolumeSnapshotDetailContent() {
  const { id } = useParams<{ id: string }>()
  const toast = useToastContext()
  const [snapshot, setSnapshot] = useState<NativeVolumeSnapshotWithVolume | null>(null)
  const [loading, setLoading] = useState(true)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [creatingVolume, setCreatingVolume] = useState(false)
  useBreadcrumbName(snapshot?.name)
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    if (!id) return
    // Last-response-wins: only the newest load may commit so a stale fetch for a
    // prior snapshot can't overwrite the one now shown.
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setLoading(true)
    try {
      const list = await listAllVolumeSnapshots()
      if (!alive()) return
      setSnapshot(list.find((s) => s.id === id) ?? null)
    } catch (e: unknown) {
      if (!alive()) return
      toast.error(formatUserError(e))
      setSnapshot(null)
    } finally {
      if (alive()) setLoading(false)
    }
  }, [id, toast])

  useEffect(() => { void load() }, [load])

  if (loading) return <PageSkeleton />
  if (!snapshot) {
    return (
      <div className="space-y-4">
        <FleetCloudSubNav />
        <Link to="/fleet-cloud/volume-snapshots" className="text-[var(--accent)] hover:underline">Back</Link>
      </div>
    )
  }

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <Link to="/fleet-cloud/volume-snapshots" className="inline-flex items-center gap-2 text-[var(--text-muted)] hover:text-[var(--text-primary)] text-sm">
        <ArrowLeft className="w-4 h-4" /> Volume snapshots
      </Link>
      <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <Camera className="w-7 h-7 text-[var(--accent)]" />
        {snapshot.name || snapshot.id.slice(0, 12)}
      </h1>
      <dl className="grid sm:grid-cols-2 gap-4 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 text-sm">
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">ID</dt><dd className="font-mono text-[var(--text-primary)] mt-1 break-all">{snapshot.id}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Status</dt><dd className="text-[var(--text-primary)] mt-1">{snapshot.status}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Source volume</dt><dd className="font-mono text-xs mt-1">
          <Link to={`/fleet-cloud/volumes/${snapshot.volume_id}`} className="text-[var(--accent)] hover:underline">{snapshot.volume_name}</Link>
        </dd></div>
      </dl>
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          className="btn-primary text-sm disabled:opacity-50"
          disabled={creatingVolume}
          onClick={() => {
            void (async () => {
              setCreatingVolume(true)
              try {
                const vol = await createVolumeFromSnapshot(
                  snapshot.id,
                  `${snapshot.name || 'snap'}-vol`,
                )
                toast.success('Volume created from snapshot')
                window.location.href = `/fleet-cloud/volumes/${vol.id}`
              } catch (e: unknown) {
                toast.error(formatUserError(e))
              } finally {
                setCreatingVolume(false)
              }
            })()
          }}
        >
          {creatingVolume ? <Loader2 className="w-4 h-4 inline animate-spin" /> : null} Create volume
        </button>
        <button type="button" className="px-3 py-1.5 rounded-lg border border-red-500/50 text-red-600 text-sm"
          onClick={() => setConfirmDelete(true)}>Delete snapshot</button>
      </div>
      <ConfirmDialog
        open={confirmDelete}
        title="Delete snapshot"
        message={`Delete snapshot ${snapshot.name || snapshot.id}?`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setConfirmDelete(false)}
        onConfirm={async () => {
          setConfirmDelete(false)
          try {
            await deleteVolumeSnapshot(snapshot.id)
            toast.success('Deleted')
            window.location.href = '/fleet-cloud/volume-snapshots'
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}
      />
      <FleetCloudFooter />
    </PageLayout>
  )
}
