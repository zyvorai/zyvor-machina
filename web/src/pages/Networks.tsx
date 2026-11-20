// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, useCallback, useRef } from 'react'
import { listNetworks, createNetwork, startNetwork, stopNetwork, deleteNetwork, setNetworkAutostart, getNetworkXml, setNetworkXml, NetworkInfo } from '../api/network'
import { useFocusTrap } from '../hooks/useFocusTrap'
import { listDhcpLeases, DhcpLease, serviceAction } from '../api/extras'
import { getHostLibvirtBoot, type LibvirtBootStatus } from '../api/host'
import { useToastContext } from '../contexts/ToastContext'
import ConfirmDialog from '../components/ConfirmDialog'
import { Play, Square, Trash2, ToggleLeft, ToggleRight, RefreshCw, Plus, Network, Wifi, X, Pencil } from 'lucide-react'
import { formatUserError } from '../utils/apiError'
import { statusBadgeClasses, statusSurfaceClasses, statusToneClass } from '../utils/semanticColors'
import EmptyState from '../components/EmptyState'
import PageLayout from '../components/PageLayout'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { libvirtErrorHints } from '../utils/libvirtHints'

export default function NetworksPage() {
  const [networks, setNetworks] = useState<NetworkInfo[]>([])
  const [search, setSearch] = useState('')
  const [leases, setLeases] = useState<DhcpLease[]>([])
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null)
  const [showCreate, setShowCreate] = useState(false)
  const [creating, setCreating] = useState(false)
  const createRef = useRef<HTMLDivElement>(null)
  const editRef = useRef<HTMLDivElement>(null)
  const [newName, setNewName] = useState('')
  const [newSubnet, setNewSubnet] = useState('192.168.100')
  const [newDhcpStart, setNewDhcpStart] = useState('192.168.100.100')
  const [newDhcpEnd, setNewDhcpEnd] = useState('192.168.100.254')
  const [libvirtBoot, setLibvirtBoot] = useState<LibvirtBootStatus | null>(null)
  const [libvirtBootBusy, setLibvirtBootBusy] = useState(false)
  const [editTarget, setEditTarget] = useState<NetworkInfo | null>(null)
  const [editXml, setEditXml] = useState('')
  const [editXmlLoading, setEditXmlLoading] = useState(false)
  const [editXmlSaving, setEditXmlSaving] = useState(false)
  const toast = useToastContext()
  useFocusTrap(createRef, showCreate, () => setShowCreate(false))
  useFocusTrap(editRef, editTarget !== null, () => { if (!editXmlSaving) setEditTarget(null) })

  const load = useCallback(async () => {
    setLoading(true)
    const [nets, dhcp, lb] = await Promise.allSettled([
      listNetworks(),
      listDhcpLeases(),
      getHostLibvirtBoot(),
    ])
    if (nets.status === 'fulfilled') {
      setNetworks(nets.value)
      setLoadError(null)
    } else {
      setLoadError(formatUserError(nets.reason))
    }
    if (dhcp.status === 'fulfilled') setLeases(dhcp.value)
    if (lb.status === 'fulfilled') setLibvirtBoot(lb.value)
    else setLibvirtBoot(null)
    setLoading(false)
  }, [])

  useEffect(() => { load() }, [load])

  const action = async (name: string, fn: (n: string) => Promise<void>, label: string) => {
    try { await fn(name); toast.success(`${label} '${name}' OK`); load() } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
  }

  const toggleAutostart = async (net: NetworkInfo) => {
    try { await setNetworkAutostart(net.name, !net.autostart); toast.success(`Autostart ${!net.autostart ? 'enabled' : 'disabled'}`); load() } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
  }

  const handleDelete = async () => {
    if (!deleteTarget) return
    await action(deleteTarget, deleteNetwork, 'Delete network')
    setDeleteTarget(null)
  }

  const handleCreate = async () => {
    if (!newName.trim() || creating) return
    setCreating(true)
    try {
      await createNetwork({ name: newName.trim(), subnet: newSubnet, dhcp_start: newDhcpStart, dhcp_end: newDhcpEnd })
      toast.success(`Network '${newName}' created`)
      setShowCreate(false); setNewName(''); load()
    } catch (e: unknown) { toast.error(`${formatUserError(e)}`) }
    finally { setCreating(false) }
  }

  const openEditXml = async (net: NetworkInfo) => {
    setEditTarget(net)
    setEditXml('')
    setEditXmlLoading(true)
    try {
      const xml = await getNetworkXml(net.name)
      setEditXml(xml)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
      setEditTarget(null)
    } finally {
      setEditXmlLoading(false)
    }
  }

  const saveEditXml = async () => {
    if (!editTarget) return
    setEditXmlSaving(true)
    try {
      await setNetworkXml(editTarget.name, editXml)
      toast.success(`Updated network '${editTarget.name}'`)
      setEditTarget(null)
      load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setEditXmlSaving(false)
    }
  }

  const filteredNetworks = networks.filter((net) => {
    const q = search.trim().toLowerCase()
    if (!q) return true
    return (
      net.name.toLowerCase().includes(q)
      || (net.bridge || '').toLowerCase().includes(q)
      || (net.active ? 'active' : 'inactive').includes(q)
    )
  })

  const filteredLeases = leases.filter((l) => {
    const q = search.trim().toLowerCase()
    if (!q) return true
    return (
      l.network.toLowerCase().includes(q)
      || l.ip.toLowerCase().includes(q)
      || l.mac.toLowerCase().includes(q)
      || (l.hostname || '').toLowerCase().includes(q)
    )
  })

  const enableLibvirtBootUnit = async () => {
    if (!libvirtBoot?.needs_attention || !libvirtBoot.systemd_unit) return
    setLibvirtBootBusy(true)
    try {
      await serviceAction(libvirtBoot.systemd_unit, 'enable_now')
      toast.success(
        `Enabled and started ${libvirtBoot.systemd_unit}. If the dashboard stalls briefly, libvirt is reconnecting — wait or refresh.`,
      )
      try {
        setLibvirtBoot(await getHostLibvirtBoot())
      } catch {
        setLibvirtBoot(null)
      }
    } catch (e: unknown) {
      toast.error(`${formatUserError(e)}`)
    } finally {
      setLibvirtBootBusy(false)
    }
  }

  return (
    <PageLayout
      eyebrow="Hypervisor"
      loading={loading}
      title="Networks"
      icon={<Network className={`w-6 h-6 ${statusToneClass('info')}`} />}
      actions={
        <>
          <button onClick={() => setShowCreate(true)} className="btn-primary text-sm inline-flex items-center gap-1"><Plus className="w-4 h-4" /> Create</button>
          <button onClick={load} className="p-2 hover:bg-[var(--surface-hover)] rounded transition" aria-label="Refresh"><RefreshCw className="w-4 h-4" /></button>
        </>
      }
      error={loadError}
      errorTitle="Could not load networks"
      errorHints={loadError ? libvirtErrorHints(loadError) : undefined}
      onErrorRetry={load}
      contentClassName="space-y-6"
    >
      {libvirtBoot?.needs_attention && libvirtBoot.detail && (
        <div className={`rounded-xl px-4 py-3 flex flex-col sm:flex-row sm:items-start sm:justify-between gap-3 ${statusSurfaceClasses('warn')}`}>
          <div className="min-w-0">
            <p className={`text-sm font-medium ${statusToneClass('warn')}`}>Libvirt networks after host reboot</p>
            <p className={`text-xs mt-1 leading-relaxed opacity-90 ${statusToneClass('warn')}`}>{libvirtBoot.detail}</p>
            <p className="text-xs text-[var(--text-muted)] mt-1.5">
              Per-network Autostart below only applies once the libvirt daemon for NAT (<code className="text-[var(--text-muted)]">virtnetworkd</code> or <code className="text-[var(--text-muted)]">libvirtd</code>) starts at boot.
            </p>
          </div>
          {libvirtBoot.systemd_unit ? (
            <button
              type="button"
              disabled={libvirtBootBusy}
              onClick={() => void enableLibvirtBootUnit()}
              className={`shrink-0 px-3 py-1.5 rounded-lg text-sm font-medium border disabled:opacity-50 transition ${statusBadgeClasses('warn')} border-[color-mix(in_srgb,var(--machina-status-warn)_40%,transparent)] hover:bg-[color-mix(in_srgb,var(--machina-status-warn)_15%,transparent)]`}
            >
              {libvirtBootBusy ? 'Running…' : `Enable at boot (${libvirtBoot.systemd_unit})`}
            </button>
          ) : null}
        </div>
      )}

      <TahoeToolbar
        search={search}
        onSearchChange={setSearch}
        placeholder="Search networks…"
        trailing={
          search ? (
            <span aria-live="polite" className="text-sm text-[var(--text-muted)] shrink-0 pr-2">
              {filteredNetworks.length} network{filteredNetworks.length !== 1 ? 's' : ''}
            </span>
          ) : null
        }
      />

      {networks.length === 0 ? (
        <EmptyState
          icon={<Network className="w-6 h-6" />}
          title="No libvirt networks"
          description="Create a NAT network for guest connectivity, or define an isolated bridge for lab topologies."
          primaryAction={
            <button type="button" onClick={() => setShowCreate(true)} className="btn-primary text-sm">
              Create network
            </button>
          }
        />
      ) : (
      <TahoeTableWrap>
        <table className="apple-table" aria-label="Virtual networks">
          <thead><tr><th scope="col">Name</th><th scope="col">Active</th><th scope="col" className="hidden md:table-cell">Bridge</th><th scope="col" className="hidden md:table-cell">Autostart</th><th scope="col" className="text-right">Actions</th></tr></thead>
          <tbody>
            {filteredNetworks.length === 0 && (
              <tr><td colSpan={5} className="text-center text-[var(--text-muted)]">No networks match your search.</td></tr>
            )}
            {filteredNetworks.map((net) => (
              <tr key={net.name}>
                <td className="font-medium"><Wifi className={`w-4 h-4 inline -mt-0.5 mr-1 ${statusToneClass('ok')}`} />{net.name}</td>
                <td><span className={`px-2 py-0.5 rounded text-xs font-medium ${statusBadgeClasses(net.active ? 'ok' : 'error')}`}>{net.active ? 'Active' : 'Inactive'}</span></td>
                <td className="hidden md:table-cell text-sm text-[var(--text-muted)] font-mono">{net.bridge || '-'}</td>
                <td className="hidden md:table-cell">
                  <button onClick={() => toggleAutostart(net)} aria-label={net.autostart ? 'Disable autostart' : 'Enable autostart'} className="flex items-center gap-1">
                    {net.autostart ? <ToggleRight className={`w-5 h-5 ${statusToneClass('ok')}`} /> : <ToggleLeft className="w-5 h-5 text-[var(--text-muted)]" />}
                  </button>
                </td>
                <td>
                  <div className="flex items-center justify-end gap-1">
                    <button type="button" onClick={() => void openEditXml(net)} className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition" title="Edit XML" aria-label="Edit XML"><Pencil className="w-4 h-4 text-[var(--text-secondary)]" /></button>
                    {!net.active && <button onClick={() => action(net.name, startNetwork, 'Start network')} className="p-1.5 hover:bg-green-600/20 rounded transition" title="Start" aria-label="Start"><Play className={`w-4 h-4 ${statusToneClass('ok')}`} /></button>}
                    {net.active && <button onClick={() => action(net.name, stopNetwork, 'Stop network')} className="p-1.5 hover:bg-red-600/20 rounded transition" title="Stop" aria-label="Stop"><Square className={`w-4 h-4 ${statusToneClass('error')}`} /></button>}
                    <button onClick={() => setDeleteTarget(net.name)} className="p-1.5 hover:bg-red-600/20 rounded transition" title="Delete" aria-label="Delete"><Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} /></button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </TahoeTableWrap>
      )}

      {/* DHCP Leases */}
      {leases.length > 0 && (
        <div className="mt-6">
          <h2 className="text-sm font-semibold text-[var(--text-secondary)] mb-3">DHCP Leases</h2>
          <TahoeTableWrap>
            <table className="apple-table text-sm" aria-label="DHCP leases">
              <thead><tr><th scope="col">Network</th><th scope="col">IP Address</th><th scope="col">MAC</th><th scope="col">Hostname</th><th scope="col">Expires</th></tr></thead>
              <tbody>
                {filteredLeases.length === 0 && search && (
                  <tr><td colSpan={5} className="text-center text-[var(--text-muted)]">No leases match your search.</td></tr>
                )}
                {filteredLeases.map((l) => (
                  <tr key={`${l.mac}-${l.ip}`}>
                    <td className="text-[var(--text-muted)]">{l.network}</td>
                    <td className={`font-mono ${statusToneClass('info')}`}>{l.ip}</td>
                    <td className="font-mono text-xs text-[var(--text-muted)]">{l.mac}</td>
                    <td>{l.hostname || '-'}</td>
                    <td className="text-[var(--text-muted)]">{l.expiry}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        </div>
      )}

      {/* Edit network XML */}
      {editTarget && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4" role="dialog" aria-modal="true" onClick={() => !editXmlSaving && setEditTarget(null)}>
          <div ref={editRef} className="bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-2xl shadow-2xl w-full max-w-4xl max-h-[90vh] flex flex-col" onClick={e => e.stopPropagation()}>
            <div className="p-5 border-b border-[var(--apple-hairline)] flex items-center justify-between shrink-0">
              <div>
                <span className="text-lg font-semibold flex items-center gap-2"><Network className={`w-5 h-5 ${statusToneClass('info')}`} /> Edit network XML</span>
                <p className="text-xs text-[var(--text-muted)] mt-1 font-mono">{editTarget.name}</p>
              </div>
              <button type="button" aria-label="Close" onClick={() => !editXmlSaving && setEditTarget(null)} className="p-1 hover:bg-[var(--surface-hover)] rounded"><X className="w-4 h-4 text-[var(--text-muted)]" /></button>
            </div>
            <div className="p-5 flex-1 min-h-0 flex flex-col gap-3">
              {editTarget.active && (
                <p className={`text-xs rounded-lg px-3 py-2 ${statusSurfaceClasses('warn')}`}>
                  This network is active. Saving applies the new definition and briefly restarts the network (guest NICs may drop traffic for a moment).
                </p>
              )}
              {editXmlLoading ? (
                <div className="flex justify-center py-12 text-[var(--text-muted)] text-sm">Loading XML…</div>
              ) : (
                <>
                  <label htmlFor="net-xml-edit" className="text-sm text-[var(--text-muted)]">Libvirt network XML (keep <code className="text-[var(--text-muted)]">&lt;name&gt;</code> equal to <span className="font-mono text-[var(--text-secondary)]">{editTarget.name}</span>)</label>
                  <textarea
                    id="net-xml-edit"
                    value={editXml}
                    onChange={e => setEditXml(e.target.value)}
                    spellCheck={false}
                    className="w-full min-h-[280px] flex-1 font-mono text-xs bg-[var(--apple-surface)] border border-[var(--apple-hairline)]/50 rounded-lg px-3 py-2 text-[var(--text-primary)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)]/40 resize-y"
                    disabled={editXmlSaving}
                  />
                </>
              )}
            </div>
            <div className="flex justify-end gap-3 px-5 pb-5 shrink-0 border-t border-[var(--apple-hairline)] pt-4">
              <button type="button" onClick={() => !editXmlSaving && setEditTarget(null)} className="btn-secondary text-sm font-medium transition">Cancel</button>
              <button
                type="button"
                disabled={editXmlSaving || editXmlLoading || !editXml.trim()}
                onClick={() => void saveEditXml()}
                className="btn-primary text-sm disabled:opacity-50"
              >
                {editXmlSaving ? 'Saving…' : 'Save definition'}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Create Network Dialog */}
      {showCreate && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" role="dialog" aria-modal="true" onClick={() => setShowCreate(false)}>
          <div ref={createRef} className="bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-2xl shadow-2xl w-full max-w-xl mx-4" onClick={e => e.stopPropagation()}>
            <div className="p-5 border-b border-[var(--apple-hairline)] flex items-center justify-between">
              <span className="text-lg font-semibold flex items-center gap-2"><Network className={`w-5 h-5 ${statusToneClass('info')}`} /> Create Network</span>
              <button aria-label="Close" onClick={() => setShowCreate(false)} className="p-1 hover:bg-[var(--surface-hover)] rounded"><X className="w-4 h-4 text-[var(--text-muted)]" /></button>
            </div>
            <div className="p-5 space-y-3">
              <div><label htmlFor="net-name" className="block text-sm text-[var(--text-muted)] mb-1">Name</label><input id="net-name" autoFocus value={newName} onChange={e => setNewName(e.target.value)} className="input-field" placeholder="my-network" /></div>
              <div><label htmlFor="net-subnet" className="block text-sm text-[var(--text-muted)] mb-1">Subnet Prefix</label><input id="net-subnet" value={newSubnet} onChange={e => setNewSubnet(e.target.value)} className="input-field" placeholder="192.168.100" /></div>
              <div className="grid grid-cols-2 gap-3">
                <div><label htmlFor="net-dhcp-start" className="block text-sm text-[var(--text-muted)] mb-1">DHCP Start</label><input id="net-dhcp-start" value={newDhcpStart} onChange={e => setNewDhcpStart(e.target.value)} className="input-field" /></div>
                <div><label htmlFor="net-dhcp-end" className="block text-sm text-[var(--text-muted)] mb-1">DHCP End</label><input id="net-dhcp-end" value={newDhcpEnd} onChange={e => setNewDhcpEnd(e.target.value)} className="input-field" /></div>
              </div>
              <p className="text-xs text-[var(--text-muted)]">Creates a NAT network with the given subnet and DHCP range.</p>
            </div>
            <div className="flex justify-end gap-3 px-5 pb-5">
              <button onClick={() => setShowCreate(false)} className="btn-secondary text-sm font-medium transition">Cancel</button>
              <button onClick={handleCreate} disabled={creating || !newName.trim()} className="btn-primary text-sm disabled:opacity-40 disabled:cursor-not-allowed">{creating ? 'Creating…' : 'Create'}</button>
            </div>
          </div>
        </div>
      )}

      <ConfirmDialog open={!!deleteTarget} title="Delete Network" message={`Delete network '${deleteTarget}'?`} confirmLabel="Delete" onConfirm={handleDelete} onCancel={() => setDeleteTarget(null)} />
    </PageLayout>
  )
}
