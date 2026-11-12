// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Gauge, Loader2, Plus, RefreshCw, Trash2 } from 'lucide-react'
import {
  bindVmQosPolicy,
  createVmQosPolicy,
  deleteVmQosPolicy,
  listVmQosPolicies,
  type VmQosPolicy,
} from '../api/vmQos'
import ConfirmDialog from '../components/ConfirmDialog'
import FleetCloudFooter from '../components/FleetCloudFooter'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import PageLayout from '../components/PageLayout'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'

export default function FleetCloudQosPage() {
  return <FleetCloudQosContent />
}

function FleetCloudQosContent() {
  const toast = useToastContext()
  const [policies, setPolicies] = useState<VmQosPolicy[]>([])
  const [loading, setLoading] = useState(true)
  const [name, setName] = useState('')
  const [egress, setEgress] = useState('100')
  const [ingress, setIngress] = useState('')
  const [pps, setPps] = useState('')
  const [bindVm, setBindVm] = useState('')
  const [bindPolicy, setBindPolicy] = useState('')
  const [creating, setCreating] = useState(false)
  const [search, setSearch] = useState('')
  const [pendingDelete, setPendingDelete] = useState<VmQosPolicy | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setPolicies(await listVmQosPolicies())
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => {
    void load()
  }, [load])

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return policies
    return policies.filter(
      (p) => p.name.toLowerCase().includes(q) || p.id.toLowerCase().includes(q),
    )
  }, [policies, search])

  const handleCreate = async () => {
    if (!name.trim()) {
      toast.error('Name is required')
      return
    }
    setCreating(true)
    try {
      await createVmQosPolicy({
        name: name.trim(),
        egress_mbps: egress.trim() ? Number(egress) : null,
        ingress_mbps: ingress.trim() ? Number(ingress) : null,
        pps: pps.trim() ? Number(pps) : null,
      })
      toast.success('QoS policy created')
      setName('')
      void load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setCreating(false)
    }
  }

  const handleBind = async () => {
    if (!bindPolicy || !bindVm.trim()) {
      toast.error('Pick a policy and VM id')
      return
    }
    try {
      const r = await bindVmQosPolicy(bindPolicy, bindVm.trim())
      toast.success(r.edge_pushed ? 'Bound and pushed to VM edge' : 'Bound (edge push pending)')
      setBindVm('')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
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
        <Gauge className="w-7 h-7 text-[var(--accent)]" />
        VM QoS
      </h1>
      <p className="text-sm text-[var(--text-secondary)] max-w-2xl">
        Bandwidth and packet-rate policies pushed into the bpfd VM-edge datapath on bind.
      </p>

      <div className="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-4 items-end rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4">
        <label className="text-xs text-[var(--text-muted)]">
          Name
          <input className="mt-1 w-full rounded-lg border border-[var(--apple-hairline)] bg-transparent px-2 py-1.5 text-sm" value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="text-xs text-[var(--text-muted)]">
          Egress Mbit/s
          <input className="mt-1 w-full rounded-lg border border-[var(--apple-hairline)] bg-transparent px-2 py-1.5 text-sm" value={egress} onChange={(e) => setEgress(e.target.value)} />
        </label>
        <label className="text-xs text-[var(--text-muted)]">
          Ingress Mbit/s
          <input className="mt-1 w-full rounded-lg border border-[var(--apple-hairline)] bg-transparent px-2 py-1.5 text-sm" value={ingress} onChange={(e) => setIngress(e.target.value)} />
        </label>
        <label className="text-xs text-[var(--text-muted)]">
          PPS
          <input className="mt-1 w-full rounded-lg border border-[var(--apple-hairline)] bg-transparent px-2 py-1.5 text-sm" value={pps} onChange={(e) => setPps(e.target.value)} />
        </label>
        <button
          type="button"
          className="btn-primary text-sm inline-flex items-center gap-2 disabled:opacity-50"
          disabled={creating}
          onClick={() => void handleCreate()}
        >
          {creating ? <Loader2 className="w-4 h-4 animate-spin" /> : <Plus className="w-4 h-4" />}
          Create
        </button>
      </div>

      <div className="mt-4 flex flex-wrap gap-3 items-end rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4">
        <label className="text-xs text-[var(--text-muted)]">
          Policy
          <select className="mt-1 block rounded-lg border border-[var(--apple-hairline)] bg-transparent px-2 py-1.5 text-sm min-w-[12rem]" value={bindPolicy} onChange={(e) => setBindPolicy(e.target.value)}>
            <option value="">Select…</option>
            {policies.map((p) => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
          </select>
        </label>
        <label className="text-xs text-[var(--text-muted)]">
          VM id
          <input className="mt-1 block rounded-lg border border-[var(--apple-hairline)] bg-transparent px-2 py-1.5 text-sm min-w-[16rem] font-mono" value={bindVm} onChange={(e) => setBindVm(e.target.value)} placeholder="uuid" />
        </label>
        <button type="button" className="px-3 py-2 rounded-lg border border-[var(--apple-hairline)] text-sm" onClick={() => void handleBind()}>
          Bind to VM edge
        </button>
      </div>

      <TahoeToolbar
        search={search}
        onSearchChange={setSearch}
        placeholder="Search name or ID…"
      />
      <TahoeTableWrap>
        <table className="apple-table" aria-label="VM QoS policies">
          <thead>
            <tr>
              <th scope="col">Name</th>
              <th scope="col">Egress</th>
              <th scope="col">Ingress</th>
              <th scope="col">PPS</th>
              <th scope="col" />
            </tr>
          </thead>
          <tbody>
            {filtered.map((p) => (
              <tr key={p.id}>
                <td className="font-medium text-[var(--text-primary)]">{p.name}</td>
                <td>{p.egress_mbps ?? '—'}</td>
                <td>{p.ingress_mbps ?? '—'}</td>
                <td>{p.pps ?? '—'}</td>
                <td className="text-right">
                  <button type="button" className="text-red-600 inline-flex items-center gap-1" onClick={() => setPendingDelete(p)}>
                    <Trash2 className="w-3.5 h-3.5" /> Delete
                  </button>
                </td>
              </tr>
            ))}
            {!loading && filtered.length === 0 && (
              <tr><td colSpan={5} className="py-6 text-[var(--text-muted)]">No QoS policies yet.</td></tr>
            )}
          </tbody>
        </table>
      </TahoeTableWrap>
      <button type="button" className="mt-3 inline-flex items-center gap-2 text-sm text-[var(--text-muted)]" onClick={() => void load()}>
        <RefreshCw className="w-3.5 h-3.5" /> Refresh
      </button>

      <ConfirmDialog
        open={!!pendingDelete}
        title="Delete QoS policy"
        message={`Delete ${pendingDelete?.name}? Bound VMs lose rate limits on next edge sync.`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setPendingDelete(null)}
        onConfirm={async () => {
          if (!pendingDelete) return
          try {
            await deleteVmQosPolicy(pendingDelete.id)
            toast.success('Deleted')
            setPendingDelete(null)
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
