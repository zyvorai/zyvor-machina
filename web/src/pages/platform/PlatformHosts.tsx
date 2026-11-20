// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { Cloud, LayoutGrid, List, Plus, RefreshCw, Server } from 'lucide-react'
import PageLayout from '../../components/PageLayout'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import HostEnrollWizard from '../../components/platform/HostEnrollWizard'
import FleetCloudImmersive from '../../components/platform/FleetCloudImmersive'
import FleetStrip from '../../components/platform/hosts/FleetStrip'
import AttentionRail from '../../components/platform/hosts/AttentionRail'
import HostDrawer from '../../components/platform/hosts/HostDrawer'
import UnderlineTabs from '../../components/kit/UnderlineTabs'
import {
  enqueueValidateHost,
  getFleetLinuxHealth,
  listPlatformHosts,
  syncAllHosts,
  type FleetLinuxHostItem,
  type PlatformHost,
} from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { statusPillClasses, hubLinkClasses } from '../../utils/semanticColors'
import { fleetAttention, fleetFacts, formatAge, heartbeatAgeSecs } from '../../utils/hostAttention'
import HostFleetCard, { memoryPercent } from '../../components/platform/fleet/HostFleetCard'
import { TahoeTableWrap } from '../../components/platform/tahoe/TahoeListKit'
import { useExpandable } from '../../hooks/useExpandable'
import { ExpandableToggle } from '../../components/ui/ExpandableToggle'

type Display = 'cloud' | 'cards' | 'list'
const DISPLAY_KEY = 'machina.hosts.display'

function storedDisplay(): Display | null {
  try {
    const v = localStorage.getItem(DISPLAY_KEY)
    return v === 'cloud' || v === 'cards' || v === 'list' ? v : null
  } catch { return null }
}

export default function PlatformHosts() {
  const toast = useToastContext()
  const [searchParams] = useSearchParams()
  const filterOffline = searchParams.get('filter') === 'offline'
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [linuxByHost, setLinuxByHost] = useState<Record<string, FleetLinuxHostItem>>({})
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [search, setSearch] = useState('')
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [enrollWizardOpen, setEnrollWizardOpen] = useState(false)
  const [pref, setPref] = useState<Display | null>(storedDisplay)

  const load = useCallback(async () => {
    setError(null)
    try {
      const [rows, linux] = await Promise.all([
        listPlatformHosts(),
        getFleetLinuxHealth().catch(() => null),
      ])
      setHosts(Array.isArray(rows) ? rows : [])
      const map: Record<string, FleetLinuxHostItem> = {}
      for (const h of linux?.hosts ?? []) map[h.host_id] = h
      setLinuxByHost(map)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  // With two or more machines the map is the point; with one, cards read better.
  const display: Display = pref ?? (hosts.length >= 2 ? 'cloud' : 'cards')
  const chooseDisplay = (d: Display) => {
    setPref(d)
    try { localStorage.setItem(DISPLAY_KEY, d) } catch { /* ignore */ }
  }

  /** Runs one action with a toast and a reload; true when it worked. */
  const run = useCallback(async (label: string, fn: () => Promise<unknown>): Promise<boolean> => {
    setBusy(true)
    try {
      await fn()
      toast.success(label)
      await load()
      return true
    } catch (e: unknown) {
      toast.error(formatUserError(e))
      return false
    } finally {
      setBusy(false)
    }
  }, [load, toast])

  const visibleHosts = useMemo(() => {
    let rows = filterOffline ? hosts.filter((h) => h.state === 'offline') : hosts
    const q = search.trim().toLowerCase()
    if (q) rows = rows.filter((h) => h.hostname.toLowerCase().includes(q) || h.address?.toLowerCase().includes(q))
    return rows
  }, [hosts, filterOffline, search])
  const hostList = useExpandable(visibleHosts, 30)

  const now = Date.now()
  const facts = useMemo(() => fleetFacts(hosts, now), [hosts, now])
  const attention = useMemo(() => fleetAttention(hosts, now).filter((a) => a.severity !== 'info'), [hosts, now])
  const attentionIds = useMemo(() => new Set(attention.map((a) => a.hostId)), [attention])
  const selected = hosts.find((h) => h.id === selectedId) ?? null

  const addButton = (
    <button type="button" className="btn-primary text-sm inline-flex items-center gap-1" onClick={() => setEnrollWizardOpen(true)} data-testid="add-machine">
      <Plus className="w-4 h-4" /> Add machine
    </button>
  )

  const listView = (
    <TahoeTableWrap>
      <table className="apple-table w-full text-sm" aria-label="Managed hosts" data-testid="hosts-list">
        <thead>
          <tr>
            <th scope="col">Host</th>
            <th scope="col">Address</th>
            <th scope="col" className="text-center">State</th>
            <th scope="col" className="text-center">VMs</th>
            <th scope="col" className="text-center">CPU</th>
            <th scope="col" className="text-center">Memory</th>
            <th scope="col">Heartbeat</th>
            <th scope="col" className="text-center">Validation</th>
          </tr>
        </thead>
        <tbody>
          {hostList.shown.map((h) => (
            <tr key={h.id} className={`cursor-pointer ${selectedId === h.id ? 'bg-[var(--accent)]/10' : ''}`} onClick={() => setSelectedId(h.id)}>
              <td><Link to={`/platform/hosts/${h.id}`} className={`hover:underline ${hubLinkClasses()}`} onClick={(e) => e.stopPropagation()}>{h.hostname}</Link></td>
              <td className="font-mono text-xs">{h.address || '—'}</td>
              <td className="capitalize text-center">{h.maintenance_mode ? 'maintenance' : h.state}</td>
              <td className="text-center">{h.vm_count}</td>
              <td className="text-center">{h.state === 'online' ? `${Math.round(h.cpu_percent ?? 0)}%` : '—'}</td>
              <td className="text-center">{h.state === 'online' && memoryPercent(h) != null ? `${memoryPercent(h)}%` : '—'}</td>
              <td className="text-xs">{formatAge(heartbeatAgeSecs(h.last_heartbeat_at, now))}</td>
              <td className="text-center">
                <span className={`text-[10px] uppercase px-2 py-0.5 rounded border ${statusPillClasses(h.validation_status === 'passed' ? 'ok' : h.validation_status === 'failed' ? 'error' : 'neutral')}`}>{h.validation_status || 'pending'}</span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </TahoeTableWrap>
  )

  const body = (
    <>
      {display === 'cloud' && (
        <section aria-label="Machines in this cloud" data-testid="hosts-cloud-map" className="w-full">
          <FleetCloudImmersive hosts={visibleHosts} selectedId={selectedId} onSelect={setSelectedId} attentionIds={attentionIds} showDetail={false} />
        </section>
      )}
      {display === 'cards' && (
        <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3 w-full nl-stagger" data-testid="host-fleet-panels">
          {hostList.shown.map((h) => (
            <HostFleetCard key={h.id} host={h} linux={linuxByHost[h.id]} selected={selectedId === h.id} onSelect={() => setSelectedId(h.id)} now={now} />
          ))}
        </div>
      )}
      {display === 'list' && listView}
      {display !== 'cloud' && hostList.showToggle && (
        <ExpandableToggle expanded={hostList.expanded} hidden={hostList.hidden} listId={hostList.listId} onToggle={hostList.toggle} noun="hosts" />
      )}
    </>
  )

  return (
    <PageLayout
      compact
      error={error}
      loading={loading && hosts.length === 0}
      title={filterOffline ? 'Offline hosts' : 'Hosts'}
      subtitle={<span aria-live="polite" className="text-sm text-[var(--text-muted)]">{filterOffline ? 'Showing offline only' : 'Every machine in this cloud, and what needs you'}</span>}
      icon={<Server className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={
        <>
          <button type="button" className="btn-secondary text-sm" onClick={() => void run('Sync all queued', () => syncAllHosts())}>Sync all</button>
          <button type="button" aria-label="Refresh" onClick={() => void load()} className="btn-secondary text-xs"><RefreshCw className="w-4 h-4" /></button>
          {!filterOffline && addButton}
        </>
      }
      contentClassName="space-y-4"
    >
      {hosts.length === 0 && !loading && !error ? (
        <PlatformEmptyState icon={Server} title="No hosts enrolled" subtitle="Add a hypervisor to start managing VMs.">
          {addButton}
        </PlatformEmptyState>
      ) : (
        <>
          <FleetStrip facts={facts} />
          <AttentionRail items={attention} busy={busy} onSelect={setSelectedId} onRecheck={(id) => void run('Validation queued', () => enqueueValidateHost(id))} />

          {hosts.length === 1 && !filterOffline && (
            <section data-testid="first-run-card" className="rounded-2xl border border-dashed border-[var(--accent)]/50 bg-[var(--accent)]/5 p-5 flex flex-wrap items-center justify-between gap-3">
              <div>
                <h2 className="m-0 text-base font-semibold text-[var(--text-primary)]">Add your second machine</h2>
                <p className="m-0 mt-1 text-sm text-[var(--text-secondary)]">One command on the new machine; you watch it join, live. VMs can then move between machines.</p>
              </div>
              {addButton}
            </section>
          )}

          <div className="flex flex-wrap items-center gap-3">
            <div className="min-w-0 flex-1">
              <UnderlineTabs<Display>
                label="Hosts display"
                value={display}
                onChange={chooseDisplay}
                tabs={[
                  { id: 'cloud', label: 'Cloud map', icon: <Cloud className="w-4 h-4" /> },
                  { id: 'cards', label: 'Cards', icon: <LayoutGrid className="w-4 h-4" /> },
                  { id: 'list', label: 'List', icon: <List className="w-4 h-4" /> },
                ]}
              />
            </div>
            <input type="search" aria-label="Filter hosts" placeholder="Filter hosts…" value={search} onChange={(e) => setSearch(e.target.value)} className="input-field text-sm w-48" data-testid="hosts-filter" />
          </div>

          {visibleHosts.length === 0 ? (
            <PlatformEmptyState icon={Server} title={filterOffline ? 'No offline hosts' : 'No hosts match'} subtitle={filterOffline ? 'All hypervisors are reporting heartbeats.' : 'Clear the filter to see every machine.'} />
          ) : selected ? (
            <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_360px] items-start">
              <div className="min-w-0 space-y-4">{body}</div>
              <HostDrawer host={selected} busy={busy} run={run} onClose={() => setSelectedId(null)} now={now} />
            </div>
          ) : (
            <div className="space-y-4">{body}</div>
          )}
        </>
      )}
      <HostEnrollWizard open={enrollWizardOpen} onClose={() => setEnrollWizardOpen(false)} />
    </PageLayout>
  )
}
