// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { ArrowLeft, Loader2, Plus, Scale, Trash2 } from 'lucide-react'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import PageSkeleton from '../components/PageSkeleton'
import ConfirmDialog from '../components/ConfirmDialog'
import { listVms, type NativeVm } from '../api/nativeVms'
import {
  addLbMember,
  deleteLbMember,
  deleteLoadBalancer,
  getLoadBalancer,
  listLbMembers,
  patchLbMember,
  type NativeLbMember,
  type NativeLoadBalancer,
} from '../api/nativeLoadBalancers'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses, statusToneClass } from '../utils/semanticColors'
import { useBreadcrumbName } from '../contexts/BreadcrumbNameContext'

// Native L4 load balancer — like the other rewired /fleet-cloud/* pages, this
// no longer depends on a wired external cloud. There's no
// the old external-cloud gate component wrapping it any more either: the daemon's
// external-cloud-client integration has since been fully removed.
export default function FleetCloudLoadBalancerDetailPage() {
  return <FleetCloudLoadBalancerDetailContent />
}

function FleetCloudLoadBalancerDetailContent() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const toast = useToastContext()
  const [lb, setLb] = useState<NativeLoadBalancer | null>(null)
  const [members, setMembers] = useState<NativeLbMember[]>([])
  const [vms, setVms] = useState<NativeVm[]>([])
  const [loading, setLoading] = useState(true)

  const [memberVmId, setMemberVmId] = useState('')
  const [memberPort, setMemberPort] = useState('80')
  const [memberWeight, setMemberWeight] = useState('1')
  const [confirmDelete, setConfirmDelete] = useState(false)
  useBreadcrumbName(lb?.name)

  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    if (!id) return
    // Last-response-wins: only the newest load may commit so a stale fetch for a
    // prior load balancer can't interleave into the one now shown.
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setLoading(true)
    try {
      const [lbR, memberR, vmR] = await Promise.all([
        getLoadBalancer(id),
        listLbMembers(id),
        listVms().catch(() => []),
      ])
      if (!alive()) return
      setLb(lbR)
      setMembers(memberR)
      setVms(vmR)
      if (vmR.length > 0) setMemberVmId((prev) => prev || vmR[0].id)
    } catch (e: unknown) {
      if (!alive()) return
      toast.error(formatUserError(e))
      setLb(null)
    } finally {
      if (alive()) setLoading(false)
    }
  }, [id, toast])

  useEffect(() => { void load() }, [load])

  if (loading) return <PageSkeleton />
  if (!lb || !id) {
    return (
      <div className="space-y-4">
        <FleetCloudSubNav />
        <Link to="/fleet-cloud/load-balancers" className="text-[var(--accent)] hover:underline">Back</Link>
      </div>
    )
  }

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <Link to="/fleet-cloud/load-balancers" className="inline-flex items-center gap-2 text-[var(--text-muted)] hover:text-[var(--text-primary)] text-sm">
        <ArrowLeft className="w-4 h-4" /> Load balancers
      </Link>
      <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <Scale className={`w-7 h-7 ${statusToneClass('ok')}`} /> {lb.name}
      </h1>
      <dl className="grid sm:grid-cols-2 gap-4 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 text-sm">
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Listener</dt><dd className="font-mono mt-1">{lb.protocol}/{lb.listener_port}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Status</dt><dd className="mt-1">{lb.status}</dd></div>
        {lb.status_message && (
          <div className="sm:col-span-2"><dt className="text-xs text-[var(--text-muted)] uppercase">Status detail</dt><dd className="mt-1 text-amber-600">{lb.status_message}</dd></div>
        )}
      </dl>

      <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3">
        <h2 className="text-sm font-medium text-[var(--text-secondary)]">Members</h2>
        <p className="text-xs text-[var(--text-muted)]">
          Traffic to {lb.protocol}/{lb.listener_port} on this host is split across enabled members by weight
          (kernel-level, weighted-random DNAT — no active health checks yet; disable a member manually to pull it
          out of rotation).
        </p>
        <div className="flex flex-wrap gap-2 items-end">
          <div>
            <label className="block text-xs text-[var(--text-muted)] mb-1">VM</label>
            <select aria-label="Member VM" value={memberVmId} onChange={(e) => setMemberVmId(e.target.value)}
              className="input-field text-sm min-w-[12rem]">
              <option value="">VM…</option>
              {vms.map((v) => <option key={v.id} value={v.id}>{v.name}{v.guest_ip ? ` (${v.guest_ip})` : ''}</option>)}
            </select>
          </div>
          <div>
            <label className="block text-xs text-[var(--text-muted)] mb-1">Port</label>
            <input aria-label="Member port" value={memberPort} onChange={(e) => setMemberPort(e.target.value)}
              className="w-20 input-field text-sm" />
          </div>
          <div>
            <label className="block text-xs text-[var(--text-muted)] mb-1">Weight</label>
            <input aria-label="Member weight" value={memberWeight} onChange={(e) => setMemberWeight(e.target.value)}
              className="w-16 input-field text-sm" />
          </div>
          <button type="button" className="btn-primary text-sm inline-flex items-center gap-1"
            disabled={!memberVmId || !Number(memberPort)}
            onClick={async () => {
              try {
                await addLbMember(id, {
                  vm_id: memberVmId,
                  port: Number(memberPort) || 80,
                  weight: Number(memberWeight) || 1,
                })
                toast.success('Member added')
                void load()
              } catch (e: unknown) { toast.error(formatUserError(e)) }
            }}>
            <Plus className="w-3.5 h-3.5" /> Add member
          </button>
        </div>

        {members.length === 0 ? (
          <p className="text-sm text-[var(--text-muted)]">No members yet — traffic to the listener port is dropped until at least one is added.</p>
        ) : (
          <div className="overflow-x-auto apple-surface rounded-2xl">
            <table className="apple-table" aria-label="Members">
              <thead className="bg-[var(--apple-surface)] text-[var(--text-muted)] text-left">
                <tr>
                  <th scope="col" className="px-3 py-2">VM</th>
                  <th scope="col" className="px-3 py-2">Address</th>
                  <th scope="col" className="px-3 py-2">Weight</th>
                  <th scope="col" className="px-3 py-2">Enabled</th>
                  <th scope="col" className="px-3 py-2" />
                </tr>
              </thead>
              <tbody>
                {members.map((m) => (
                  <tr key={m.id} className="border-t border-[var(--apple-hairline)]">
                    <td className="px-3 py-2">{m.vm_name}</td>
                    <td className="px-3 py-2 font-mono">{m.vm_ip ? `${m.vm_ip}:${m.port}` : <span className="text-amber-400">no guest IP yet</span>}</td>
                    <td className="px-3 py-2">{m.weight}</td>
                    <td className="px-3 py-2">
                      <button type="button" className={statusActionLinkClasses(m.enabled ? 'ok' : 'neutral', 'text-xs')}
                        onClick={async () => {
                          try {
                            await patchLbMember(id, m.id, { enabled: !m.enabled })
                            void load()
                          } catch (e: unknown) { toast.error(formatUserError(e)) }
                        }}>
                        {m.enabled ? 'Enabled' : 'Disabled'}
                      </button>
                    </td>
                    <td className="px-3 py-2 text-right">
                      <button type="button" aria-label={`Remove ${m.vm_name}`} className={statusActionLinkClasses('error', 'text-xs')}
                        onClick={async () => {
                          try {
                            await deleteLbMember(id, m.id)
                            toast.success('Member removed')
                            void load()
                          } catch (e: unknown) { toast.error(formatUserError(e)) }
                        }}>Remove</button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <button type="button" className="px-3 py-1.5 rounded-lg border border-red-600/50 text-red-600 text-sm inline-flex items-center gap-1"
        onClick={() => setConfirmDelete(true)}>
        <Trash2 className="w-4 h-4" /> Delete load balancer
      </button>
      <ConfirmDialog
        open={confirmDelete}
        title="Delete load balancer"
        message={`Delete ${lb.name}?`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setConfirmDelete(false)}
        onConfirm={async () => {
          setConfirmDelete(false)
          try {
            await deleteLoadBalancer(lb.id)
            toast.success('Deleted')
            navigate('/fleet-cloud/load-balancers')
          } catch (e: unknown) { toast.error(formatUserError(e)) }
        }}
      />
      <FleetCloudFooter />
    </PageLayout>
  )
}
