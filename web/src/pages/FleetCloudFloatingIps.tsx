// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import {
  createVmPortForward,
  deleteVmPortForward,
  listVmPortForwards,
  listVms,
  type NativePortForward,
  type NativeVm,
} from '../api/nativeVms'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import ConfirmDialog from '../components/ConfirmDialog'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses } from '../utils/semanticColors'
import { Globe, Loader2, RefreshCw } from 'lucide-react'
import { platformStatSubtitle } from '../components/platform/PlatformPageChrome'

const PROTOCOLS = ['tcp', 'udp'] as const

// Native "floating IPs" — there's no old external-cloud gate component in the
// way: the daemon's external-cloud-client integration has since been
// fully removed. There's no allocatable floating-IP pool; the native equivalent
// is a per-VM host_port -> vm_port NAT rule (controller::api::vms::port_forwards,
// already built) — see api/nativeVms.ts. Pick an instance, then manage its
// forwards.
export default function FleetCloudFloatingIpsPage() {
  return <FleetCloudFloatingIpsContent />
}

function FleetCloudFloatingIpsContent() {
  const toast = useToastContext()
  const [vms, setVms] = useState<NativeVm[]>([])
  const [vmId, setVmId] = useState('')
  const [forwards, setForwards] = useState<NativePortForward[]>([])
  const [loading, setLoading] = useState(true)
  const [protocol, setProtocol] = useState<string>(PROTOCOLS[0])
  const [hostPort, setHostPort] = useState('')
  const [vmPort, setVmPort] = useState('')
  const [description, setDescription] = useState('')
  const [creating, setCreating] = useState(false)
  const [search, setSearch] = useState('')
  const [pendingRemove, setPendingRemove] = useState<NativePortForward | null>(null)

  const loadVms = useCallback(async () => {
    try {
      const list = await listVms()
      setVms(list)
      if (!vmId && list.length > 0) setVmId(list[0].id)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [toast])

  useEffect(() => { void loadVms() }, [loadVms])

  const loadForwards = useCallback(async () => {
    if (!vmId) { setForwards([]); return }
    setLoading(true)
    try {
      const f = await listVmPortForwards(vmId)
      setForwards(f)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
      setForwards([])
    } finally {
      setLoading(false)
    }
  }, [vmId, toast])

  useEffect(() => { void loadForwards() }, [loadForwards])

  const selectedVm = vms.find((v) => v.id === vmId)

  const filteredForwards = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return forwards
    return forwards.filter(
      (f) =>
        f.protocol.toLowerCase().includes(q) ||
        String(f.host_port).includes(q) ||
        String(f.vm_port).includes(q) ||
        (f.description?.toLowerCase().includes(q) ?? false),
    )
  }, [forwards, search])

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <Globe className="w-7 h-7 text-[var(--accent)]" />
        Floating IPs (port forwards)
      </h1>
      {platformStatSubtitle([{ label: 'Port forwards', value: forwards.length }, { label: 'Instances', value: vms.length }])}
      <p className="text-sm text-[var(--text-muted)]">
        No allocatable floating-IP pool natively — reach a VM's service from outside via a
        host_port → vm_port NAT rule instead.
      </p>

      <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 flex flex-wrap gap-3 items-end text-sm">
        <div>
          <label className="block text-xs text-[var(--text-muted)] mb-1">Instance</label>
          <select value={vmId} onChange={(e) => setVmId(e.target.value)}
            aria-label="Instance"
            className="input-field min-w-[12rem]">
            {vms.map((v) => <option key={v.id} value={v.id}>{v.name}</option>)}
          </select>
        </div>
        {selectedVm && (
          <span className="text-xs text-[var(--text-muted)] font-mono">guest IP: {selectedVm.guest_ip || 'unknown yet'}</span>
        )}
        <button type="button" onClick={() => void loadForwards()}
          className="ml-auto inline-flex items-center gap-1 px-3 py-1.5 rounded-lg border border-[var(--apple-hairline)]">
          <RefreshCw className="w-4 h-4" /> Refresh
        </button>
      </div>

      <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3 text-sm">
        <h2 className="text-sm font-medium text-[var(--text-secondary)]">Add forward</h2>
        <div className="flex flex-wrap gap-3 items-end">
          <select value={protocol} onChange={(e) => setProtocol(e.target.value)}
            aria-label="Protocol"
            className="input-field">
            {PROTOCOLS.map((p) => <option key={p} value={p}>{p}</option>)}
          </select>
          <input aria-label="Host port" value={hostPort} onChange={(e) => setHostPort(e.target.value)} placeholder="Host port" type="number"
            className="w-28 input-field" />
          <input aria-label="VM port" value={vmPort} onChange={(e) => setVmPort(e.target.value)} placeholder="VM port" type="number"
            className="w-28 input-field" />
          <input aria-label="Description" value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Description (optional)"
            className="input-field min-w-[10rem]" />
          <button type="button" disabled={!vmId || !hostPort || !vmPort || creating}
            className="btn-primary text-sm disabled:opacity-40"
            onClick={async () => {
              const hp = Number.parseInt(hostPort, 10)
              const vp = Number.parseInt(vmPort, 10)
              if (!Number.isFinite(hp) || !Number.isFinite(vp)) {
                toast.warning('Ports must be numbers')
                return
              }
              setCreating(true)
              try {
                await createVmPortForward(vmId, { protocol, host_port: hp, vm_port: vp, description })
                toast.success('Forward added')
                setHostPort(''); setVmPort(''); setDescription('')
                void loadForwards()
              } catch (e: unknown) {
                toast.error(formatUserError(e))
              } finally {
                setCreating(false)
              }
            }}>{creating ? 'Adding…' : 'Add'}</button>
        </div>
      </div>

      {loading ? (
        <Loader2 className="w-8 h-8 animate-spin text-[var(--accent)] mx-auto" />
      ) : (
        <>
          <TahoeToolbar
            search={search}
            onSearchChange={setSearch}
            placeholder="Search protocol, ports, or description…"
          />
          <TahoeTableWrap>
            <table className="apple-table" aria-label="Port forwards">
              <thead>
                <tr>
                  <th scope="col">Protocol</th>
                  <th scope="col">Host port</th>
                  <th scope="col">VM port</th>
                  <th scope="col">Description</th>
                  <th scope="col" />
                </tr>
              </thead>
              <tbody>
                {filteredForwards.length === 0 && (
                  <tr>
                    <td colSpan={5} className="text-center text-[var(--text-muted)]">
                      {search.trim() ? 'No forwards match your search.' : 'No port forwards for this instance.'}
                    </td>
                  </tr>
                )}
                {filteredForwards.map((f) => (
                  <tr key={f.id}>
                    <td className="font-mono text-[var(--text-primary)]">{f.protocol}</td>
                    <td>{f.host_port}</td>
                    <td>{f.vm_port}</td>
                    <td className="text-[var(--text-muted)]">{f.description || '—'}</td>
                    <td>
                      <button type="button" className={statusActionLinkClasses('error', 'text-xs')}
                        onClick={() => setPendingRemove(f)}>Remove</button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        </>
      )}
      <ConfirmDialog
        open={!!pendingRemove}
        title="Remove port forward"
        message={pendingRemove ? `Remove forward ${pendingRemove.protocol}/${pendingRemove.host_port} → ${pendingRemove.vm_port}?` : ''}
        confirmLabel="Remove"
        variant="danger"
        onCancel={() => setPendingRemove(null)}
        onConfirm={async () => {
          if (!pendingRemove) return
          const target = pendingRemove
          setPendingRemove(null)
          try {
            await deleteVmPortForward(vmId, { protocol: target.protocol, host_port: target.host_port, vm_port: target.vm_port })
            toast.success('Removed')
            void loadForwards()
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}
      />
      {selectedVm && (
        <Link to={`/fleet-cloud/instances/${selectedVm.id}`} className="text-sm text-[var(--accent)] hover:underline">
          View instance
        </Link>
      )}
      <FleetCloudFooter />
    </PageLayout>
  )
}
