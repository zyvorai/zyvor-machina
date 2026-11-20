// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import ConfirmDialog from '../../components/ConfirmDialog'
import { Link } from 'react-router'
import { HardDrive, Loader2, Plus, RefreshCw } from 'lucide-react'
import ErrorBanner from '../../components/ErrorBanner'
import DetailTabs from '../../components/platform/DetailTabs'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton, platformStatSubtitle } from '../../components/platform/PlatformPageChrome'
import { usePlatformTabState } from '../../hooks/usePlatformTabState'
import { useFleetSettings } from '../../hooks/useFleetSettings'
import { StructuredErrorBanner } from '../../components/StructuredErrorBanner'
import { storageErrorPresentation } from '../../utils/storageErrorPresentation'
import { TahoeListEmpty, TahoeTableWrap, TahoeToolbar } from '../../components/platform/tahoe/TahoeListKit'
import StoragePoolWizard from '../../components/platform/StoragePoolWizard'
import {
  MacGlassPanel,
  MacListRow,
  MacSheet,
} from '../../components/platform/mac/PlatformMacUi'
import {
  bindStoragePoolTier,
  deleteStoragePool,
  discoverStoragePools,
  getStorageBackupSla,
  getStorageTiersOverview,
  listPlatformHosts,
  listStoragePools,
  patchStoragePool,
  syncAllHosts,
  upsertStorageBackupSla,
  activateStoragePool,
  deactivateStoragePool,
  refreshStoragePool,
  listLiveStoragePools,
  type FleetStorageOverview,
  type StorageBackupSla,
  type StoragePool,
  type StorageTierOverview,
  type LiveStoragePoolInfo,
} from '../../api/platform'
import {
  createStoragePoolVolume,
  deleteStoragePoolVolume,
  getStorageSnapshotPolicy as getPoolSnapshotPolicy,
  listStoragePoolVolumes,
  type StoragePoolVolume,
} from '../../api/platformStorage'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import {statusBadgeClasses, statusToneClass, hubLinkClasses} from '../../utils/semanticColors'

type TabId = 'disks' | 'pools' | 'tiers' | 'sla'

const STORAGE_TABS: Array<{ id: TabId; label: string }> = [
  { id: 'disks', label: 'Disks' },
  { id: 'pools', label: 'Pools' },
  { id: 'tiers', label: 'Tiers' },
  { id: 'sla', label: 'Backup SLA' },
]

function capacityRing(used: number, cap: number) {
  if (cap <= 0) return 0
  return Math.min(100, Math.round((used / cap) * 100))
}

export default function PlatformStorage() {
  const toast = useToastContext()
  const [tab, setTab] = usePlatformTabState<TabId>(STORAGE_TABS.map((t) => t.id), { defaultTab: 'disks' })

  const [rows, setRows] = useState<StoragePool[]>([])
  const { data: fleetStorage } = useFleetSettings('storage', tab === 'disks')
  const [tiers, setTiers] = useState<StorageTierOverview[]>([])
  const [slaPolicies, setSlaPolicies] = useState<StorageBackupSla[]>([])
  const [hostCount, setHostCount] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const [discovering, setDiscovering] = useState(false)
  const [poolWizardOpen, setPoolWizardOpen] = useState(false)
  const [bindDraft, setBindDraft] = useState<Record<string, string>>({})
  const [binding, setBinding] = useState<string | null>(null)
  const [slaEdit, setSlaEdit] = useState<StorageBackupSla | null>(null)
  const [slaRpo, setSlaRpo] = useState(24)
  const [slaRto, setSlaRto] = useState(4)
  const [slaRetention, setSlaRetention] = useState(30)
  const [slaSaving, setSlaSaving] = useState(false)
  const [snapshotPolicies, setSnapshotPolicies] = useState<Record<string, { pool_name: string; snapshot_retention_days: number; summary: string }>>({})
  const [snapshotPolicyLoading, setSnapshotPolicyLoading] = useState<string | null>(null)
  const [livePools, setLivePools] = useState<Record<string, LiveStoragePoolInfo>>({})
  const [poolActionId, setPoolActionId] = useState<string | null>(null)
  const [expandedPoolId, setExpandedPoolId] = useState<string | null>(null)
  const [poolVolumes, setPoolVolumes] = useState<Record<string, StoragePoolVolume[]>>({})
  const [volumesLoading, setVolumesLoading] = useState<string | null>(null)
  const [volumeCreatePool, setVolumeCreatePool] = useState<StoragePool | null>(null)
  const [volumeName, setVolumeName] = useState('')
  const [volumeCapacityGb, setVolumeCapacityGb] = useState(10)
  const [volumeFormat, setVolumeFormat] = useState('qcow2')
  const [volumeSaving, setVolumeSaving] = useState(false)
  const [confirmVolume, setConfirmVolume] = useState<{ poolId: string; volName: string; poolName: string } | null>(null)
  const [confirmPoolId, setConfirmPoolId] = useState<string | null>(null)
  const [resizePool, setResizePool] = useState<{ id: string; name: string; currentGib: number } | null>(null)
  const [resizePoolInput, setResizePoolInput] = useState('')
  const [search, setSearch] = useState('')

  const filteredPools = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return rows
    return rows.filter((p) => p.name.toLowerCase().includes(q) || p.backend.toLowerCase().includes(q))
  }, [rows, search])

  const tierName = (id?: string | null) => tiers.find((t) => t.id === id)?.name ?? null

  const loadSnapshotPolicy = async (poolId: string) => {
    setSnapshotPolicyLoading(poolId)
    try {
      const r = await getPoolSnapshotPolicy(poolId)
      setSnapshotPolicies((prev) => ({ ...prev, [poolId]: r }))
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSnapshotPolicyLoading(null)
    }
  }

  const load = useCallback(async (autoDiscover = false) => {
    setError(null)
    try {
      const [pools, hosts, tierOverview, sla] = await Promise.all([
        listStoragePools(),
        listPlatformHosts(),
        getStorageTiersOverview().catch(() => ({ tiers: [], summary: '' })),
        getStorageBackupSla().catch(() => ({ policies: [], summary: '' })),
      ])
      setTiers(tierOverview.tiers)
      setSlaPolicies(sla.policies ?? [])
      setHostCount(hosts.filter((h) => h.state === 'online').length)
      if (hosts.some((h) => h.state === 'online')) {
        listLiveStoragePools()
          .then((live) => {
            const map: Record<string, LiveStoragePoolInfo> = {}
            for (const p of live.pools ?? []) map[p.name] = p
            setLivePools(map)
          })
          .catch(() => setLivePools({}))
      }
      if (pools.length === 0 && autoDiscover && hosts.some((h) => h.state === 'online')) {
        setDiscovering(true)
        try {
          const r = await discoverStoragePools()
          const imported = r.pools ?? []
          setRows(imported)
          if (imported.length > 0) {
            toast.success(`Imported ${imported.length} storage pool(s) from libvirt`)
          }
        } catch (e: unknown) {
          setError(formatUserError(e))
        } finally {
          setDiscovering(false)
        }
      } else {
        setRows(pools)
      }
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }, [toast])

  useEffect(() => { void load(true) }, [load])

  const runDiscover = async () => {
    setDiscovering(true)
    setError(null)
    try {
      const r = await discoverStoragePools()
      const found = r.pools ?? []
      setRows(found)
      toast.success(found.length ? `Found ${found.length} pool(s)` : 'No libvirt pools on online hosts')
      await load(false)
    } catch (e: unknown) {
      const msg = formatUserError(e)
      setError(msg)
      toast.error(msg)
    } finally {
      setDiscovering(false)
    }
  }

  const bindTier = async (poolId: string, tierId: string) => {
    setBinding(poolId)
    try {
      await bindStoragePoolTier(poolId, tierId)
      toast.success('Pool assigned to tier')
      await load(false)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBinding(null)
    }
  }

  const openSlaEdit = (policy: StorageBackupSla) => {
    setSlaEdit(policy)
    setSlaRpo(policy.rpo_hours)
    setSlaRto(policy.rto_hours)
    setSlaRetention(policy.retention_days)
  }

  const saveSla = async () => {
    if (!slaEdit) return
    setSlaSaving(true)
    try {
      await upsertStorageBackupSla(slaEdit.pool_id, {
        rpo_hours: slaRpo,
        rto_hours: slaRto,
        retention_days: slaRetention,
      })
      toast.success('Backup SLA updated')
      setSlaEdit(null)
      await load(false)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSlaSaving(false)
    }
  }

  const loadPoolVolumes = async (poolId: string) => {
    setVolumesLoading(poolId)
    try {
      const r = await listStoragePoolVolumes(poolId)
      setPoolVolumes((prev) => ({ ...prev, [poolId]: r.volumes ?? [] }))
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setVolumesLoading(null)
    }
  }

  const togglePoolVolumes = (poolId: string) => {
    if (expandedPoolId === poolId) {
      setExpandedPoolId(null)
      return
    }
    setExpandedPoolId(poolId)
    if (!poolVolumes[poolId]) void loadPoolVolumes(poolId)
  }

  const saveVolume = async () => {
    if (!volumeCreatePool || !volumeName.trim()) return
    setVolumeSaving(true)
    try {
      await createStoragePoolVolume(volumeCreatePool.id, {
        name: volumeName.trim(),
        capacity_gb: Math.max(1, volumeCapacityGb),
        format: volumeFormat,
      })
      toast.success(`Created volume ${volumeName.trim()}`)
      setVolumeCreatePool(null)
      setVolumeName('')
      setVolumeCapacityGb(10)
      await loadPoolVolumes(volumeCreatePool.id)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setVolumeSaving(false)
    }
  }

  const removeVolume = async (poolId: string, volName: string) => {
    setVolumesLoading(poolId)
    try {
      await deleteStoragePoolVolume(poolId, volName)
      toast.success(`Deleted ${volName}`)
      await loadPoolVolumes(poolId)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setVolumesLoading(null)
    }
  }

  const totalCap = rows.reduce((s, p) => s + p.capacity_gib, 0)
  const totalUsed = rows.reduce((s, p) => s + p.used_gib, 0)

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load(false)}
      className="platform-readable"
      prepend={<PlatformBackLink to="/platform/infrastructure" label="Infrastructure" />}
      title="Storage"
      subtitle={
        <span className="flex flex-col gap-1">
          <span className="text-[var(--text-muted)]">Pools, tiers, backup SLA, and fleet disk health</span>
          {rows.length > 0
            ? platformStatSubtitle([
                { label: 'Pools', value: String(rows.length) },
                { label: 'Capacity', value: `${totalUsed.toFixed(0)} / ${totalCap.toFixed(0)} GiB` },
                { label: 'Online hosts', value: String(hostCount) },
              ])
            : null}
        </span>
      }
      icon={<HardDrive className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={
        <>
          {tab === 'pools' && (
            <>
              <button type="button" className="btn-secondary text-sm" disabled={discovering} onClick={() => void runDiscover()}>
                {discovering ? <Loader2 className="w-4 h-4 animate-spin" /> : <RefreshCw className="w-4 h-4" />}
                Import from hosts
              </button>
              <button type="button" className="btn-secondary text-sm" disabled={discovering} onClick={async () => {
                try { await syncAllHosts(); toast.success('Host sync queued') } catch (e: unknown) { toast.error(formatUserError(e)) }
              }}>Sync hosts</button>
              <button type="button" className="btn-primary flex items-center gap-2 text-sm" onClick={() => setPoolWizardOpen(true)}><Plus className="w-4 h-4" /> Add pool</button>
            </>
          )}
          <PlatformRefreshButton onClick={() => void load(false)} />
        </>
      }
      contentClassName="space-y-4"
    >
      <DetailTabs primary={STORAGE_TABS} active={tab} onChange={setTab} />

      {error && (storageErrorPresentation(error) ? (
        <StructuredErrorBanner error={storageErrorPresentation(error)!} />
      ) : (
        <ErrorBanner message={error} />
      ))}
      {error && storageErrorPresentation(error) && (
        <p className="text-xs text-[var(--text-muted)]">
          <Link to="/platform/hosts" className={hubLinkClasses()}>Hosts</Link>
          {' · '}
          <Link to="/node" className={hubLinkClasses()}>Classic node tools</Link>
        </p>
      )}

      {tab === 'disks' && (
        <div className="space-y-4">
          {fleetStorage && (
            <>
              <p className="text-sm text-[var(--text-muted)]">{fleetStorage.summary}</p>
              {(fleetStorage.pools?.length ?? 0) > 0 && (
                <>
                  <h2 className="text-sm font-semibold text-[var(--text-secondary)]">Pool health</h2>
                  <TahoeTableWrap>
                  <table className="apple-table w-full text-sm" aria-label="Pool health">
                    <thead>
                      <tr>
                        <th scope="col">Pool</th>
                        <th scope="col">Class</th>
                        <th scope="col">Used</th>
                        <th scope="col">Capacity</th>
                        <th scope="col">Used %</th>
                      </tr>
                    </thead>
                    <tbody className="nl-stagger">
                      {fleetStorage.pools.map((p) => (
                        <tr key={p.id}>
                          <td className="font-medium">{p.name}</td>
                          <td className="capitalize text-[var(--text-muted)]">{p.storage_class}{p.tier_name ? ` · ${p.tier_name}` : ''}</td>
                          <td>{p.used_gib} GiB</td>
                          <td>{p.capacity_gib || '—'} GiB</td>
                          <td>
                            <span className={p.status === 'critical' ? statusToneClass('error') : p.status === 'warn' ? statusToneClass('warn') : statusToneClass('info')}>
                              {Math.round(p.used_pct)}%
                            </span>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  </TahoeTableWrap>
                </>
              )}
            </>
          )}
          {!fleetStorage && (
            <div className="flex items-center gap-2 text-sm text-[var(--text-muted)] py-8">
              <Loader2 className="w-4 h-4 animate-spin" /> Loading fleet storage…
            </div>
          )}
          {fleetStorage && (
            <MacGlassPanel title="SMART status" subtitle="Failed disks reported by online hypervisors (linux-obs).">
              {fleetStorage.smart_hosts_sampled === 0 ? (
                <p className="text-sm text-amber-600/90">SMART data unavailable — no online host returned a disk sample.</p>
              ) : (fleetStorage.smart_disks ?? []).length === 0 ? (
                <p className="text-sm text-[var(--text-muted)]">No SMART failures detected on sampled hosts.</p>
              ) : (
                <div className="divide-y divide-white/[0.04] -mx-1">
                  {(fleetStorage.smart_disks ?? []).map((d) => (
                    <MacListRow
                      key={`${d.host_id}-${d.device}`}
                      title={`${d.hostname} · ${d.device}`}
                      subtitle={d.summary}
                      href={`/platform/hosts/${d.host_id}`}
                    />
                  ))}
                </div>
              )}
            </MacGlassPanel>
          )}
        </div>
      )}

      {tab === 'pools' && (
        <>
          <TahoeToolbar search={search} onSearchChange={setSearch} placeholder="Search pools…" />

          {rows.length === 0 && !error ? (
            <TahoeListEmpty
              icon={HardDrive}
              title="No storage pools"
              description="Import libvirt pools from your KVM hosts, or add one manually."
              primaryAction={{ label: 'Import from hosts', onClick: () => void runDiscover() }}
              secondaryAction={{ label: 'Add pool', onClick: () => setPoolWizardOpen(true) }}
            />
          ) : (
            <TahoeTableWrap>
              <table className="apple-table w-full text-sm min-w-[800px]" aria-label="Storage pools">
                <thead>
                  <tr>
                    <th scope="col">Pool</th>
                    <th scope="col">Class</th>
                    <th scope="col">Used</th>
                    <th scope="col">Capacity</th>
                    <th scope="col">Tier</th>
                    <th scope="col">State</th>
                    <th scope="col" className="text-right">Actions</th>
                  </tr>
                </thead>
                <tbody className="nl-stagger">
                  {filteredPools.map((p) => {
                    const live = livePools[p.name]
                    const active = live?.state === 'running'
                    const pct = capacityRing(p.used_gib, p.capacity_gib)
                    return (
                      <tr key={p.id} data-testid={`storage-pool-${p.name}`}>
                        <td className="font-medium">
                          {p.name}
                          {snapshotPolicies[p.id] && (
                            <p className="text-[10px] font-normal text-[var(--text-muted)]">{snapshotPolicies[p.id].summary}</p>
                          )}
                        </td>
                        <td className="capitalize text-[var(--text-muted)]">{p.storage_class} · {p.backend}</td>
                        <td>{p.used_gib} GiB</td>
                        <td>
                          <div className="min-w-[8rem]">
                            <p>{p.capacity_gib || '—'} GiB <span className="text-[var(--text-muted)]">({pct}%)</span></p>
                            <div className="mt-1.5 h-1.5 rounded-full bg-[var(--apple-fill-tertiary)] overflow-hidden" aria-hidden>
                              <div className={`h-full rounded-full transition-all duration-700 ${pct > 90 ? 'bg-red-500' : pct > 75 ? 'bg-amber-500' : 'bg-emerald-500'}`} style={{ width: `${Math.min(100, pct)}%` }} />
                            </div>
                          </div>
                        </td>
                        <td>
                          {tiers.length > 0 ? (
                            <div className="flex gap-1">
                              <select aria-label="Storage tier" className="input text-xs max-w-[6rem]" value={bindDraft[p.id] ?? p.tier_id ?? ''} onChange={(e) => setBindDraft((d) => ({ ...d, [p.id]: e.target.value }))}>
                                <option value="">Tier…</option>
                                {tiers.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
                              </select>
                              <button type="button" className="btn-secondary text-xs" disabled={binding === p.id || !(bindDraft[p.id] ?? p.tier_id)} onClick={() => {
                                const tid = bindDraft[p.id] ?? p.tier_id
                                if (tid) void bindTier(p.id, tid)
                              }}>Bind</button>
                            </div>
                          ) : (tierName(p.tier_id) ?? '—')}
                        </td>
                        <td>
                          {live && (
                            <span className={`px-1.5 py-0.5 rounded text-[10px] uppercase ${statusBadgeClasses(active ? 'ok' : 'warn')}`}>{live.state}</span>
                          )}
                        </td>
                        <td className="text-right">
                          <div className="flex flex-wrap justify-end gap-1">
                            {!active ? (
                              <button type="button" className="btn-primary text-xs" disabled={poolActionId === p.id || hostCount === 0} onClick={async () => {
                                setPoolActionId(p.id)
                                try { await activateStoragePool(p.id); toast.success(`Activated ${p.name}`); await load(false) }
                                catch (e: unknown) { toast.error(formatUserError(e)) }
                                finally { setPoolActionId(null) }
                              }}>Activate</button>
                            ) : (
                              <button type="button" className="btn-secondary text-xs" disabled={poolActionId === p.id} onClick={async () => {
                                setPoolActionId(p.id)
                                try { await deactivateStoragePool(p.id); toast.success(`Deactivated ${p.name}`); await load(false) }
                                catch (e: unknown) { toast.error(formatUserError(e)) }
                                finally { setPoolActionId(null) }
                              }}>Deactivate</button>
                            )}
                            <button type="button" className="btn-secondary text-xs" onClick={() => togglePoolVolumes(p.id)}>
                              {expandedPoolId === p.id ? 'Hide vols' : 'Volumes'}
                            </button>
                            <button type="button" className="btn-destructive text-xs" onClick={() => setConfirmPoolId(p.id)}>Remove</button>
                            <button
                              type="button"
                              className="btn-secondary text-xs"
                              disabled={snapshotPolicyLoading === p.id}
                              onClick={() => void loadSnapshotPolicy(p.id)}
                            >
                              {snapshotPolicyLoading === p.id ? 'Loading…' : 'Snapshot policy'}
                            </button>
                          </div>
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </TahoeTableWrap>
          )}
          {expandedPoolId && poolVolumes[expandedPoolId] && (
            <MacGlassPanel title="Pool volumes" subtitle={`${poolVolumes[expandedPoolId].length} volume(s)`}>
              <ul className="text-xs space-y-2">
                {poolVolumes[expandedPoolId].map((v) => (
                  <li key={v.name} className="flex justify-between gap-2">
                    <span>{v.name} · {v.capacity_gb} GiB</span>
                    <button type="button" className="btn-destructive text-xs" onClick={() => setConfirmVolume({ poolId: expandedPoolId, volName: v.name, poolName: rows.find((r) => r.id === expandedPoolId)?.name ?? '' })}>Delete</button>
                  </li>
                ))}
              </ul>
            </MacGlassPanel>
          )}
        </>
      )}

      {tab === 'tiers' && (
        <MacGlassPanel title="Storage tiers" subtitle="Gold / silver / bronze taxonomy with IOPS and replication stubs.">
          {tiers.length === 0 ? (
            <p className="text-sm text-[var(--text-muted)]">No tiers — run migration 028 to seed defaults.</p>
          ) : (
            <TahoeTableWrap>
            <table className="apple-table w-full text-sm" aria-label="Storage tiers">
                <thead>
                  <tr className="text-left text-[var(--text-muted)] border-b border-white/[0.06]">
                    <th scope="col" className="py-2 px-2">Name</th>
                    <th scope="col" className="py-2 px-2">Class</th>
                    <th scope="col" className="py-2 px-2">IOPS</th>
                    <th scope="col" className="py-2 px-2">Replication</th>
                    <th scope="col" className="py-2 px-2">Snapshots</th>
                    <th scope="col" className="py-2 px-2">RPO</th>
                    <th scope="col" className="py-2 px-2">Pools</th>
                  </tr>
                </thead>
                <tbody>
                  {tiers.map((t) => (
                    <tr key={t.id} className="border-b border-white/[0.04] text-[var(--text-primary)]">
                      <td className="py-2.5 px-2 font-medium">{t.name}</td>
                      <td className="py-2.5 px-2 capitalize">{t.tier_class}</td>
                      <td className="py-2.5 px-2">{t.iops_tier}</td>
                      <td className="py-2.5 px-2">{t.replication}</td>
                      <td className="py-2.5 px-2">{t.snapshot_retention_days}d</td>
                      <td className="py-2.5 px-2">{t.backup_rpo_hours}h</td>
                      <td className="py-2.5 px-2">{t.pool_count}</td>
                    </tr>
                  ))}
                </tbody>
            </table>
            </TahoeTableWrap>
          )}
        </MacGlassPanel>
      )}

      {tab === 'sla' && (
        <MacGlassPanel title="Backup SLA" subtitle="Per-pool RPO/RTO compliance grades (simulated).">
          {slaPolicies.length === 0 ? (
            <p className="text-sm text-[var(--text-muted)]">No SLA policies — import pools first.</p>
          ) : (
            <TahoeTableWrap>
            <table className="apple-table w-full text-sm" aria-label="Backup SLA policies">
                <thead>
                  <tr className="text-left text-[var(--text-muted)] border-b border-white/[0.06]">
                    <th scope="col" className="py-2 px-2">Pool</th>
                    <th scope="col" className="py-2 px-2">Tier</th>
                    <th scope="col" className="py-2 px-2">RPO</th>
                    <th scope="col" className="py-2 px-2">RTO</th>
                    <th scope="col" className="py-2 px-2">Retention</th>
                    <th scope="col" className="py-2 px-2">Grade</th>
                    <th scope="col" className="py-2 px-2" />
                  </tr>
                </thead>
                <tbody>
                  {slaPolicies.map((s) => (
                    <tr key={s.id} className="border-b border-white/[0.04] text-[var(--text-primary)]">
                      <td className="py-2.5 px-2">{s.pool_name}</td>
                      <td className="py-2.5 px-2">{s.tier_name ?? '—'}</td>
                      <td className="py-2.5 px-2">{s.rpo_hours}h</td>
                      <td className="py-2.5 px-2">{s.rto_hours}h</td>
                      <td className="py-2.5 px-2">{s.retention_days}d</td>
                      <td className="py-2.5 px-2">
                        <span className={`px-2 py-0.5 rounded text-xs font-semibold ${
                          statusBadgeClasses(s.compliance_grade === 'A' ? 'ok' : s.compliance_grade === 'B' ? 'info' : 'warn')
                        }`}>{s.compliance_grade}</span>
                      </td>
                      <td className="py-2.5 px-2 text-right">
                        <button type="button" className={`text-xs hover:underline ${hubLinkClasses()}`} onClick={() => openSlaEdit(s)}>
                          Edit
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
            </table>
            </TahoeTableWrap>
          )}
        </MacGlassPanel>
      )}

      <StoragePoolWizard
        open={poolWizardOpen}
        onClose={() => setPoolWizardOpen(false)}
        onCreated={async () => {
          toast.success('Pool added')
          await load(false)
        }}
      />

      <MacSheet open={!!volumeCreatePool} onClose={() => setVolumeCreatePool(null)} title={`New volume — ${volumeCreatePool?.name ?? ''}`}>
        <div className="space-y-4">
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">Name</span>
            <input className="input mt-1 w-full" value={volumeName} onChange={(e) => setVolumeName(e.target.value)} placeholder="data-01.qcow2" />
          </label>
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">Capacity (GiB)</span>
            <input type="number" min={1} className="input mt-1 w-full" value={volumeCapacityGb} onChange={(e) => setVolumeCapacityGb(Number(e.target.value))} />
          </label>
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">Format</span>
            <select className="input mt-1 w-full" value={volumeFormat} onChange={(e) => setVolumeFormat(e.target.value)}>
              <option value="qcow2">qcow2</option>
              <option value="raw">raw</option>
            </select>
          </label>
          <button type="button" className="btn-primary text-sm w-full" disabled={volumeSaving || !volumeName.trim()} onClick={() => void saveVolume()}>
            {volumeSaving ? 'Creating…' : 'Create volume'}
          </button>
        </div>
      </MacSheet>

      <MacSheet open={!!slaEdit} onClose={() => setSlaEdit(null)} title={`Edit backup SLA — ${slaEdit?.pool_name ?? ''}`}>
        <div className="space-y-4">
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">RPO (hours)</span>
            <input type="number" min={1} max={168} className="input mt-1 w-full" value={slaRpo} onChange={(e) => setSlaRpo(Number(e.target.value))} />
          </label>
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">RTO (hours)</span>
            <input type="number" min={1} max={72} className="input mt-1 w-full" value={slaRto} onChange={(e) => setSlaRto(Number(e.target.value))} />
          </label>
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">Retention (days)</span>
            <input type="number" min={1} max={365} className="input mt-1 w-full" value={slaRetention} onChange={(e) => setSlaRetention(Number(e.target.value))} />
          </label>
          <button type="button" className="btn-primary text-sm w-full" disabled={slaSaving} onClick={() => void saveSla()}>
            {slaSaving ? 'Saving…' : 'Save SLA'}
          </button>
        </div>
      </MacSheet>
      <ConfirmDialog
        open={confirmVolume !== null}
        title="Delete Volume"
        message={`Permanently delete volume "${confirmVolume?.volName}" from pool "${confirmVolume?.poolName}"? This cannot be undone.`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setConfirmVolume(null)}
        onConfirm={async () => {
          if (!confirmVolume) return
          try { await removeVolume(confirmVolume.poolId, confirmVolume.volName) }
          catch (e: unknown) { toast.error(formatUserError(e)) }
          finally { setConfirmVolume(null) }
        }}
      />
      <ConfirmDialog
        open={confirmPoolId !== null}
        title="Remove Storage Pool"
        message={`Remove pool "${rows.find((p) => p.id === confirmPoolId)?.name}" from the hypervisor and inventory? This cannot be undone.`}
        confirmLabel="Remove"
        variant="danger"
        onCancel={() => setConfirmPoolId(null)}
        onConfirm={async () => {
          try { await deleteStoragePool(confirmPoolId!); toast.success('Pool removed'); await load(false) }
          catch (e: unknown) { toast.error(formatUserError(e)) }
          finally { setConfirmPoolId(null) }
        }}
      />
      {resizePool && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" onClick={() => setResizePool(null)}>
          <div className="bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-2xl shadow-2xl w-full max-w-sm mx-4" onClick={(e) => e.stopPropagation()}>
            <div className="p-5 border-b border-[var(--apple-hairline)]">
              <span className="text-lg font-semibold">Edit pool capacity</span>
              <p className="text-sm text-[var(--text-muted)] mt-1">Pool: {resizePool.name}</p>
            </div>
            <div className="p-5 space-y-3">
              <label htmlFor="resize-pool-capacity" className="block text-sm text-[var(--text-muted)]">Capacity (GiB)</label>
              <input
                id="resize-pool-capacity"
                type="number"
                min={1}
                className="input-field w-full"
                value={resizePoolInput}
                onChange={(e) => setResizePoolInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    const n = Number(resizePoolInput)
                    if (!n || n < 1) return
                    const id = resizePool.id
                    setResizePool(null)
                    void patchStoragePool(id, { capacity_gib: n })
                      .then(() => { toast.success('Pool updated'); void load(false) })
                      .catch((err: unknown) => toast.error(formatUserError(err)))
                  } else if (e.key === 'Escape') {
                    setResizePool(null)
                  }
                }}
                autoFocus
              />
            </div>
            <div className="flex justify-end gap-3 px-5 pb-5">
              <button type="button" onClick={() => setResizePool(null)} className="btn-secondary text-sm font-medium transition">Cancel</button>
              <button
                type="button"
                disabled={!resizePoolInput || Number(resizePoolInput) < 1}
                className="btn-primary text-sm disabled:opacity-40 disabled:cursor-not-allowed"
                onClick={() => {
                  const n = Number(resizePoolInput)
                  if (!n || n < 1) return
                  const id = resizePool.id
                  setResizePool(null)
                  void patchStoragePool(id, { capacity_gib: n })
                    .then(() => { toast.success('Pool updated'); void load(false) })
                    .catch((err: unknown) => toast.error(formatUserError(err)))
                }}
              >
                Save
              </button>
            </div>
          </div>
        </div>
      )}
    </PlatformPageChrome>
  )
}
