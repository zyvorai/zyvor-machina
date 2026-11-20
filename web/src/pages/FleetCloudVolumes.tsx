// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import {
  attachVolume,
  createVolume,
  createVolumeSnapshot,
  deleteVolume,
  deleteVolumeSnapshot,
  detachVolume,
  extendVolume,
  listVolumeSnapshots,
  listVolumes,
  type NativeVolume,
  type NativeVolumeSnapshot,
} from '../api/nativeVolumes'
import { listVms, type NativeVm } from '../api/nativeVms'
import ConfirmDialog from '../components/ConfirmDialog'
import { useExpandable } from '../hooks/useExpandable'
import { ExpandableToggle } from '../components/ui/ExpandableToggle'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import PageSkeleton from '../components/PageSkeleton'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses } from '../utils/semanticColors'
import { HardDrive, RefreshCw } from 'lucide-react'
import { platformStatSubtitle } from '../components/platform/PlatformPageChrome'

// Native standalone volumes — no old external-cloud gate component in the
// way any more (the daemon's external-cloud-client integration has
// since been fully removed). Narrower than Cinder: no
// transfers/retype/clone/bootable-flag/create-from-image (no native
// equivalent yet) — see api/nativeVolumes.ts.
export default function FleetCloudVolumesPage() {
  return <FleetCloudVolumesContent />
}

function FleetCloudVolumesContent() {
  const toast = useToastContext()
  const [volumes, setVolumes] = useState<NativeVolume[]>([])
  const [snapshotsByVol, setSnapshotsByVol] = useState<Record<string, NativeVolumeSnapshot[]>>({})
  const [loading, setLoading] = useState(true)
  const [sizeGb, setSizeGb] = useState('10')
  const [name, setName] = useState('')
  const [creating, setCreating] = useState(false)
  const [attachVolId, setAttachVolId] = useState('')
  const [attachInstId, setAttachInstId] = useState('')
  const [instances, setInstances] = useState<NativeVm[]>([])
  const [search, setSearch] = useState('')
  const [pendingDeleteVolume, setPendingDeleteVolume] = useState<NativeVolume | null>(null)
  const [pendingDeleteSnapshot, setPendingDeleteSnapshot] = useState<NativeVolumeSnapshot | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const [v, inst] = await Promise.all([listVolumes(), listVms().catch(() => [])])
      setVolumes(v)
      setInstances(inst)
      if (!attachInstId && inst.length > 0) setAttachInstId(inst[0].id)
      const free = v.filter((vol) => !vol.attached_vm_id)
      if (!attachVolId && free.length > 0) setAttachVolId(free[0].id)
      const snaps = await Promise.all(v.map((vol) => listVolumeSnapshots(vol.id).catch(() => [])))
      const byVol: Record<string, NativeVolumeSnapshot[]> = {}
      v.forEach((vol, i) => { byVol[vol.id] = snaps[i] })
      setSnapshotsByVol(byVol)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setLoading(false)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [toast])

  useEffect(() => {
    void load()
  }, [load])

  const allSnapshots = Object.values(snapshotsByVol).flat()
  const snapshotList = useExpandable(allSnapshots, 20)

  const filteredVolumes = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return volumes
    return volumes.filter(
      (v) =>
        v.name.toLowerCase().includes(q) ||
        v.id.toLowerCase().includes(q) ||
        (v.attached_vm_id?.toLowerCase().includes(q) ?? false),
    )
  }, [volumes, search])
  const volumeList = useExpandable(filteredVolumes, 40)

  const handleCreate = async () => {
    if (creating) return
    const size = Number.parseInt(sizeGb, 10)
    if (!Number.isFinite(size) || size < 1) {
      toast.warning('Enter valid size in GB')
      return
    }
    setCreating(true)
    try {
      await createVolume({ size_gib: size, name: name.trim() || `vol-${Date.now()}` })
      toast.success('Volume created')
      setName('')
      void load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setCreating(false)
    }
  }

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <HardDrive className="w-7 h-7 text-[var(--accent)]" />
        Volumes
      </h1>
      {platformStatSubtitle([{ label: 'Volumes', value: volumes.length }, { label: 'Attached', value: volumes.filter((v) => v.attached_vm_id).length }, { label: 'Total GiB', value: volumes.reduce((a, v) => a + (v.size_gib || 0), 0) }])}
      <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 flex flex-wrap gap-3 items-end">
        <div>
          <label className="block text-xs text-[var(--text-muted)] mb-1">Size (GB)</label>
          <input type="number" min={1} value={sizeGb} onChange={(e) => setSizeGb(e.target.value)}
            aria-label="Size in GB"
            className="w-24 input-field text-sm" />
        </div>
        <div>
          <label className="block text-xs text-[var(--text-muted)] mb-1">Name</label>
          <input value={name} onChange={(e) => setName(e.target.value)}
            aria-label="Volume name"
            className="input-field text-sm" />
        </div>
        <button type="button" onClick={() => void handleCreate()} disabled={creating}
          className="btn-primary text-sm disabled:opacity-40 disabled:cursor-not-allowed">
          {creating ? 'Creating…' : 'Create volume'}
        </button>
        <button type="button" onClick={() => void load()}
          className="ml-auto btn-secondary text-sm inline-flex items-center gap-1">
          <RefreshCw className="w-4 h-4" /> Refresh
        </button>
      </div>

      <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3">
        <h2 className="text-sm font-medium text-[var(--text-secondary)]">Attach volume to instance</h2>
        <div className="flex flex-wrap gap-3 items-end">
          <div className="min-w-[14rem]">
            <label className="block text-xs text-[var(--text-muted)] mb-1">Volume</label>
            <select
              value={attachVolId}
              onChange={(e) => setAttachVolId(e.target.value)}
              aria-label="Volume to attach"
              className="w-full input-field text-sm"
            >
              <option value="">Select volume…</option>
              {volumes.filter((vol) => !vol.attached_vm_id).map((vol) => (
                <option key={vol.id} value={vol.id}>
                  {vol.name} ({vol.size_gib} GiB)
                </option>
              ))}
            </select>
          </div>
          <div className="min-w-[14rem]">
            <label className="block text-xs text-[var(--text-muted)] mb-1">Instance</label>
            <select
              value={attachInstId}
              onChange={(e) => setAttachInstId(e.target.value)}
              aria-label="Instance to attach to"
              className="w-full input-field text-sm"
            >
              <option value="">Select instance…</option>
              {instances.map((i) => (
                <option key={i.id} value={i.id}>{i.name}</option>
              ))}
            </select>
          </div>
          <button
            type="button"
            disabled={!attachVolId || !attachInstId}
            onClick={async () => {
              try {
                await attachVolume(attachVolId, { vm_id: attachInstId })
                toast.success('Volume attached')
                void load()
              } catch (e: unknown) {
                toast.error(formatUserError(e))
              }
            }}
            className="btn-primary text-sm disabled:opacity-40"
          >
            Attach
          </button>
        </div>
      </div>

      {loading ? (
        <PageSkeleton />
      ) : (
        <>
          <TahoeToolbar
            search={search}
            onSearchChange={setSearch}
            placeholder="Search name, ID, or attachment…"
          />

          <TahoeTableWrap>
            <table className="apple-table" aria-label="Volumes">
              <thead>
                <tr>
                  <th scope="col">Name</th>
                  <th scope="col">Size</th>
                  <th scope="col">Status</th>
                  <th scope="col">Attached</th>
                  <th scope="col">Actions</th>
                </tr>
              </thead>
              <tbody id={volumeList.listId}>
                {filteredVolumes.length === 0 && (
                  <tr>
                    <td colSpan={5} className="text-center text-[var(--text-muted)]">
                      {search.trim() ? 'No volumes match your search.' : 'No volumes in this project.'}
                    </td>
                  </tr>
                )}
                {volumeList.shown.map((v) => (
                  <tr key={v.id}>
                    <td className="font-mono text-[var(--text-primary)]">
                      <Link to={`/fleet-cloud/volumes/${v.id}`} className="apple-link hover:opacity-90">
                        {v.name}
                      </Link>
                    </td>
                    <td>{v.size_gib} GiB</td>
                    <td className="text-[var(--text-muted)]">{v.status}</td>
                    <td className="text-[var(--text-muted)] font-mono text-xs">
                      {v.attached_vm_id ? v.attached_vm_id.slice(0, 8) : '—'}
                    </td>
                    <td className="flex flex-wrap gap-2">
                      {v.attached_vm_id && (
                        <button
                          type="button"
                          className={statusActionLinkClasses('warn', 'text-xs')}
                          onClick={async () => {
                            try {
                              await detachVolume(v.id)
                              toast.success('Volume detached')
                              void load()
                            } catch (e: unknown) {
                              toast.error(formatUserError(e))
                            }
                          }}
                        >
                          Detach
                        </button>
                      )}
                      <button type="button" className="text-xs text-[var(--accent)] hover:underline"
                        onClick={async () => {
                          const n = prompt('Snapshot name', `${v.name}-snap`)
                          if (!n) return
                          try {
                            await createVolumeSnapshot(v.id, n)
                            toast.success('Snapshot requested')
                            void load()
                          } catch (e: unknown) {
                            toast.error(formatUserError(e))
                          }
                        }}>Snapshot</button>
                      <button type="button" className={statusActionLinkClasses('warn', 'text-xs')}
                        onClick={async () => {
                          const n = prompt('New size (GiB)', String(v.size_gib + 1))
                          if (!n) return
                          const size = Number.parseInt(n, 10)
                          if (!Number.isFinite(size) || size <= v.size_gib) {
                            toast.warning(`Enter a whole number of GiB greater than ${v.size_gib}`)
                            return
                          }
                          try {
                            await extendVolume(v.id, size)
                            toast.success('Extended')
                            void load()
                          } catch (e: unknown) {
                            toast.error(formatUserError(e))
                          }
                        }}>Extend</button>
                      <button type="button" className={statusActionLinkClasses('error', 'text-xs')}
                        onClick={() => setPendingDeleteVolume(v)}>Delete</button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
          {volumeList.showToggle && (
            <ExpandableToggle expanded={volumeList.expanded} hidden={volumeList.hidden} listId={volumeList.listId} onToggle={volumeList.toggle} noun="volumes" className="btn-secondary text-sm mt-2" />
          )}

          {allSnapshots.length > 0 && (
            <>
              <div className="flex items-center justify-between mb-2 mt-6">
                <h2 className="text-xs text-[var(--text-muted)] uppercase tracking-wide">Snapshots</h2>
                <Link to="/fleet-cloud/volume-snapshots" className="text-sm text-[var(--accent)] hover:underline">View all</Link>
              </div>
              <TahoeTableWrap>
                <table className="apple-table" aria-label="Volume snapshots">
                  <thead>
                    <tr>
                      <th scope="col">Name</th>
                      <th scope="col">Volume</th>
                      <th scope="col">Status</th>
                    </tr>
                  </thead>
                  <tbody className="font-mono text-xs" id={snapshotList.listId}>
                    {snapshotList.shown.map((s) => (
                      <tr key={s.id}>
                        <td className="text-[var(--text-primary)]">{s.name}</td>
                        <td className="text-[var(--text-muted)]">{s.volume_id.slice(0, 8)}</td>
                        <td>
                          <span className="text-[var(--text-muted)]">{s.status}</span>
                          <button
                            type="button"
                            className={statusActionLinkClasses('error', 'ml-2')}
                            onClick={() => setPendingDeleteSnapshot(s)}
                          >
                            Del
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </TahoeTableWrap>
              {snapshotList.showToggle && (
                <ExpandableToggle expanded={snapshotList.expanded} hidden={snapshotList.hidden} listId={snapshotList.listId} onToggle={snapshotList.toggle} noun="snapshots" className="btn-secondary text-sm mt-2" />
              )}
            </>
          )}
        </>
      )}
      <ConfirmDialog
        open={!!pendingDeleteVolume}
        title="Delete volume"
        message={pendingDeleteVolume ? `Delete volume ${pendingDeleteVolume.name}?` : ''}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setPendingDeleteVolume(null)}
        onConfirm={async () => {
          if (!pendingDeleteVolume) return
          const target = pendingDeleteVolume
          setPendingDeleteVolume(null)
          try {
            await deleteVolume(target.id)
            toast.success('Deleted')
            void load()
          } catch (e: unknown) {
            toast.error(formatUserError(e))
          }
        }}
      />
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
            toast.success('Snapshot deleted')
            void load()
          } catch (e: unknown) {
            toast.error(formatUserError(e))
          }
        }}
      />
      <FleetCloudFooter />
    </PageLayout>
  )
}
