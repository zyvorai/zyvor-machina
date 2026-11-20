// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { ArrowLeft, Disc } from 'lucide-react'
import {
  createVolumeSnapshot,
  deleteVolume,
  deleteVolumeSnapshot,
  detachVolume,
  createVolumeFromSnapshot,
  extendVolume,
  getVolume,
  listVolumeSnapshots,
  type NativeVolume,
  type NativeVolumeSnapshot,
} from '../api/nativeVolumes'
import ConfirmDialog from '../components/ConfirmDialog'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import VolumeSettings from '../components/platform/VolumeSettings'
import TagEditor from '../components/platform/TagEditor'
import PageSkeleton from '../components/PageSkeleton'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses } from '../utils/semanticColors'
import { useBreadcrumbName } from '../contexts/BreadcrumbNameContext'

// Native volumes — no old external-cloud gate component in the way any more
// (the daemon's external-cloud-client integration has since been fully
// removed). Narrower than Cinder: no rename, no bootable flag, no
// upload-to-image (no native equivalent) — see api/nativeVolumes.ts.
export default function FleetCloudVolumeDetailPage() {
  return <FleetCloudVolumeDetailContent />
}

function FleetCloudVolumeDetailContent() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const toast = useToastContext()
  const [vol, setVol] = useState<NativeVolume | null>(null)
  const [snapshots, setSnapshots] = useState<NativeVolumeSnapshot[]>([])
  const [loading, setLoading] = useState(true)
  const [confirmDeleteVolume, setConfirmDeleteVolume] = useState(false)
  const [pendingDeleteSnapshot, setPendingDeleteSnapshot] = useState<NativeVolumeSnapshot | null>(null)
  useBreadcrumbName(vol?.name)
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    if (!id) return
    // Last-response-wins: only the newest load may commit so a stale fetch for a
    // prior volume can't overwrite the one now shown.
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setLoading(true)
    try {
      const [v, s] = await Promise.all([getVolume(id), listVolumeSnapshots(id).catch(() => [])])
      if (!alive()) return
      setVol(v)
      setSnapshots(s)
    } catch (e: unknown) {
      if (!alive()) return
      toast.error(formatUserError(e))
      setVol(null)
    } finally {
      if (alive()) setLoading(false)
    }
  }, [id, toast])

  useEffect(() => {
    void load()
  }, [load])

  if (loading) return <PageSkeleton />
  if (!vol) {
    return (
      <div className="space-y-4">
        <FleetCloudSubNav />
        <p className="text-[var(--text-muted)]">Volume not found.</p>
        <Link to="/fleet-cloud/volumes" className="text-[var(--accent)] hover:underline">Back</Link>
      </div>
    )
  }

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <Link to="/fleet-cloud/volumes" className="inline-flex items-center gap-2 text-[var(--text-muted)] hover:text-[var(--text-primary)] text-sm">
        <ArrowLeft className="w-4 h-4" /> Volumes
      </Link>
      <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <Disc className="w-7 h-7 text-[var(--accent)]" />
        {vol.name}
      </h1>
      <dl className="grid sm:grid-cols-2 gap-4 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 text-sm">
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">ID</dt><dd className="font-mono text-[var(--text-primary)] mt-1 break-all">{vol.ec2_id ? <>{vol.ec2_id}<span className="block text-xs text-[var(--text-faint)]">{vol.id}</span></> : vol.id}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Size</dt><dd className="text-[var(--text-primary)] mt-1">{vol.size_gib} GiB</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Status</dt><dd className="text-[var(--text-primary)] mt-1">{vol.status}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Class</dt><dd className="text-[var(--text-primary)] mt-1">{vol.volume_class}{vol.atlas_backed ? ' (Atlas)' : ''}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Attached</dt><dd className="mt-1 font-mono text-xs">
          {vol.attached_vm_id ? (
            <Link to={`/fleet-cloud/instances/${vol.attached_vm_id}`} className="text-[var(--accent)] hover:underline">{vol.attached_vm_id}</Link>
          ) : '—'}
        </dd></div>
        {vol.attached_device && <div><dt className="text-xs text-[var(--text-muted)] uppercase">Device</dt><dd className="font-mono text-[var(--text-primary)] mt-1">{vol.attached_device}</dd></div>}
      </dl>
      <div className="flex flex-wrap gap-2">
        {vol.attached_vm_id && (
          <button type="button" className={`px-3 py-1.5 rounded-lg border text-sm ${statusActionLinkClasses('warn')}`}
            onClick={async () => {
              try {
                await detachVolume(vol.id)
                toast.success('Detached')
                void load()
              } catch (e: unknown) { toast.error(formatUserError(e)) }
            }}>Detach</button>
        )}
        <button type="button" className="btn-secondary text-sm"
          onClick={async () => {
            const n = prompt('New size (GiB)', String(vol.size_gib + 1))
            if (!n) return
            const size = Number.parseInt(n, 10)
            if (!Number.isFinite(size) || size <= vol.size_gib) {
              toast.warning(`Enter a whole number of GiB greater than ${vol.size_gib}`)
              return
            }
            try {
              await extendVolume(vol.id, size)
              toast.success('Extended')
              void load()
            } catch (e: unknown) { toast.error(formatUserError(e)) }
          }}>Extend</button>
        <button type="button" className={`px-3 py-1.5 rounded-lg border text-sm ${statusActionLinkClasses('error')}`}
          onClick={() => setConfirmDeleteVolume(true)}>Delete</button>
      </div>
      <VolumeSettings key={vol.id} volume={vol} onChanged={() => void load()} />
      <TagEditor resourceType="volume" resourceId={vol.id} />
      <ConfirmDialog
        open={confirmDeleteVolume}
        title="Delete volume"
        message={`Delete volume ${vol.name}?`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setConfirmDeleteVolume(false)}
        onConfirm={async () => {
          setConfirmDeleteVolume(false)
          try {
            await deleteVolume(vol.id)
            toast.success('Deleted')
            navigate('/fleet-cloud/volumes')
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}
      />

      <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-medium text-[var(--text-secondary)]">Snapshots</h2>
          <button type="button" className="text-xs text-[var(--accent)] hover:underline"
            onClick={async () => {
              const n = prompt('Snapshot name', `${vol.name}-snap`)
              if (!n) return
              try {
                await createVolumeSnapshot(vol.id, n)
                toast.success('Snapshot requested')
                void load()
              } catch (e: unknown) { toast.error(formatUserError(e)) }
            }}>+ Snapshot</button>
        </div>
        {snapshots.length === 0 ? (
          <p className="text-sm text-[var(--text-muted)]">No snapshots.</p>
        ) : (
          <ul className="text-sm font-mono space-y-1">
            {snapshots.map((s) => (
              <li key={s.id} className="flex items-center gap-2">
                <span>{s.name}</span>
                <span className="text-[var(--text-muted)] text-xs">{s.status}</span>
                <button type="button" className="text-xs text-[var(--accent)] hover:underline ml-auto"
                  onClick={async () => {
                    const n = prompt('Name for the new volume', `${vol.name}-copy`)
                    if (!n) return
                    try {
                      const v = await createVolumeFromSnapshot(s.id, n)
                      toast.success('Volume created')
                      navigate(`/fleet-cloud/volumes/${v.id}`)
                    } catch (e: unknown) { toast.error(formatUserError(e)) }
                  }}>Create volume</button>
                <button type="button" className={statusActionLinkClasses('error', 'text-xs')}
                  onClick={() => setPendingDeleteSnapshot(s)}>Delete</button>
              </li>
            ))}
          </ul>
        )}
      </section>
      <ConfirmDialog
        open={!!pendingDeleteSnapshot}
        title="Delete snapshot"
        message={pendingDeleteSnapshot ? `Delete snapshot ${pendingDeleteSnapshot.name}?` : ''}
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
