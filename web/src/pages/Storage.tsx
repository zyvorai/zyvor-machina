// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, useCallback } from 'react'
import { Link } from 'react-router'
import { listPools, listVolumes, startPool, stopPool, refreshPool, deleteVolume, setPoolAutostart, createVolume, StoragePoolInfo, StorageVolumeInfo } from '../api/storage'
import { createPool, deletePool, getPoolXml, resizeVolume, cloneVolume } from '../api/advanced'
import { useToastContext } from '../contexts/ToastContext'
import ConfirmDialog from '../components/ConfirmDialog'
import { GlassModal } from '../components/glass/GlassModal'
import { Play, Square, RefreshCw, Trash2, ArrowLeft, HardDrive, Plus, Code, Copy, Maximize, ToggleLeft, ToggleRight } from 'lucide-react'
import { formatUserError } from '../utils/apiError'
import { poolStateBadgeClasses, statusBadgeClasses, statusToneClass } from '../utils/semanticColors'
import PageLayout from '../components/PageLayout'
import EmptyState from '../components/EmptyState'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { libvirtErrorHints } from '../utils/libvirtHints'

export default function StoragePage() {
  const [pools, setPools] = useState<StoragePoolInfo[]>([])
  const [search, setSearch] = useState('')
  const [volumes, setVolumes] = useState<StorageVolumeInfo[]>([])
  const [selectedPool, setSelectedPool] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [volumesLoading, setVolumesLoading] = useState(false)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<{ pool: string; vol: string } | null>(null)
  const [deletePoolTarget, setDeletePoolTarget] = useState<string | null>(null)
  const [showCreatePool, setShowCreatePool] = useState(false)
  const [newPoolName, setNewPoolName] = useState('')
  const [newPoolType, setNewPoolType] = useState('dir')
  const [newPoolPath, setNewPoolPath] = useState('')
  const [xmlContent, setXmlContent] = useState<string | null>(null)
  const [xmlName, setXmlName] = useState('')
  const [resizeTarget, setResizeTarget] = useState<{ pool: string; vol: string } | null>(null)
  const [resizeGb, setResizeGb] = useState('')
  const [cloneTarget, setCloneTarget] = useState<{ pool: string; vol: string } | null>(null)
  const [cloneName, setCloneName] = useState('')
  const [showCreateVol, setShowCreateVol] = useState(false)
  const [newVolName, setNewVolName] = useState('')
  const [newVolCapacity, setNewVolCapacity] = useState('10')
  const [newVolFormat, setNewVolFormat] = useState('qcow2')
  const [creatingPool, setCreatingPool] = useState(false)
  const [creatingVol, setCreatingVol] = useState(false)
  const toast = useToastContext()

  const loadPools = useCallback(async () => {
    try {
      setLoading(true)
      setLoadError(null)
      setPools(await listPools())
    } catch (e: unknown) {
      const msg = formatUserError(e)
      setLoadError(msg)
      toast.error(msg)
    } finally { setLoading(false) }
  }, [toast])

  const loadVolumes = async (pool: string) => {
    try {
      setVolumesLoading(true)
      setSelectedPool(pool)
      setVolumes(await listVolumes(pool))
    } catch (e: unknown) {
      toast.error(`${formatUserError(e)}`)
      setSelectedPool(null)
      setVolumes([])
    } finally {
      setVolumesLoading(false)
    }
  }

  useEffect(() => { loadPools() }, [loadPools])

  const poolAction = async (name: string, fn: (n: string) => Promise<void>, label: string) => {
    try { await fn(name); toast.success(`${label} '${name}' OK`); loadPools() } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
  }

  const handleDeleteVol = async () => {
    if (!deleteTarget) return
    try { await deleteVolume(deleteTarget.pool, deleteTarget.vol); toast.success(`Deleted volume '${deleteTarget.vol}'`); loadVolumes(deleteTarget.pool) } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
    setDeleteTarget(null)
  }

  const handleDeletePool = async () => {
    if (!deletePoolTarget) return
    try { await deletePool(deletePoolTarget); toast.success(`Deleted pool '${deletePoolTarget}'`); loadPools() } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
    setDeletePoolTarget(null)
  }

  const handleCreatePool = async () => {
    if (!newPoolName.trim() || creatingPool) return
    setCreatingPool(true)
    try { await createPool({ name: newPoolName.trim(), pool_type: newPoolType, target_path: newPoolPath }); toast.success(`Created pool '${newPoolName.trim()}'`); setShowCreatePool(false); setNewPoolName(''); setNewPoolPath(''); loadPools() } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
    finally { setCreatingPool(false) }
  }

  const showPoolXml = async (name: string) => {
    try { const xml = await getPoolXml(name); setXmlContent(xml); setXmlName(name) } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
  }

  const handleResize = async () => {
    if (!resizeTarget) return
    const gb = parseFloat(resizeGb)
    if (!Number.isFinite(gb) || gb <= 0) { toast.error('Enter a size in GB greater than 0'); return }
    try { await resizeVolume(resizeTarget.pool, resizeTarget.vol, gb); toast.success(`Resized volume '${resizeTarget.vol}'`); loadVolumes(resizeTarget.pool) } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
    setResizeTarget(null); setResizeGb('')
  }

  const handleClone = async () => {
    if (!cloneTarget) return
    try { await cloneVolume(cloneTarget.pool, cloneTarget.vol, cloneName); toast.success(`Cloned volume '${cloneTarget.vol}' to '${cloneName}'`); loadVolumes(cloneTarget.pool) } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
    setCloneTarget(null); setCloneName('')
  }

  const togglePoolAutostart = async (pool: StoragePoolInfo) => {
    try {
      await setPoolAutostart(pool.name, !pool.autostart)
      toast.success(`Autostart ${!pool.autostart ? 'enabled' : 'disabled'} for '${pool.name}'`)
      loadPools()
    } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
  }

  const handleCreateVol = async () => {
    if (!selectedPool || !newVolName.trim() || creatingVol) return
    const cap = parseFloat(newVolCapacity)
    if (!Number.isFinite(cap) || cap <= 0) { toast.error('Enter a capacity in GB greater than 0'); return }
    setCreatingVol(true)
    try {
      await createVolume(selectedPool, { name: newVolName.trim(), capacity_gb: cap, format: newVolFormat })
      toast.success(`Created volume '${newVolName.trim()}'`)
      setShowCreateVol(false)
      setNewVolName('')
      loadVolumes(selectedPool)
    } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
    finally { setCreatingVol(false) }
  }

  const filterBySearch = (name: string, extra = '') => {
    const q = search.trim().toLowerCase()
    if (!q) return true
    return name.toLowerCase().includes(q) || extra.toLowerCase().includes(q)
  }

  const filteredVolumes = volumes.filter((v) => filterBySearch(v.name, v.path))
  const filteredPools = pools.filter((p) => filterBySearch(p.name, p.state))

  if (selectedPool) {
    return (
      <PageLayout
      eyebrow="Hypervisor"
        title={`Volumes in '${selectedPool}'`}
        actions={
          <>
            <button onClick={() => { setSelectedPool(null); setVolumes([]); setVolumesLoading(false) }} className="p-2 hover:bg-[var(--surface-hover)] rounded transition" title="Back" aria-label="Back"><ArrowLeft className="w-5 h-5" /></button>
            <button onClick={() => loadVolumes(selectedPool)} className="p-2 hover:bg-[var(--surface-hover)] rounded transition" title="Refresh" aria-label="Refresh"><RefreshCw className="w-4 h-4" /></button>
            <button onClick={() => setShowCreateVol(true)} className="btn-primary text-sm inline-flex items-center gap-1"><Plus className="w-4 h-4" /> Create Volume</button>
          </>
        }
        contentLoading={volumesLoading}
      >
        <TahoeToolbar
          search={search}
          onSearchChange={setSearch}
          placeholder="Search volumes…"
          trailing={
            search ? (
              <span aria-live="polite" className="text-sm text-[var(--text-muted)] shrink-0 pr-2">
                {filteredVolumes.length} volume{filteredVolumes.length !== 1 ? 's' : ''}
              </span>
            ) : null
          }
        />

        {volumes.length === 0 ? (
          <EmptyState
            icon={<HardDrive className="w-6 h-6" />}
            title="No volumes"
            description="Create a volume in this pool to attach to a VM."
            primaryAction={
              <button
                type="button"
                onClick={() => setShowCreateVol(true)}
                className="btn-primary text-sm"
              >
                Create volume
              </button>
            }
          />
        ) : (
          <TahoeTableWrap>
            <table className="apple-table" aria-label="Storage volumes">
              <thead><tr><th scope="col">Name</th><th scope="col">Type</th><th scope="col">Capacity</th><th scope="col">Used</th><th scope="col" className="hidden lg:table-cell">Path</th><th scope="col" className="text-right">Actions</th></tr></thead>
              <tbody>
                {filteredVolumes.length === 0 && (
                  <tr><td colSpan={6} className="text-center text-[var(--text-muted)]">No volumes match your search.</td></tr>
                )}
                {filteredVolumes.map((v) => (
                  <tr key={v.name}>
                    <td className="font-medium">{v.name}</td>
                    <td className="text-sm text-[var(--text-muted)]">{v.vol_type}</td>
                    <td className="text-sm">{v.capacity_gb.toFixed(2)} GB</td>
                    <td className="text-sm">{v.allocation_gb.toFixed(2)} GB</td>
                    <td className="text-sm text-[var(--text-muted)] truncate max-w-xs hidden lg:table-cell">{v.path}</td>
                    <td className="text-right">
                      <div className="flex items-center justify-end gap-1">
                        <button onClick={() => { setResizeTarget({ pool: selectedPool, vol: v.name }); setResizeGb(v.capacity_gb.toFixed(2)) }} className={`p-1.5 rounded transition ${statusBadgeClasses('info')} hover:opacity-80`} title="Resize" aria-label="Resize"><Maximize className={`w-4 h-4 ${statusToneClass('info')}`} /></button>
                        <button onClick={() => { setCloneTarget({ pool: selectedPool, vol: v.name }); setCloneName(`${v.name}-clone`) }} className="p-1.5 hover:bg-green-600/20 rounded transition" title="Clone" aria-label="Clone"><Copy className={`w-4 h-4 ${statusToneClass('ok')}`} /></button>
                        <button onClick={() => setDeleteTarget({ pool: selectedPool, vol: v.name })} className="p-1.5 hover:bg-red-600/20 rounded transition" title="Delete" aria-label="Delete"><Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} /></button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        )}
        <ConfirmDialog open={!!deleteTarget} title="Delete Volume" message={`Delete volume '${deleteTarget?.vol}'?`} confirmLabel="Delete" onConfirm={handleDeleteVol} onCancel={() => setDeleteTarget(null)} />

        {resizeTarget && (
          <GlassModal open onClose={() => setResizeTarget(null)} title="Resize Volume"
            footer={<div className="flex justify-end gap-3">
                <button onClick={() => setResizeTarget(null)} className="btn-secondary text-sm font-medium transition">Cancel</button>
                <button onClick={handleResize} disabled={!(parseFloat(resizeGb) > 0)} className="btn-primary text-sm disabled:opacity-40 disabled:cursor-not-allowed">Resize</button>
            </div>}>
            <div className="space-y-4">
                <div className="text-sm text-[var(--text-muted)]">Volume: <span className="text-[var(--text-primary)] font-medium">{resizeTarget.vol}</span></div>
                <div>
                  <label className="block text-sm text-[var(--text-muted)] mb-1">New Size (GB)</label>
                  <input aria-label="New Size (GB)" type="number" step="0.01" min="0.01" value={resizeGb} onChange={(e) => setResizeGb(e.target.value)} className="w-full px-3 py-2 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm" />
                </div>
            </div>
          </GlassModal>
        )}

        {cloneTarget && (
          <GlassModal open onClose={() => setCloneTarget(null)} title="Clone Volume"
            footer={<div className="flex justify-end gap-3">
                <button onClick={() => setCloneTarget(null)} className="btn-secondary text-sm font-medium transition">Cancel</button>
                <button onClick={handleClone} className="btn-primary text-sm">Clone</button>
            </div>}>
            <div className="space-y-4">
                <div className="text-sm text-[var(--text-muted)]">Source: <span className="text-[var(--text-primary)] font-medium">{cloneTarget.vol}</span></div>
                <div>
                  <label className="block text-sm text-[var(--text-muted)] mb-1">New Volume Name</label>
                  <input aria-label="New Volume Name" type="text" value={cloneName} onChange={(e) => setCloneName(e.target.value)} className="w-full px-3 py-2 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm" />
                </div>
            </div>
          </GlassModal>
        )}

        {showCreateVol && (
          <GlassModal open onClose={() => setShowCreateVol(false)} title="Create Volume"
            footer={<div className="flex justify-end gap-3">
                <button onClick={() => setShowCreateVol(false)} className="btn-secondary text-sm font-medium transition">Cancel</button>
                <button onClick={handleCreateVol} disabled={creatingVol || !newVolName.trim() || !(parseFloat(newVolCapacity) > 0)} className="btn-primary text-sm disabled:opacity-40 disabled:cursor-not-allowed">{creatingVol ? 'Creating…' : 'Create'}</button>
            </div>}>
            <div className="space-y-4">
                <div>
                  <label className="block text-sm text-[var(--text-muted)] mb-1">Name</label>
                  <input aria-label="Name" type="text" value={newVolName} onChange={(e) => setNewVolName(e.target.value)} placeholder="my-volume.qcow2" className="w-full px-3 py-2 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm" />
                </div>
                <div>
                  <label className="block text-sm text-[var(--text-muted)] mb-1">Capacity (GB)</label>
                  <input aria-label="Capacity (GB)" type="number" step="0.01" min="0.01" value={newVolCapacity} onChange={(e) => setNewVolCapacity(e.target.value)} className="w-full px-3 py-2 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm" />
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
          </GlassModal>
        )}
      </PageLayout>
    )
  }

  return (
    <PageLayout
      eyebrow="Hypervisor"
      title="Storage Pools"
      icon={<HardDrive className="w-6 h-6" />}
      actions={
        <>
          <button onClick={() => setShowCreatePool(true)} className="btn-primary text-sm inline-flex items-center gap-1"><Plus className="w-4 h-4" /> Create Pool</button>
          <button onClick={loadPools} className="p-2 hover:bg-[var(--surface-hover)] rounded transition" title="Refresh" aria-label="Refresh"><RefreshCw className="w-4 h-4" /></button>
        </>
      }
      contentLoading={loading}
      error={loadError}
      errorTitle="Could not load storage pools"
      errorHints={loadError ? libvirtErrorHints(loadError) : undefined}
      onErrorRetry={loadPools}
    >
      <TahoeToolbar
        search={search}
        onSearchChange={setSearch}
        placeholder="Search pools…"
        trailing={
          search ? (
            <span aria-live="polite" className="text-sm text-[var(--text-muted)] shrink-0 pr-2">
              {filteredPools.length} pool{filteredPools.length !== 1 ? 's' : ''}
            </span>
          ) : null
        }
      />

      {pools.length === 0 && !loadError ? (
        <EmptyState
          icon={<HardDrive className="w-6 h-6" />}
          title="No storage pools"
          description="Create a libvirt pool or import from an existing path on this host."
          primaryAction={<button type="button" className="btn-primary" onClick={() => setShowCreatePool(true)}>Create pool</button>}
        />
      ) : pools.length > 0 ? (
      <TahoeTableWrap>
        <table className="apple-table" aria-label="Storage pools">
          <thead>
            <tr>
              <th scope="col">Name</th>
              <th scope="col">State</th>
              <th scope="col">Capacity</th>
              <th scope="col">Used</th>
              <th scope="col" className="hidden md:table-cell">Available</th>
              <th scope="col" className="hidden lg:table-cell">Usage</th>
              <th scope="col">Autostart</th>
              <th scope="col" className="text-right">Actions</th>
            </tr>
          </thead>
          <tbody>
            {filteredPools.length === 0 && (
              <tr><td colSpan={8} className="text-center text-[var(--text-muted)]">No pools match your search.</td></tr>
            )}
            {filteredPools.map((pool) => {
              const usagePct = pool.capacity_gb > 0 ? Math.min(100, pool.allocation_gb / pool.capacity_gb * 100) : 0
              return (
                <tr key={pool.name}>
                  <td>
                    <div className="flex items-center gap-2 font-medium">
                      <HardDrive className="w-4 h-4 text-[var(--accent)] shrink-0" />
                      {pool.name}
                    </div>
                  </td>
                  <td><span className={poolStateBadgeClasses(pool.state)}>{pool.state}</span></td>
                  <td>{pool.capacity_gb.toFixed(1)} GB</td>
                  <td>{pool.allocation_gb.toFixed(1)} GB</td>
                  <td className="hidden md:table-cell">{pool.available_gb.toFixed(1)} GB</td>
                  <td className="hidden lg:table-cell">
                    {pool.capacity_gb > 0 ? (
                      <div className="flex items-center gap-2 min-w-[6rem]">
                        <div className="flex-1 bg-[var(--surface-hover)] rounded-full h-2">
                          <div className="bg-[var(--accent)] h-2 rounded-full" style={{ width: `${usagePct.toFixed(0)}%` }} />
                        </div>
                        <span className="text-xs text-[var(--text-muted)] tabular-nums">{usagePct.toFixed(0)}%</span>
                      </div>
                    ) : '—'}
                  </td>
                  <td>
                    <button onClick={() => togglePoolAutostart(pool)} className="p-1 hover:bg-white/10 rounded transition" title={pool.autostart ? 'Disable Autostart' : 'Enable Autostart'} aria-label={pool.autostart ? 'Disable Autostart' : 'Enable Autostart'}>
                      {pool.autostart ? <ToggleRight className={`w-4 h-4 ${statusToneClass('ok')}`} /> : <ToggleLeft className="w-4 h-4 text-[var(--text-muted)]" />}
                    </button>
                  </td>
                  <td>
                    <div className="flex items-center justify-end gap-1">
                      <Link to={`/storage/${encodeURIComponent(pool.name)}`} className="btn-secondary text-xs px-2 py-1 transition">Browse</Link>
                      {pool.state !== 'running' && <button aria-label="Start pool" onClick={() => poolAction(pool.name, startPool, 'Start pool')} className="p-1.5 hover:bg-green-600/20 rounded transition"><Play className={`w-4 h-4 ${statusToneClass('ok')}`} /></button>}
                      {pool.state === 'running' && (
                        <>
                          <button aria-label="Refresh pool" onClick={() => poolAction(pool.name, refreshPool, 'Refresh pool')} className={`p-1.5 rounded transition ${statusBadgeClasses('info')} hover:opacity-80`}><RefreshCw className={`w-4 h-4 ${statusToneClass('info')}`} /></button>
                          <button aria-label="Stop pool" onClick={() => poolAction(pool.name, stopPool, 'Stop pool')} className="p-1.5 hover:bg-red-600/20 rounded transition"><Square className={`w-4 h-4 ${statusToneClass('error')}`} /></button>
                        </>
                      )}
                      <button onClick={() => showPoolXml(pool.name)} className={`p-1.5 rounded transition ${statusBadgeClasses('info')} hover:opacity-80`} title="View XML" aria-label="View XML"><Code className={`w-4 h-4 ${statusToneClass('info')}`} /></button>
                      <button onClick={() => setDeletePoolTarget(pool.name)} className="p-1.5 hover:bg-red-600/20 rounded transition" title="Delete Pool" aria-label="Delete Pool"><Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} /></button>
                    </div>
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </TahoeTableWrap>
      ) : null}

      <ConfirmDialog open={!!deletePoolTarget} title="Delete Pool" message={`Delete storage pool '${deletePoolTarget}'? This cannot be undone.`} confirmLabel="Delete" onConfirm={handleDeletePool} onCancel={() => setDeletePoolTarget(null)} />

      {showCreatePool && (
        <GlassModal open onClose={() => setShowCreatePool(false)} title="Create Storage Pool"
          footer={<div className="flex justify-end gap-3">
              <button onClick={() => setShowCreatePool(false)} className="btn-secondary text-sm font-medium transition">Cancel</button>
              <button onClick={handleCreatePool} disabled={creatingPool || !newPoolName.trim()} className="btn-primary text-sm disabled:opacity-40 disabled:cursor-not-allowed">{creatingPool ? 'Creating…' : 'Create'}</button>
          </div>}>
          <div className="space-y-4">
              <div>
                <label className="block text-sm text-[var(--text-muted)] mb-1">Name</label>
                <input aria-label="Name" type="text" value={newPoolName} onChange={(e) => setNewPoolName(e.target.value)} placeholder="my-pool" className="w-full px-3 py-2 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm" />
              </div>
              <div>
                <label className="block text-sm text-[var(--text-muted)] mb-1">Type</label>
                <select aria-label="Pool type" value={newPoolType} onChange={(e) => setNewPoolType(e.target.value)} className="w-full px-3 py-2 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm">
                  <option value="dir">dir</option>
                  <option value="fs">fs</option>
                  <option value="netfs">netfs</option>
                  <option value="logical">logical</option>
                  <option value="disk">disk</option>
                  <option value="iscsi">iscsi</option>
                  <option value="zfs">zfs</option>
                </select>
              </div>
              <div>
                <label className="block text-sm text-[var(--text-muted)] mb-1">Target Path</label>
                <input aria-label="Target Path" type="text" value={newPoolPath} onChange={(e) => setNewPoolPath(e.target.value)} placeholder="/var/lib/libvirt/images" className="w-full px-3 py-2 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm" />
              </div>
          </div>
        </GlassModal>
      )}

      {xmlContent !== null && (
        <GlassModal open onClose={() => setXmlContent(null)} title={xmlName} wide>
          <pre className="text-sm text-[var(--text-secondary)] overflow-auto whitespace-pre-wrap font-mono">{xmlContent}</pre>
        </GlassModal>
      )}
    </PageLayout>
  )
}
