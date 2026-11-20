// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { createNetwork, deleteNetwork, listNetworks, type NativeNetwork } from '../api/nativeNetworks'
import { createPort, deletePort, listPorts, type NativePort } from '../api/nativePorts'
import { listVms, type NativeVm } from '../api/nativeVms'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import PageSkeleton from '../components/PageSkeleton'
import ConfirmDialog from '../components/ConfirmDialog'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses } from '../utils/semanticColors'
import { Network, Plus, RefreshCw } from 'lucide-react'

// Native networking overview — no old external-cloud gate component in the way
// any more (the daemon's external-cloud-client integration has since been
// fully removed). Networks (already pre-existing, libvirt-backed) and ports
// (this session's Neutron-port equivalent) only — no subnets/routers, since
// Machina has no native L3 routing layer. See FleetCloudTopology.tsx for the
// graph view of the same data.
export default function FleetCloudNetworkingPage() {
  return <FleetCloudNetworkingContent />
}

function FleetCloudNetworkingContent() {
  const toast = useToastContext()
  const [networks, setNetworks] = useState<NativeNetwork[]>([])
  const [ports, setPorts] = useState<NativePort[]>([])
  const [vms, setVms] = useState<NativeVm[]>([])
  const [loading, setLoading] = useState(true)
  const [newNetName, setNewNetName] = useState('')
  const [creatingNet, setCreatingNet] = useState(false)
  const [portNetId, setPortNetId] = useState('')
  const [portVmId, setPortVmId] = useState('')
  const [creatingPort, setCreatingPort] = useState(false)
  const [netSearch, setNetSearch] = useState('')
  const [portSearch, setPortSearch] = useState('')
  const [pendingDeleteNetwork, setPendingDeleteNetwork] = useState<NativeNetwork | null>(null)
  const [pendingDeletePort, setPendingDeletePort] = useState<NativePort | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const [n, p, v] = await Promise.all([listNetworks(), listPorts(), listVms().catch(() => [])])
      setNetworks(n)
      setPorts(p)
      setVms(v)
      if (!portNetId && n.length > 0) setPortNetId(n[0].id)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setLoading(false)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [toast])

  useEffect(() => { void load() }, [load])

  const vmName = (id: string | null) => (id ? vms.find((v) => v.id === id)?.name || id.slice(0, 8) : '—')
  const netName = (id: string) => networks.find((n) => n.id === id)?.name || id.slice(0, 8)

  const filteredNetworks = useMemo(() => {
    const q = netSearch.trim().toLowerCase()
    if (!q) return networks
    return networks.filter(
      (n) => n.name.toLowerCase().includes(q) || n.id.toLowerCase().includes(q) || n.backend.toLowerCase().includes(q),
    )
  }, [networks, netSearch])

  const filteredPorts = useMemo(() => {
    const q = portSearch.trim().toLowerCase()
    if (!q) return ports
    return ports.filter((p) => {
      const nName = netName(p.network_id).toLowerCase()
      const vName = p.vm_id ? vmName(p.vm_id).toLowerCase() : ''
      return (
        nName.includes(q) ||
        vName.includes(q) ||
        p.id.toLowerCase().includes(q) ||
        (p.mac_address?.toLowerCase().includes(q) ?? false)
      )
    })
  }, [ports, portSearch, networks, vms])

  if (loading) return <PageSkeleton />

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <div className="flex items-center justify-between">
        <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
          <Network className="w-7 h-7 text-[var(--accent)]" />
          Networking
        </h1>
        <button type="button" onClick={() => void load()}
          className="btn-secondary text-sm inline-flex items-center gap-1">
          <RefreshCw className="w-4 h-4" /> Refresh
        </button>
      </div>

      <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3">
        <h2 className="text-sm font-medium text-[var(--text-secondary)] flex items-center gap-2"><Plus className="w-4 h-4" /> Create network</h2>
        <div className="flex flex-wrap gap-2 items-end">
          <input aria-label="Network name" value={newNetName} onChange={(e) => setNewNetName(e.target.value)} placeholder="Name"
            className="input-field text-sm" />
          <button type="button" disabled={!newNetName.trim() || creatingNet}
            className="btn-primary text-sm disabled:opacity-40"
            onClick={async () => {
              setCreatingNet(true)
              try {
                await createNetwork({ name: newNetName.trim() })
                toast.success('Network created')
                setNewNetName('')
                void load()
              } catch (e: unknown) { toast.error(formatUserError(e)) } finally { setCreatingNet(false) }
            }}>{creatingNet ? 'Creating…' : 'Create'}</button>
        </div>
        <TahoeToolbar
          search={netSearch}
          onSearchChange={setNetSearch}
          placeholder="Search networks…"
        />
        <TahoeTableWrap>
          <table className="apple-table" aria-label="Networks">
            <thead>
              <tr>
                <th scope="col">Name</th>
                <th scope="col">Backend</th>
                <th scope="col">VLAN</th>
                <th scope="col" />
              </tr>
            </thead>
            <tbody>
              {filteredNetworks.length === 0 && (
                <tr><td colSpan={4} className="text-center text-[var(--text-muted)]">
                  {netSearch.trim() ? 'No networks match your search.' : 'No networks.'}
                </td></tr>
              )}
              {filteredNetworks.map((n) => (
                <tr key={n.id}>
                  <td className="text-[var(--text-primary)]">
                    <Link to={`/fleet-cloud/networks/${n.id}`} className="apple-link">{n.name}</Link>
                  </td>
                  <td className="text-[var(--text-muted)]">{n.backend}</td>
                  <td className="text-[var(--text-muted)]">{n.vlan_id ?? '—'}</td>
                  <td>
                    <button type="button" className={`min-h-9 px-1 ${statusActionLinkClasses('error', 'text-xs')}`}
                      onClick={() => setPendingDeleteNetwork(n)}>Delete</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </TahoeTableWrap>
      </section>

      <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3">
        <h2 className="text-sm font-medium text-[var(--text-secondary)] flex items-center gap-2"><Plus className="w-4 h-4" /> Create port</h2>
        <div className="flex flex-wrap gap-2 items-end">
          <select value={portNetId} onChange={(e) => setPortNetId(e.target.value)}
            aria-label="Network" className="input-field text-sm min-w-[10rem]">
            {networks.map((n) => <option key={n.id} value={n.id}>{n.name}</option>)}
          </select>
          <select value={portVmId} onChange={(e) => setPortVmId(e.target.value)}
            aria-label="VM (optional)" className="input-field text-sm min-w-[10rem]">
            <option value="">Unbound port</option>
            {vms.map((v) => <option key={v.id} value={v.id}>{v.name}</option>)}
          </select>
          <button type="button" disabled={!portNetId || creatingPort}
            className="btn-primary text-sm disabled:opacity-40"
            onClick={async () => {
              setCreatingPort(true)
              try {
                await createPort({ network_id: portNetId, vm_id: portVmId || undefined })
                toast.success('Port created')
                void load()
              } catch (e: unknown) { toast.error(formatUserError(e)) } finally { setCreatingPort(false) }
            }}>{creatingPort ? 'Creating…' : 'Create'}</button>
        </div>
        <TahoeToolbar
          search={portSearch}
          onSearchChange={setPortSearch}
          placeholder="Search ports…"
        />
        <TahoeTableWrap>
          <table className="apple-table" aria-label="Ports">
            <thead>
              <tr>
                <th scope="col">Network</th>
                <th scope="col">VM</th>
                <th scope="col">MAC</th>
                <th scope="col">Status</th>
                <th scope="col" />
              </tr>
            </thead>
            <tbody className="font-mono text-xs">
              {filteredPorts.length === 0 && (
                <tr><td colSpan={5} className="text-center text-[var(--text-muted)]">
                  {portSearch.trim() ? 'No ports match your search.' : 'No ports.'}
                </td></tr>
              )}
              {filteredPorts.map((p) => (
                <tr key={p.id}>
                  <td>{netName(p.network_id)}</td>
                  <td>
                    {p.vm_id ? <Link to={`/fleet-cloud/instances/${p.vm_id}`} className="text-[var(--accent)] hover:underline">{vmName(p.vm_id)}</Link> : '—'}
                  </td>
                  <td className="text-[var(--text-muted)]">{p.mac_address || '—'}</td>
                  <td className="text-[var(--text-muted)]">{p.status}</td>
                  <td>
                    <button type="button" className={`min-h-9 px-1 ${statusActionLinkClasses('error', 'text-xs')}`}
                      onClick={() => setPendingDeletePort(p)}>Delete</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </TahoeTableWrap>
      </section>

      <ConfirmDialog
        open={!!pendingDeleteNetwork}
        title="Delete network"
        message={pendingDeleteNetwork ? `Delete network ${pendingDeleteNetwork.name}?` : ''}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setPendingDeleteNetwork(null)}
        onConfirm={async () => {
          if (!pendingDeleteNetwork) return
          const target = pendingDeleteNetwork
          setPendingDeleteNetwork(null)
          try {
            await deleteNetwork(target.id)
            toast.success('Deleted')
            void load()
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}
      />
      <ConfirmDialog
        open={!!pendingDeletePort}
        title="Delete port"
        message="Delete this port?"
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setPendingDeletePort(null)}
        onConfirm={async () => {
          if (!pendingDeletePort) return
          const target = pendingDeletePort
          setPendingDeletePort(null)
          try {
            await deletePort(target.id)
            toast.success('Deleted')
            void load()
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}
      />

      <FleetCloudFooter />
    </PageLayout>
  )
}
