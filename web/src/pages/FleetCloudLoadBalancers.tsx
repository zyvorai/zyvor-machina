// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { Loader2, Plus, Scale, Trash2 } from 'lucide-react'
import ConfirmDialog from '../components/ConfirmDialog'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import EmptyState from '../components/EmptyState'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { listPlatformHosts, type PlatformHost } from '../api/platform'
import {
  createLoadBalancer,
  deleteLoadBalancer,
  listLoadBalancers,
  type NativeLoadBalancer,
} from '../api/nativeLoadBalancers'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses, statusToneClass } from '../utils/semanticColors'

// Native L4 load balancer — like the other rewired /fleet-cloud/* pages, this
// no longer depends on a wired external cloud (see
// api/nativeLoadBalancers.ts). There's no old external-cloud gate component to wrap it
// in any more either: the daemon's external-cloud-client integration has
// since been fully removed, so this page always renders.
export default function FleetCloudLoadBalancersPage() {
  return <FleetCloudLoadBalancersContent />
}

function FleetCloudLoadBalancersContent() {
  const toast = useToastContext()
  const [lbs, setLbs] = useState<NativeLoadBalancer[]>([])
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [loading, setLoading] = useState(true)
  const [name, setName] = useState('')
  const [hostId, setHostId] = useState('')
  const [listenerPort, setListenerPort] = useState('8080')
  const [search, setSearch] = useState('')
  const [pendingDelete, setPendingDelete] = useState<NativeLoadBalancer | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const [lbR, hostR] = await Promise.all([
        listLoadBalancers(),
        listPlatformHosts().catch(() => []),
      ])
      setLbs(lbR)
      setHosts(hostR)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
      setLbs([])
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => { void load() }, [load])

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return lbs
    return lbs.filter((lb) => {
      const hostName = hosts.find((h) => h.id === lb.host_id)?.hostname?.toLowerCase() ?? ''
      return (
        lb.name.toLowerCase().includes(q) ||
        lb.id.toLowerCase().includes(q) ||
        lb.status.toLowerCase().includes(q) ||
        hostName.includes(q)
      )
    })
  }, [lbs, search, hosts])

  return (
    <PageLayout hideHeader prepend={<><FleetCloudSubNav /></>}>
      <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <Scale className={`w-7 h-7 ${statusToneClass('ok')}`} /> Load balancers
      </h1>
      <p className="text-[var(--text-muted)] text-sm">
        Native, kernel-level L4 (TCP/UDP) load balancing — a weighted round-robin iptables rule set on the
        chosen host, no external cloud or amphora VM required.
      </p>

      <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 flex flex-wrap gap-3 items-end">
        <div>
          <label className="block text-xs text-[var(--text-muted)] mb-1">Name</label>
          <input value={name} onChange={(e) => setName(e.target.value)} aria-label="Name" className="input-field text-sm" />
        </div>
        <div>
          <label className="block text-xs text-[var(--text-muted)] mb-1">Host</label>
          <select value={hostId} onChange={(e) => setHostId(e.target.value)}
            aria-label="Host"
            className="input-field text-sm min-w-[14rem]">
            <option value="">Select host…</option>
            {hosts.map((h) => (
              <option key={h.id} value={h.id}>{h.hostname}</option>
            ))}
          </select>
        </div>
        <div>
          <label className="block text-xs text-[var(--text-muted)] mb-1">Listener port</label>
          <input value={listenerPort} onChange={(e) => setListenerPort(e.target.value)} aria-label="Listener port"
            className="w-24 input-field text-sm" />
        </div>
        <button type="button" disabled={!name.trim() || !hostId || !Number(listenerPort)}
          className="btn-primary text-sm disabled:opacity-40 inline-flex items-center gap-1"
          onClick={async () => {
            try {
              await createLoadBalancer({ name: name.trim(), host_id: hostId, listener_port: Number(listenerPort) })
              toast.success('Load balancer created')
              setName('')
              void load()
            } catch (e: unknown) { toast.error(formatUserError(e)) }
          }}>
          <Plus className="w-4 h-4" /> Create
        </button>
      </div>

      {loading ? (
        <Loader2 className="w-8 h-8 animate-spin text-[var(--accent)] mx-auto" />
      ) : lbs.length === 0 ? (
        <EmptyState title="No load balancers" description="Create one above — pick a host and listener port, then add members." />
      ) : (
        <>
          <TahoeToolbar
            search={search}
            onSearchChange={setSearch}
            placeholder="Search name, host, or status…"
          />
          <TahoeTableWrap>
            <table className="apple-table" aria-label="Load balancers">
              <thead>
                <tr>
                  <th scope="col">Name</th>
                  <th scope="col">Listener</th>
                  <th scope="col">Host</th>
                  <th scope="col">Status</th>
                  <th scope="col" />
                </tr>
              </thead>
              <tbody>
                {filtered.length === 0 && (
                  <tr>
                    <td colSpan={5} className="text-center text-[var(--text-muted)]">No load balancers match your search.</td>
                  </tr>
                )}
                {filtered.map((lb) => (
                  <tr key={lb.id}>
                    <td>
                      <Link to={`/fleet-cloud/load-balancers/${lb.id}`} className="apple-link">{lb.name}</Link>
                    </td>
                    <td className="font-mono">{lb.protocol}/{lb.listener_port}</td>
                    <td className="font-mono text-xs">{hosts.find((h) => h.id === lb.host_id)?.hostname ?? lb.host_id}</td>
                    <td>{lb.status}</td>
                    <td className="text-right">
                      <button type="button" aria-label="Delete" className={statusActionLinkClasses('error', 'inline-flex items-center gap-1')}
                        onClick={() => setPendingDelete(lb)}>
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        </>
      )}
      <ConfirmDialog
        open={!!pendingDelete}
        title="Delete load balancer"
        message={pendingDelete ? `Delete ${pendingDelete.name}?` : ''}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setPendingDelete(null)}
        onConfirm={async () => {
          if (!pendingDelete) return
          const target = pendingDelete
          setPendingDelete(null)
          try {
            await deleteLoadBalancer(target.id)
            toast.success('Deleted')
            void load()
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}
      />
      <FleetCloudFooter />
    </PageLayout>
  )
}
