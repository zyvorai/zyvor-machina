// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import ConfirmDialog from '../../components/ConfirmDialog'
import { Link } from 'react-router'
import { Cable, Layers, Loader2, Network, Plus, RefreshCw, Shield } from 'lucide-react'
import DetailTabs from '../../components/platform/DetailTabs'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton, platformStatSubtitle } from '../../components/platform/PlatformPageChrome'
import { usePlatformTabState } from '../../hooks/usePlatformTabState'
import PageSkeleton from '../../components/PageSkeleton'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import { TahoeListEmpty, TahoeTableWrap, TahoeToolbar } from '../../components/platform/tahoe/TahoeListKit'
import NetworkCreateWizard from '../../components/platform/NetworkCreateWizard'
import FleetSettingsPane from '../../components/platform/FleetSettingsPane'
import MachinaNetworkLens from '../../components/ai/MachinaNetworkLens'
import {
  MacGlassPanel,
  MacListRow,
  MacSheet,
} from '../../components/platform/mac/PlatformMacUi'
import {
  allocateIpam,
  bindNetworkToSegment,
  createNetworkSegment,
  deletePlatformNetwork,
  discoverPlatformNetworks,
  emergencyUnlockNetworkSegment,
  getFleetNetwork,
  getNetworkSegmentsOverview,
  listIpamPools,
  listPlatformHosts,
  listPlatformNetworks,
  listPlatformVms,
  patchPlatformNetwork,
  simulateSegmentConnectivity,
  syncAllHosts,
  exportNetworkSegmentsGitops,
  listLivePlatformNetworks,
  activatePlatformNetwork,
  deactivatePlatformNetwork,
  type FleetNetworkOverview,
  type IpamPoolRow,
  type NetworkSegmentOverview,
  type PlatformNetwork,
  type SegmentConnectivityResult,
  type LiveNetworkInfo,
} from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import {hostStateTone, httpStatusTone, migrationReadinessTone, riskTone, statusBadgeClasses, statusPillClasses, statusSurfaceClasses, statusToneClass, taskStatusTone, webhookDeliveryTone, hubLinkClasses} from '../../utils/semanticColors'

type TabId = 'networks' | 'segments' | 'ipam' | 'lens'

const NETWORK_TABS = [
  { id: 'networks' as const, label: 'Networks' },
  { id: 'segments' as const, label: 'Segments' },
  { id: 'ipam' as const, label: 'IPAM' },
  { id: 'lens' as const, label: 'Network Lens' },
]

export default function PlatformNetworks() {
  const toast = useToastContext()
  const [tab, setTab] = usePlatformTabState<TabId>(NETWORK_TABS.map((t) => t.id), { defaultTab: 'networks' })

  const [loading, setLoading] = useState(true)
  const [rows, setRows] = useState<PlatformNetwork[]>([])
  const [segments, setSegments] = useState<NetworkSegmentOverview[]>([])
  const [ipamPools, setIpamPools] = useState<IpamPoolRow[]>([])
  const [hostCount, setHostCount] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const [discovering, setDiscovering] = useState(false)
  const [networkWizardOpen, setNetworkWizardOpen] = useState(false)
  const [segmentSheetOpen, setSegmentSheetOpen] = useState(false)
  const [creating, setCreating] = useState(false)
  const [confirmDeleteNetwork, setConfirmDeleteNetwork] = useState<{ id: string; name: string } | null>(null)
  const [confirmUnlockSegment, setConfirmUnlockSegment] = useState<{ id: string; name: string } | null>(null)
  const [segName, setSegName] = useState('app-tier1')
  const [segTier, setSegTier] = useState('tier1')
  const [segCidr, setSegCidr] = useState('10.20.0.0/16')
  const [segEastWest, setSegEastWest] = useState('allow')
  const [segProfile, setSegProfile] = useState('ProductionServer')
  const [allocating, setAllocating] = useState<string | null>(null)
  const [ipamHostname, setIpamHostname] = useState('')
  const [bindDraft, setBindDraft] = useState<Record<string, string>>({})
  const [binding, setBinding] = useState<string | null>(null)
  const [connectivitySegment, setConnectivitySegment] = useState<NetworkSegmentOverview | null>(null)
  const [connectivity, setConnectivity] = useState<SegmentConnectivityResult | null>(null)
  const [connectivityLoading, setConnectivityLoading] = useState(false)
  const [fleetNetwork, setFleetNetwork] = useState<FleetNetworkOverview | null>(null)
  const [lensVmNames, setLensVmNames] = useState<string[]>([])
  const [editDraft, setEditDraft] = useState<Record<string, { bridge: string; vlan: string }>>({})
  const [savingId, setSavingId] = useState<string | null>(null)
  const [liveNetworks, setLiveNetworks] = useState<Record<string, LiveNetworkInfo>>({})
  const [networkActionId, setNetworkActionId] = useState<string | null>(null)
  const [search, setSearch] = useState('')

  const filteredRows = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return rows
    return rows.filter((n) =>
      n.name.toLowerCase().includes(q)
      || n.backend.toLowerCase().includes(q)
      || (n.bridge?.toLowerCase().includes(q) ?? false),
    )
  }, [rows, search])

  const segmentName = (id?: string | null) =>
    segments.find((s) => s.id === id)?.name ?? null

  const load = useCallback(async (autoDiscover = false) => {
    setError(null)
    setLoading(true)
    try {
      const [nets, hosts, overview, pools] = await Promise.all([
        listPlatformNetworks(),
        listPlatformHosts(),
        getNetworkSegmentsOverview().catch(() => ({ segments: [], summary: '' })),
        listIpamPools().catch(() => []),
      ])
      setSegments(overview.segments)
      setIpamPools(pools)
      setHostCount(hosts.filter((h) => h.state === 'online').length)
      if (hosts.some((h) => h.state === 'online')) {
        listLivePlatformNetworks()
          .then((live) => {
            const map: Record<string, LiveNetworkInfo> = {}
            for (const n of live.networks ?? []) map[n.name] = n
            setLiveNetworks(map)
          })
          .catch(() => setLiveNetworks({}))
      }
      if (nets.length === 0 && autoDiscover && hosts.some((h) => h.state === 'online')) {
        setDiscovering(true)
        try {
          const r = await discoverPlatformNetworks()
          setRows(r.networks)
          if (r.networks.length > 0) {
            toast.success(`Imported ${r.networks.length} network(s) from libvirt`)
          }
        } catch (e: unknown) {
          setError(formatUserError(e))
        } finally {
          setDiscovering(false)
        }
      } else {
        setRows(nets)
      }
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => { void load(true) }, [load])

  useEffect(() => {
    if (tab !== 'lens') return
    void Promise.all([
      getFleetNetwork().then(setFleetNetwork).catch(() => setFleetNetwork(null)),
      listPlatformVms().then((v) => setLensVmNames(v.map((x) => x.name))).catch(() => setLensVmNames([])),
    ])
  }, [tab])

  const runDiscover = async () => {
    setDiscovering(true)
    setError(null)
    try {
      const r = await discoverPlatformNetworks()
      setRows(r.networks)
      toast.success(r.networks.length ? `Found ${r.networks.length} network(s)` : 'No libvirt networks found on online hosts')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setDiscovering(false)
    }
  }

  const syncHosts = async () => {
    setDiscovering(true)
    try {
      await syncAllHosts()
      toast.success('Host sync queued — networks import with inventory')
      setTimeout(() => void load(false), 4000)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setDiscovering(false)
    }
  }

  const createSegment = async () => {
    setCreating(true)
    try {
      await createNetworkSegment({
        name: segName,
        tier: segTier,
        cidr: segCidr,
        east_west_default: segEastWest,
        firewall_profile: segProfile || undefined,
      })
      toast.success('Overlay segment created')
      setSegmentSheetOpen(false)
      await load(false)
      setTab('segments')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setCreating(false)
    }
  }

  const runAllocate = async (segmentId: string) => {
    setAllocating(segmentId)
    try {
      const r = await allocateIpam(segmentId, ipamHostname.trim() ? { hostname: ipamHostname.trim() } : undefined)
      toast.success(`Allocated ${r.ip_address}${r.hostname ? ` (${r.hostname})` : ''} on ${r.segment_name}`)
      setIpamHostname('')
      await load(false)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setAllocating(null)
    }
  }

  const bindNetwork = async (networkId: string, segmentId: string) => {
    setBinding(networkId)
    try {
      await bindNetworkToSegment(segmentId, networkId)
      toast.success('Network bound to segment')
      await load(false)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBinding(null)
    }
  }

  const networkDraft = (n: PlatformNetwork) =>
    editDraft[n.id] ?? { bridge: n.bridge ?? '', vlan: n.vlan_id != null ? String(n.vlan_id) : '' }

  const saveNetwork = async (n: PlatformNetwork) => {
    const draft = networkDraft(n)
    setSavingId(n.id)
    try {
      const vlan = draft.vlan.trim() ? Number.parseInt(draft.vlan, 10) : undefined
      const updated = await patchPlatformNetwork(n.id, {
        bridge: draft.bridge.trim() || undefined,
        vlan_id: vlan != null && !Number.isNaN(vlan) ? vlan : undefined,
      })
      toast.success(`Updated ${n.name}`)
      setRows((prev) => prev.map((row) => (row.id === n.id ? updated : row)))
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSavingId(null)
    }
  }

  const runSegmentConnectivity = async (segment: NetworkSegmentOverview) => {
    setConnectivitySegment(segment)
    setConnectivity(null)
    setConnectivityLoading(true)
    try {
      setConnectivity(await simulateSegmentConnectivity(segment.id))
    } catch (e: unknown) {
      toast.error(formatUserError(e))
      setConnectivitySegment(null)
    } finally {
      setConnectivityLoading(false)
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load(false)}
      prepend={<PlatformBackLink to="/platform/infrastructure" label="Infrastructure" />}
      title="Networks"
      subtitle={
        <span className="flex flex-col gap-1">
          <span className="text-[var(--text-muted)]">Overlays, libvirt bridges, IPAM — NSX-class segments and micro-segmentation.</span>
          {platformStatSubtitle([
            { label: 'Networks', value: rows.length },
            { label: 'Online hosts', value: hostCount },
            { label: 'Segments', value: segments.length },
          ])}
        </span>
      }
      icon={<Network className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={
        <>
          {tab === 'networks' && (
            <>
              <button type="button" className="btn-secondary text-sm flex items-center gap-1.5" disabled={discovering} onClick={() => void runDiscover()}>
                {discovering ? <Loader2 className="w-4 h-4 animate-spin" /> : <RefreshCw className="w-4 h-4" />}
                Import from hosts
              </button>
              <button type="button" className="btn-primary text-sm flex items-center gap-1.5" onClick={() => setNetworkWizardOpen(true)}>
                <Plus className="w-4 h-4" /> New network
              </button>
            </>
          )}
          {tab === 'segments' && (
            <button type="button" className="btn-primary text-sm flex items-center gap-1.5" onClick={() => setSegmentSheetOpen(true)}>
              <Plus className="w-4 h-4" /> New segment
            </button>
          )}
          <PlatformRefreshButton onClick={() => void load(false)} />
        </>
      }
      contentClassName="space-y-4"
    >
      <DetailTabs primary={NETWORK_TABS} active={tab} onChange={setTab} />

      {loading && rows.length === 0 && !discovering && !error && <PageSkeleton />}

      {tab === 'networks' && (
        <>
          <TahoeToolbar search={search} onSearchChange={setSearch} placeholder="Search networks…" />

          {rows.length === 0 && !discovering && !error && !loading && (
            <TahoeListEmpty
              icon={Network}
              title="No networks yet"
              description="Import libvirt networks from online hosts or create a bridge-backed network for VMs."
              primaryAction={{ label: 'Import from hosts', onClick: () => void runDiscover() }}
              secondaryAction={{ label: 'Create network', onClick: () => setNetworkWizardOpen(true) }}
            />
          )}

          {discovering && rows.length === 0 && (
            <div className="flex items-center justify-center gap-2 text-sm text-[var(--text-muted)] py-12">
              <Loader2 className="w-5 h-5 animate-spin" /> Discovering libvirt networks…
            </div>
          )}

          {rows.length > 0 && (
            <TahoeTableWrap>
              <table className="apple-table w-full text-sm min-w-[720px]" aria-label="Platform networks">
                <thead>
                  <tr>
                    <th scope="col">Name</th>
                    <th scope="col">Backend</th>
                    <th scope="col">Bridge</th>
                    <th scope="col">VLAN</th>
                    <th scope="col">Segment</th>
                    <th scope="col">Status</th>
                    <th scope="col" className="text-right">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {filteredRows.map((n) => {
                    const live = liveNetworks[n.name]
                    const active = live?.active ?? false
                    return (
                      <tr key={n.id} data-testid={`platform-network-${n.name}`}>
                        <td className="font-medium">{n.name}</td>
                        <td className="capitalize text-[var(--text-muted)]">{n.backend.replace('-', ' ')}</td>
                        <td>
                          <input
                            aria-label={`Bridge for ${n.name}`}
                            className="input w-full font-mono text-xs max-w-[8rem]"
                            value={networkDraft(n).bridge}
                            onChange={(e) => setEditDraft((d) => ({ ...d, [n.id]: { ...networkDraft(n), bridge: e.target.value } }))}
                            placeholder="virbr0"
                          />
                        </td>
                        <td>
                          <input
                            aria-label={`VLAN for ${n.name}`}
                            className="input w-full text-xs max-w-[4rem]"
                            inputMode="numeric"
                            value={networkDraft(n).vlan}
                            onChange={(e) => setEditDraft((d) => ({ ...d, [n.id]: { ...networkDraft(n), vlan: e.target.value } }))}
                            placeholder="—"
                          />
                        </td>
                        <td className="text-xs">{segmentName(n.segment_id) ?? 'Unbound'}</td>
                        <td>
                          {live && (
                            <span className={`px-1.5 py-0.5 rounded text-[10px] uppercase tracking-wide ${statusBadgeClasses(active ? 'ok' : 'warn')}`}>
                              {active ? 'active' : 'inactive'}
                            </span>
                          )}
                        </td>
                        <td className="text-right">
                          <div className="flex flex-wrap justify-end gap-1">
                            <button type="button" data-testid={`network-save-${n.id}`} className="btn-secondary text-xs" disabled={savingId === n.id} onClick={() => void saveNetwork(n)}>
                              {savingId === n.id ? <Loader2 className="w-3 h-3 animate-spin inline" /> : 'Save'}
                            </button>
                            {!active ? (
                              <button type="button" className="btn-primary text-xs" disabled={networkActionId === n.id || hostCount === 0} onClick={async () => {
                                setNetworkActionId(n.id)
                                try { await activatePlatformNetwork(n.id); toast.success(`Activated ${n.name}`); await load(false) }
                                catch (e: unknown) { toast.error(formatUserError(e)) }
                                finally { setNetworkActionId(null) }
                              }}>Activate</button>
                            ) : (
                              <button type="button" className="btn-secondary text-xs" disabled={networkActionId === n.id} onClick={async () => {
                                setNetworkActionId(n.id)
                                try { await deactivatePlatformNetwork(n.id); toast.success(`Deactivated ${n.name}`); await load(false) }
                                catch (e: unknown) { toast.error(formatUserError(e)) }
                                finally { setNetworkActionId(null) }
                              }}>Deactivate</button>
                            )}
                            {segments.length > 0 && (
                              <>
                                <select aria-label="Segment" className="input text-xs max-w-[6rem]" value={bindDraft[n.id] ?? n.segment_id ?? ''} onChange={(e) => setBindDraft((d) => ({ ...d, [n.id]: e.target.value }))}>
                                  <option value="">Segment…</option>
                                  {segments.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
                                </select>
                                <button type="button" className="btn-secondary text-xs" disabled={binding === n.id || !(bindDraft[n.id] ?? n.segment_id)} onClick={() => {
                                  const seg = bindDraft[n.id] ?? n.segment_id
                                  if (seg) void bindNetwork(n.id, seg)
                                }}>Bind</button>
                              </>
                            )}
                            <button type="button" className="btn-destructive text-xs min-h-9" onClick={() => setConfirmDeleteNetwork({ id: n.id, name: n.name })}>Remove</button>
                          </div>
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </TahoeTableWrap>
          )}
          {rows.length > 0 && filteredRows.length === 0 && (
            <p className="text-sm text-center text-[var(--text-muted)] py-6">No networks match your search.</p>
          )}
        </>
      )}

      {tab === 'segments' && (
        <>
          <MacGlassPanel title="Overlay segments" subtitle="Tier-0/Tier-1 taxonomy with east-west defaults and micro-seg grade.">
            <div className="flex justify-end mb-3">
              <button
                type="button"
                className="tahoe-btn-ghost text-xs"
                onClick={async () => {
                  try {
                    const data = await exportNetworkSegmentsGitops()
                    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
                    const url = URL.createObjectURL(blob)
                    const a = document.createElement('a')
                    a.href = url
                    a.download = 'network-segments-gitops.json'
                    a.click()
                    URL.revokeObjectURL(url)
                    toast.success('GitOps export downloaded')
                  } catch (e: unknown) {
                    toast.error(formatUserError(e))
                  }
                }}
              >
                Export GitOps
              </button>
            </div>
            {segments.length === 0 ? (
              <PlatformEmptyState
                icon={Layers}
                title="No network segments"
                subtitle="Create a segment for east-west policy, or seed prod-tier1 / dmz-tier0 after migration."
                action={(
                  <button type="button" className="btn-primary text-sm" onClick={() => setSegmentSheetOpen(true)}>Create segment</button>
                )}
              />
            ) : (
              <TahoeTableWrap>
              <table className="apple-table w-full text-sm" aria-label="Network segments">
                  <thead>
                    <tr className="text-left text-[var(--text-muted)] border-b border-white/[0.06]">
                      <th scope="col" className="py-2 px-2">Name</th>
                      <th scope="col" className="py-2 px-2">Tier</th>
                      <th scope="col" className="py-2 px-2">CIDR</th>
                      <th scope="col" className="py-2 px-2">East-west</th>
                      <th scope="col" className="py-2 px-2">Grade</th>
                      <th scope="col" className="py-2 px-2">VMs</th>
                      <th scope="col" className="py-2 px-2">Profile</th>
                      <th scope="col" className="py-2 px-2">Networks</th>
                      <th scope="col" className="py-2 px-2" />
                    </tr>
                  </thead>
                  <tbody>
                    {segments.map((s) => (
                      <tr key={s.id} className="border-b border-white/[0.04] text-[var(--text-primary)]">
                        <td className="py-2.5 px-2 font-medium">{s.name}</td>
                        <td className="py-2.5 px-2 font-mono text-xs">{s.tier}</td>
                        <td className="py-2.5 px-2 font-mono text-xs">{s.cidr}</td>
                        <td className="py-2.5 px-2">{s.east_west_default}</td>
                        <td className="py-2.5 px-2">
                          <span className={`px-2 py-0.5 rounded text-xs font-semibold ${
                            statusBadgeClasses(
                              s.micro_seg_grade === 'A' ? 'ok' : s.micro_seg_grade === 'B' ? 'info' : 'warn',
                            )
                          }`}>
                            {s.micro_seg_grade} ({s.micro_seg_score})
                          </span>
                        </td>
                        <td className="py-2.5 px-2">{s.vm_count}</td>
                        <td className="py-2.5 px-2 text-xs">
                          {s.firewall_profile ? (
                            <Link to="/platform/zeus/security/firewall" className="text-orange-600 hover:underline">{s.firewall_profile}</Link>
                          ) : '—'}
                        </td>
                        <td className="py-2.5 px-2">{s.network_count}</td>
                        <td className="py-2.5 px-2">
                          <div className="flex flex-wrap gap-1">
                            <button
                              type="button"
                              className="btn-secondary text-xs flex items-center gap-1"
                              onClick={() => void runSegmentConnectivity(s)}
                            >
                              <Cable className="w-3 h-3" /> Matrix
                            </button>
                            <button
                              type="button"
                              className="btn-destructive text-xs"
                              onClick={() => setConfirmUnlockSegment({ id: s.id, name: s.name })}
                            >
                              Unlock
                            </button>
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
              </table>
              </TahoeTableWrap>
            )}
          </MacGlassPanel>
        </>
      )}

      {tab === 'ipam' && (
        <MacGlassPanel title="IPAM pools" subtitle="Next-free allocation from segment CIDR pools.">
          <label className="block text-sm mb-4 max-w-md">
            <span className="text-[var(--text-muted)]">Default hostname (optional)</span>
            <input
              className="input w-full mt-1.5 font-mono text-sm"
              value={ipamHostname}
              onChange={(e) => setIpamHostname(e.target.value)}
              placeholder="app-01.prod"
            />
          </label>
          {ipamPools.length === 0 ? (
            <PlatformEmptyState
              title="No IPAM pools"
              subtitle="Create a network segment with IPAM enabled to allocate addresses from a CIDR."
              action={<button type="button" className="btn-secondary text-sm" onClick={() => setTab('segments')}>Open segments</button>}
            />
          ) : (
            <TahoeTableWrap>
            <table className="apple-table w-full text-sm" aria-label="IPAM pools">
                <thead>
                  <tr className="text-left text-[var(--text-muted)] border-b border-white/[0.06]">
                    <th scope="col" className="py-2 px-2">Segment</th>
                    <th scope="col" className="py-2 px-2">CIDR</th>
                    <th scope="col" className="py-2 px-2">Gateway</th>
                    <th scope="col" className="py-2 px-2">Next offset</th>
                    <th scope="col" className="py-2 px-2">Reservations</th>
                    <th scope="col" className="py-2 px-2" />
                  </tr>
                </thead>
                <tbody>
                  {ipamPools.map((p) => (
                    <tr key={p.id} className="border-b border-white/[0.04] text-[var(--text-primary)]">
                      <td className="py-2.5 px-2">{p.segment_name}</td>
                      <td className="py-2.5 px-2 font-mono text-xs">{p.cidr}</td>
                      <td className="py-2.5 px-2 font-mono text-xs">{p.gateway || '—'}</td>
                      <td className="py-2.5 px-2">{p.next_offset}</td>
                      <td className="py-2.5 px-2">{p.reservation_count}</td>
                      <td className="py-2.5 px-2">
                        <button
                          type="button"
                          className="btn-secondary text-xs"
                          disabled={allocating === p.segment_id}
                          onClick={() => void runAllocate(p.segment_id)}
                        >
                          {allocating === p.segment_id ? <Loader2 className="w-3 h-3 animate-spin inline" /> : 'Allocate'}
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

      {tab === 'lens' && (
        <div className="space-y-4 xl:space-y-0 xl:columns-2 xl:gap-4 [&>*]:break-inside-avoid xl:[&>*]:mb-4">
          {fleetNetwork && (
            <p className="text-sm text-[var(--text-muted)]">{fleetNetwork.summary}</p>
          )}
          <MachinaNetworkLens vmNames={lensVmNames} />
          <MacGlassPanel title="System Settings" subtitle="Fleet network pane — segments, firewall SLA, host systemd">
            <Link to="/platform/settings?section=network" className={`text-sm ${hubLinkClasses()}`}>
              Open Network in System Settings →
            </Link>
          </MacGlassPanel>
        </div>
      )}

      <NetworkCreateWizard
        open={networkWizardOpen}
        onClose={() => setNetworkWizardOpen(false)}
        suggestDiscover={rows.length === 0}
        onDiscover={runDiscover}
        onCreated={async () => {
          toast.success('Network added and provision task queued')
          await load(false)
        }}
      />

      <MacSheet open={segmentSheetOpen} onClose={() => setSegmentSheetOpen(false)} title="New overlay segment" subtitle="Tier-0 uplink / Tier-1 workload segment with optional Zyra profile." wide>
        <div className="space-y-4 xl:space-y-0 xl:columns-2 xl:gap-4 [&>*]:break-inside-avoid xl:[&>*]:mb-4">
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">Name</span>
            <input className="input w-full mt-1.5" value={segName} onChange={(e) => setSegName(e.target.value)} />
          </label>
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">Tier</span>
            <select className="input w-full mt-1.5" value={segTier} onChange={(e) => setSegTier(e.target.value)}>
              <option value="tier1">tier1 — workload</option>
              <option value="tier0">tier0 — uplink / DMZ</option>
            </select>
          </label>
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">CIDR</span>
            <input className="input w-full mt-1.5 font-mono" value={segCidr} onChange={(e) => setSegCidr(e.target.value)} />
          </label>
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">East-west default</span>
            <select className="input w-full mt-1.5" value={segEastWest} onChange={(e) => setSegEastWest(e.target.value)}>
              <option value="allow">allow</option>
              <option value="deny">deny (micro-seg)</option>
            </select>
          </label>
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">Zeus firewall profile</span>
            <input className="input w-full mt-1.5" value={segProfile} onChange={(e) => setSegProfile(e.target.value)} placeholder="ProductionServer" />
          </label>
          <div className="flex gap-2 pt-2">
            <button type="button" className="btn-secondary text-sm flex-1" onClick={() => setSegmentSheetOpen(false)}>Cancel</button>
            <button type="button" className="btn-primary text-sm flex-1" disabled={creating} onClick={() => void createSegment()}>
              Create segment
            </button>
          </div>
        </div>
      </MacSheet>

      <MacSheet
        open={!!connectivitySegment}
        onClose={() => { setConnectivitySegment(null); setConnectivity(null) }}
        title={connectivitySegment ? `Connectivity — ${connectivitySegment.name}` : 'Connectivity'}
        subtitle="East-west micro-segmentation simulation for this overlay"
        wide
      >
        {connectivityLoading && (
          <p className="text-sm text-[var(--text-muted)] flex items-center gap-2"><Loader2 className="w-4 h-4 animate-spin" /> Simulating…</p>
        )}
        {connectivity && (
          <div className="space-y-4 xl:space-y-0 xl:columns-2 xl:gap-4 [&>*]:break-inside-avoid xl:[&>*]:mb-4">
            <p className="text-sm text-[var(--text-muted)]">{connectivity.matrix.summary}</p>
            {connectivity.matrix.warnings.length > 0 && (
              <div className={`rounded-xl p-3 text-sm space-y-1 ${statusSurfaceClasses('warn')}`}>
                {connectivity.matrix.warnings.map((w) => (
                  <p key={w}>{w}</p>
                ))}
              </div>
            )}
            <div className="grid md:grid-cols-2 gap-4">
              <div>
                <p className={`text-xs font-semibold uppercase mb-2 ${statusToneClass('ok')}`}>Allowed</p>
                <div className="rounded-xl border border-white/[0.06] overflow-hidden">
                  {connectivity.matrix.allows.map((c, i) => (
                    <MacListRow key={`a-${i}`} title={`${c.source} → ${c.destination}:${c.port}`} subtitle={c.reason} />
                  ))}
                  {connectivity.matrix.allows.length === 0 && (
                    <p className="px-4 py-3 text-sm text-[var(--text-muted)]">No allowed paths</p>
                  )}
                </div>
              </div>
              <div>
                <p className={`text-xs font-semibold uppercase mb-2 ${statusToneClass('error')}`}>Blocked</p>
                <div className="rounded-xl border border-white/[0.06] overflow-hidden">
                  {connectivity.matrix.blocks.map((c, i) => (
                    <MacListRow key={`b-${i}`} title={`${c.source} → ${c.destination}:${c.port}`} subtitle={c.reason} />
                  ))}
                  {connectivity.matrix.blocks.length === 0 && (
                    <p className="px-4 py-3 text-sm text-[var(--text-muted)]">No blocked paths</p>
                  )}
                </div>
              </div>
            </div>
          </div>
        )}
      </MacSheet>
      {tab === 'networks' && <FleetSettingsPane kind="network" />}
      <ConfirmDialog
        open={confirmDeleteNetwork !== null}
        title="Remove Network"
        message={`Undefine network "${confirmDeleteNetwork?.name}" on the hypervisor and remove from inventory? VMs on this network will lose connectivity.`}
        confirmLabel="Remove"
        variant="danger"
        onCancel={() => setConfirmDeleteNetwork(null)}
        onConfirm={async () => {
          try {
            await deletePlatformNetwork(confirmDeleteNetwork!.id)
            toast.success('Network removed')
            await load(false)
          } catch (e: unknown) { toast.error(formatUserError(e)) }
          finally { setConfirmDeleteNetwork(null) }
        }}
      />
      <ConfirmDialog
        open={confirmUnlockSegment !== null}
        title="Emergency Unlock Segment"
        message={`Emergency unlock segment "${confirmUnlockSegment?.name}"? This bypasses micro-segmentation and allows unrestricted traffic.`}
        confirmLabel="Unlock"
        variant="danger"
        onCancel={() => setConfirmUnlockSegment(null)}
        onConfirm={() => {
          const seg = confirmUnlockSegment
          setConfirmUnlockSegment(null)
          if (seg) void emergencyUnlockNetworkSegment(seg.id).then((r) => toast.success(r.summary)).catch((e: unknown) => toast.error(formatUserError(e)))
        }}
      />
    </PlatformPageChrome>
  )
}
