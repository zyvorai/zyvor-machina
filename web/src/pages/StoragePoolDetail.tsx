// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, useCallback, useRef } from 'react'
import { useParams, Link } from 'react-router'
import { listPools, listVolumes, startPool, stopPool, refreshPool, deleteVolume, setPoolAutostart, createVolume, StoragePoolInfo, StorageVolumeInfo } from '../api/storage'
import { getPoolXml, resizeVolume, cloneVolume } from '../api/advanced'
import { useToastContext } from '../contexts/ToastContext'
import ConfirmDialog from '../components/ConfirmDialog'
import PageLayout from '../components/PageLayout'
import EmptyState from '../components/EmptyState'
import { formatUserError } from '../utils/apiError'
import { poolStateBadgeClasses, statusActionLinkClasses, statusBgClass, statusToneClass, utilizationTone } from '../utils/semanticColors'
import {
  ArrowLeft, Play, Square, RefreshCw, Trash2, HardDrive, Plus, Code,
  X, Copy, Maximize, ToggleLeft, ToggleRight, Download,
} from 'lucide-react'

export default function StoragePoolDetail() {
  const { pool: poolName } = useParams<{ pool: string }>()
  const [pool, setPool] = useState<StoragePoolInfo | null>(null)
  const [volumes, setVolumes] = useState<StorageVolumeInfo[]>([])
  const [poolXml, setPoolXml] = useState('')
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [creatingVol, setCreatingVol] = useState(false)
  const [showXml, setShowXml] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<{ pool: string; vol: string } | null>(null)
  const [resizeTarget, setResizeTarget] = useState<{ pool: string; vol: string } | null>(null)
  const [resizeGb, setResizeGb] = useState('')
  const [cloneTarget, setCloneTarget] = useState<{ pool: string; vol: string } | null>(null)
  const [cloneName, setCloneName] = useState('')
  const [showCreateVol, setShowCreateVol] = useState(false)
  const [newVolName, setNewVolName] = useState('')
  const [newVolCapacity, setNewVolCapacity] = useState('10')
  const [newVolFormat, setNewVolFormat] = useState('qcow2')
  const toast = useToastContext()
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    if (!poolName) return
    // Last-response-wins: rapid navigation between pools can leave stale awaits
    // in flight; only the newest load may commit so pool A can't overwrite B.
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    try {
      setLoadError(null)
      const pools = await listPools()
      if (!alive()) return
      const found = pools.find((p) => p.name === poolName)
      setPool(found || null)
      if (found) {
        try { const v = await listVolumes(poolName); if (alive()) setVolumes(v) } catch { if (alive()) setVolumes([]) }
        try { const x = await getPoolXml(poolName); if (alive()) setPoolXml(x) } catch { if (alive()) setPoolXml('') }
      }
    } catch (e: unknown) {
      if (alive()) setLoadError(formatUserError(e))
    } finally {
      if (alive()) setLoading(false)
    }
  }, [poolName])

  useEffect(() => { load() }, [load])

  const poolAction = async (fn: (n: string) => Promise<void>, label: string) => {
    if (!poolName) return
    try { await fn(poolName); toast.success(`${label} OK`); load() } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
  }

  const toggleAutostart = async () => {
    if (!poolName || !pool) return
    try {
      await setPoolAutostart(poolName, !pool.autostart)
      toast.success(`Autostart ${!pool.autostart ? 'enabled' : 'disabled'}`)
      load()
    } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
  }

  const handleDeleteVol = async () => {
    if (!deleteTarget) return
    try { await deleteVolume(deleteTarget.pool, deleteTarget.vol); toast.success(`Deleted volume '${deleteTarget.vol}'`); load() } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
    setDeleteTarget(null)
  }

  const handleResize = async () => {
    if (!resizeTarget) return
    const gb = parseFloat(resizeGb)
    if (!Number.isFinite(gb) || gb <= 0) { toast.error('Enter a size in GB greater than 0'); return }
    try { await resizeVolume(resizeTarget.pool, resizeTarget.vol, gb); toast.success(`Resized volume '${resizeTarget.vol}'`); load() } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
    setResizeTarget(null); setResizeGb('')
  }

  const handleClone = async () => {
    if (!cloneTarget) return
    try { await cloneVolume(cloneTarget.pool, cloneTarget.vol, cloneName); toast.success(`Cloned volume '${cloneTarget.vol}' to '${cloneName}'`); load() } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
    setCloneTarget(null); setCloneName('')
  }

  const handleCreateVol = async () => {
    if (!poolName || !newVolName.trim() || creatingVol) return
    const cap = parseFloat(newVolCapacity)
    if (!Number.isFinite(cap) || cap <= 0) { toast.error('Enter a capacity in GB greater than 0'); return }
    setCreatingVol(true)
    try {
      await createVolume(poolName, { name: newVolName.trim(), capacity_gb: cap, format: newVolFormat })
      toast.success(`Created volume '${newVolName.trim()}'`)
      setShowCreateVol(false)
      setNewVolName('')
      load()
    } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
    finally { setCreatingVol(false) }
  }

  const downloadXml = () => {
    if (!poolXml || !pool) return
    const blob = new Blob([poolXml], { type: 'text/xml' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `${pool.name}-pool.xml`
    a.click()
    URL.revokeObjectURL(url)
    toast.success('XML downloaded')
  }

  if (!loading && !pool && loadError) {
    return (
      <EmptyState
        title="Failed to load storage pool"
        description={loadError}
        primaryAction={
          <button type="button" onClick={() => { setLoading(true); void load() }} className="btn-primary text-sm inline-flex items-center gap-2">
            <RefreshCw className="w-4 h-4" /> Retry
          </button>
        }
        secondaryAction={
          <Link to="/storage" className="btn-secondary text-sm inline-flex items-center gap-2">
            <ArrowLeft className="w-4 h-4" /> Back to Storage
          </Link>
        }
      />
    )
  }

  if (!loading && !pool) {
    return (
      <EmptyState
        title="Storage pool not found"
        description={poolName ? `No pool named '${poolName}' on this host.` : 'Pool name missing from URL.'}
        primaryAction={
          <Link to="/storage" className="btn-primary text-sm inline-flex items-center gap-2">
            <ArrowLeft className="w-4 h-4" /> Back to Storage
          </Link>
        }
      />
    )
  }

  const usagePct = pool && pool.capacity_gb > 0 ? (pool.allocation_gb / pool.capacity_gb * 100) : 0

  return (
    <PageLayout
      title={pool?.name ?? poolName ?? 'Storage pool'}
      subtitle={pool ? <span className="font-mono">{pool.uuid}</span> : undefined}
      icon={<HardDrive className="w-6 h-6 text-[var(--accent)]" />}
      actions={
        pool ? (
          <>
            <Link to="/storage" className="p-2 hover:bg-[var(--surface-hover)] rounded-lg transition" aria-label="Back to Storage"><ArrowLeft className="w-5 h-5" /></Link>
            <span className={poolStateBadgeClasses(pool.state)}>{pool.state}</span>
            {pool.state !== 'running' && (
              <button onClick={() => poolAction(startPool, 'Start pool')} className="px-3 py-1.5 bg-green-600 hover:bg-green-700 rounded-lg text-sm transition flex items-center gap-1"><Play className="w-4 h-4" /> Start</button>
            )}
            {pool.state === 'running' && (
              <>
                <button onClick={() => poolAction(refreshPool, 'Refresh pool')} className="px-3 py-1.5 bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] rounded-lg text-sm transition flex items-center gap-1"><RefreshCw className="w-4 h-4" /> Refresh</button>
                <button onClick={() => poolAction(stopPool, 'Stop pool')} className="px-3 py-1.5 bg-red-600 hover:bg-red-700 rounded-lg text-sm transition flex items-center gap-1"><Square className="w-4 h-4" /> Stop</button>
              </>
            )}
            <button onClick={toggleAutostart} className="px-3 py-1.5 bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] rounded-lg text-sm transition flex items-center gap-1">
              {pool.autostart ? <ToggleRight className={`w-4 h-4 ${statusToneClass('ok')}`} /> : <ToggleLeft className="w-4 h-4 text-[var(--text-muted)]" />}
              Autostart
            </button>
            <button onClick={load} className="p-2 hover:bg-[var(--surface-hover)] rounded-lg transition" aria-label="Reload"><RefreshCw className="w-4 h-4" /></button>
          </>
        ) : (
          <Link to="/storage" className="p-2 hover:bg-[var(--surface-hover)] rounded-lg transition" aria-label="Back to Storage"><ArrowLeft className="w-5 h-5" /></Link>
        )
      }
      contentLoading={loading}
    >
      {pool && (
      <>
      {/* Stats Row */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div className="bg-[var(--apple-surface)] rounded-xl p-5 border border-[var(--apple-hairline)] text-center">
          <div className="text-sm text-[var(--text-muted)] mb-1">Capacity</div>
          <div className="text-2xl font-bold">{(pool.capacity_gb ?? 0).toFixed(1)} <span className="text-sm text-[var(--text-muted)] font-normal">GB</span></div>
        </div>
        <div className="bg-[var(--apple-surface)] rounded-xl p-5 border border-[var(--apple-hairline)] text-center">
          <div className="text-sm text-[var(--text-muted)] mb-1">Used</div>
          <div className="text-2xl font-bold">{(pool.allocation_gb ?? 0).toFixed(1)} <span className="text-sm text-[var(--text-muted)] font-normal">GB</span></div>
        </div>
        <div className="bg-[var(--apple-surface)] rounded-xl p-5 border border-[var(--apple-hairline)] text-center">
          <div className="text-sm text-[var(--text-muted)] mb-1">Available</div>
          <div className={`text-2xl font-bold ${statusToneClass('ok')}`}>{(pool.available_gb ?? 0).toFixed(1)} <span className="text-sm text-[var(--text-muted)] font-normal">GB</span></div>
        </div>
      </div>

      {/* Capacity Bar */}
      {pool.capacity_gb > 0 && (
        <div className="bg-[var(--apple-surface)] rounded-xl p-5 border border-[var(--apple-hairline)]">
          <div className="flex justify-between text-sm text-[var(--text-muted)] mb-2">
            <span>Storage Usage</span>
            <span>{usagePct.toFixed(1)}%</span>
          </div>
          <div className="w-full bg-[var(--surface-hover)] rounded-full h-3">
            <div
              role="progressbar"
              aria-label="Storage usage"
              aria-valuenow={Math.round(Math.min(usagePct, 100))}
              aria-valuemin={0}
              aria-valuemax={100}
              className={`h-3 rounded-full transition-all ${statusBgClass(utilizationTone(usagePct))}`}
              style={{ width: `${Math.min(usagePct, 100)}%` }}
            />
          </div>
          <div className="flex justify-between text-xs text-[var(--text-muted)] mt-1">
            <span>{pool.allocation_gb.toFixed(1)} GB used</span>
            <span>{pool.available_gb.toFixed(1)} GB free</span>
          </div>
        </div>
      )}

      {/* Volumes Table */}
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-semibold">Volumes ({volumes.length})</h2>
          <button onClick={() => setShowCreateVol(true)} className="btn-primary text-sm inline-flex items-center gap-1"><Plus className="w-4 h-4" /> Create Volume</button>
        </div>
        <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
          {volumes.length === 0 ? (
            <EmptyState title="No volumes" description="This pool is empty. Create a volume to get started." />
          ) : (
            <table className="w-full" aria-label="Storage volumes">
              <thead><tr className="border-b border-[var(--apple-hairline)] text-left text-sm text-[var(--text-muted)]"><th scope="col" className="px-6 py-3">Name</th><th scope="col" className="px-6 py-3">Type</th><th scope="col" className="px-6 py-3">Capacity</th><th scope="col" className="px-6 py-3">Used</th><th scope="col" className="px-6 py-3 hidden lg:table-cell">Path</th><th scope="col" className="px-6 py-3 text-right">Actions</th></tr></thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/50">
                {volumes.map((v) => (
                  <tr key={v.name} className="hover:bg-[var(--surface-hover)]/50">
                    <td className="px-6 py-3 font-medium">{v.name}</td>
                    <td className="px-6 py-3 text-sm text-[var(--text-muted)]">{v.vol_type}</td>
                    <td className="px-6 py-3 text-sm">{v.capacity_gb.toFixed(2)} GB</td>
                    <td className="px-6 py-3 text-sm">{v.allocation_gb.toFixed(2)} GB</td>
                    <td className="px-6 py-3 text-sm text-[var(--text-muted)] truncate max-w-xs hidden lg:table-cell">{v.path}</td>
                    <td className="px-6 py-3 text-right">
                      <div className="flex items-center justify-end gap-1">
                        <button onClick={() => { setResizeTarget({ pool: pool.name, vol: v.name }); setResizeGb(v.capacity_gb.toFixed(2)) }} className="p-1.5 hover:bg-white/10 rounded transition" title="Resize" aria-label="Resize"><Maximize className={`w-4 h-4 ${statusToneClass('info')}`} /></button>
                        <button onClick={() => { setCloneTarget({ pool: pool.name, vol: v.name }); setCloneName(`${v.name}-clone`) }} className="p-1.5 hover:bg-green-600/20 rounded transition" title="Clone" aria-label="Clone"><Copy className={`w-4 h-4 ${statusToneClass('ok')}`} /></button>
                        <button onClick={() => setDeleteTarget({ pool: pool.name, vol: v.name })} className="p-1.5 hover:bg-red-600/20 rounded transition" title="Delete" aria-label="Delete"><Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} /></button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>

      {/* XML Section */}
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-semibold flex items-center gap-2"><Code className={`w-5 h-5 ${statusToneClass('info')}`} /> Pool XML</h2>
          <button onClick={() => setShowXml(!showXml)} className={`text-sm transition ${statusActionLinkClasses('info')}`}>
            {showXml ? 'Hide' : 'Show'}
          </button>
        </div>
        {showXml && (
          <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
            <div className="px-6 py-3 border-b border-[var(--apple-hairline)] flex items-center justify-between">
              <span className="text-sm text-[var(--text-muted)]">Pool XML Configuration</span>
              <div className="flex items-center gap-3">
                <button onClick={downloadXml} className={`text-xs transition flex items-center gap-1 ${statusActionLinkClasses('info')}`}><Download className="w-3 h-3" /> Download</button>
                <button onClick={() => { if (poolXml) navigator.clipboard.writeText(poolXml).then(() => toast.success('XML copied')) }} className={`text-xs transition flex items-center gap-1 ${statusActionLinkClasses('info')}`}><Copy className="w-3 h-3" /> Copy</button>
              </div>
            </div>
            <pre className="p-6 text-xs font-mono text-[var(--text-secondary)] overflow-x-auto max-h-[400px] whitespace-pre">{poolXml || 'Loading...'}</pre>
          </div>
        )}
      </div>

      {/* Dialogs */}
      <ConfirmDialog open={!!deleteTarget} title="Delete Volume" message={`Delete volume '${deleteTarget?.vol}'?`} confirmLabel="Delete" onConfirm={handleDeleteVol} onCancel={() => setDeleteTarget(null)} />

      {resizeTarget && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" onClick={() => setResizeTarget(null)}>
          <div className="bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-2xl shadow-2xl w-full max-w-md mx-4" onClick={(e) => e.stopPropagation()}>
            <div className="p-5 border-b border-[var(--apple-hairline)] flex items-center justify-between">
              <span className="text-lg font-semibold">Resize Volume</span>
              <button aria-label="Close" onClick={() => setResizeTarget(null)} className="p-1 hover:bg-[var(--surface-hover)] rounded transition"><X className="w-4 h-4 text-[var(--text-muted)]" /></button>
            </div>
            <div className="p-5 space-y-4">
              <div className="text-sm text-[var(--text-muted)]">Volume: <span className="text-[var(--text-primary)] font-medium">{resizeTarget.vol}</span></div>
              <div>
                <label className="block text-sm text-[var(--text-muted)] mb-1">New Size (GB)</label>
                <input aria-label="New size in gigabytes" type="number" step="0.01" min="0.01" value={resizeGb} onChange={(e) => setResizeGb(e.target.value)} className="w-full px-3 py-2 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm" />
              </div>
            </div>
            <div className="flex justify-end gap-3 px-5 pb-5">
              <button onClick={() => setResizeTarget(null)} className="btn-secondary text-sm font-medium transition">Cancel</button>
              <button onClick={handleResize} disabled={!(parseFloat(resizeGb) > 0)} className="btn-primary text-sm disabled:opacity-40 disabled:cursor-not-allowed">Resize</button>
            </div>
          </div>
        </div>
      )}

      {cloneTarget && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" onClick={() => setCloneTarget(null)}>
          <div className="bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-2xl shadow-2xl w-full max-w-md mx-4" onClick={(e) => e.stopPropagation()}>
            <div className="p-5 border-b border-[var(--apple-hairline)] flex items-center justify-between">
              <span className="text-lg font-semibold">Clone Volume</span>
              <button aria-label="Close" onClick={() => setCloneTarget(null)} className="p-1 hover:bg-[var(--surface-hover)] rounded transition"><X className="w-4 h-4 text-[var(--text-muted)]" /></button>
            </div>
            <div className="p-5 space-y-4">
              <div className="text-sm text-[var(--text-muted)]">Source: <span className="text-[var(--text-primary)] font-medium">{cloneTarget.vol}</span></div>
              <div>
                <label className="block text-sm text-[var(--text-muted)] mb-1">New Volume Name</label>
                <input aria-label="New volume name" type="text" value={cloneName} onChange={(e) => setCloneName(e.target.value)} className="w-full px-3 py-2 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm" />
              </div>
            </div>
            <div className="flex justify-end gap-3 px-5 pb-5">
              <button onClick={() => setCloneTarget(null)} className="btn-secondary text-sm font-medium transition">Cancel</button>
              <button onClick={handleClone} className="px-4 py-2 bg-green-600 hover:bg-green-500 rounded-lg text-sm text-white font-medium transition">Clone</button>
            </div>
          </div>
        </div>
      )}

      {showCreateVol && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" onClick={() => setShowCreateVol(false)}>
          <div className="bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-2xl shadow-2xl w-full max-w-md mx-4" onClick={(e) => e.stopPropagation()}>
            <div className="p-5 border-b border-[var(--apple-hairline)] flex items-center justify-between">
              <span className="text-lg font-semibold">Create Volume</span>
              <button aria-label="Close" onClick={() => setShowCreateVol(false)} className="p-1 hover:bg-[var(--surface-hover)] rounded transition"><X className="w-4 h-4 text-[var(--text-muted)]" /></button>
            </div>
            <div className="p-5 space-y-4">
              <div>
                <label className="block text-sm text-[var(--text-muted)] mb-1">Name</label>
                <input aria-label="Name" type="text" value={newVolName} onChange={(e) => setNewVolName(e.target.value)} placeholder="my-volume.qcow2" className="w-full px-3 py-2 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm" />
              </div>
              <div>
                <label className="block text-sm text-[var(--text-muted)] mb-1">Capacity (GB)</label>
                <input aria-label="Volume capacity in gigabytes" type="number" step="0.01" min="0.01" value={newVolCapacity} onChange={(e) => setNewVolCapacity(e.target.value)} className="w-full px-3 py-2 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm" />
              </div>
              <div>
                <label className="block text-sm text-[var(--text-muted)] mb-1">Format</label>
                <select aria-label="Volume format" value={newVolFormat} onChange={(e) => setNewVolFormat(e.target.value)} className="w-full px-3 py-2 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm">
                  <option value="qcow2">qcow2</option>
                  <option value="raw">raw</option>
                  <option value="qcow">qcow</option>
                </select>
              </div>
            </div>
            <div className="flex justify-end gap-3 px-5 pb-5">
              <button onClick={() => setShowCreateVol(false)} className="btn-secondary text-sm font-medium transition">Cancel</button>
              <button onClick={handleCreateVol} disabled={creatingVol || !newVolName.trim() || !(parseFloat(newVolCapacity) > 0)} className="btn-primary text-sm disabled:opacity-40 disabled:cursor-not-allowed">{creatingVol ? 'Creating…' : 'Create'}</button>
            </div>
          </div>
        </div>
      )}
      </>
      )}
    </PageLayout>
  )
}
