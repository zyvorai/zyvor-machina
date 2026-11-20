// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { useBreadcrumbName } from '../contexts/BreadcrumbNameContext'
import { Link, useParams } from 'react-router'
import { ArrowLeft, Network } from 'lucide-react'
import { listNetworks, type NativeNetwork } from '../api/nativeNetworks'
import { attachVmNic, detachVmNic, getVm, listVmNics, type NativeVm, type NativeVmNic } from '../api/nativeVms'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import PageSkeleton from '../components/PageSkeleton'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses } from '../utils/semanticColors'

// Native NIC inventory — no old external-cloud gate component in the way any
// more (the daemon's external-cloud-client integration has since been
// fully removed); see FleetCloudInstances.tsx.
export default function FleetCloudInstanceInterfacesPage() {
  return <FleetCloudInstanceInterfacesContent />
}

function FleetCloudInstanceInterfacesContent() {
  const { id } = useParams<{ id: string }>()
  const toast = useToastContext()
  const [inst, setInst] = useState<NativeVm | null>(null)
  const [ifaces, setIfaces] = useState<NativeVmNic[]>([])
  const [networks, setNetworks] = useState<NativeNetwork[]>([])
  const [attachNet, setAttachNet] = useState('')
  const [loading, setLoading] = useState(true)

  useBreadcrumbName(inst?.name)

  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    if (!id) return
    // Last-response-wins: only the newest load may commit so a stale fetch for a
    // prior instance can't interleave into the one now shown.
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setLoading(true)
    try {
      const [instance, ifc, nets] = await Promise.all([
        getVm(id),
        listVmNics(id),
        listNetworks().catch(() => []),
      ])
      if (!alive()) return
      setInst(instance)
      setIfaces(ifc)
      setNetworks(nets)
    } catch (e: unknown) {
      if (!alive()) return
      toast.error(formatUserError(e))
      setInst(null)
    } finally {
      if (alive()) setLoading(false)
    }
  }, [id, toast])

  useEffect(() => { void load() }, [load])

  const run = async (fn: () => Promise<unknown>, ok: string) => {
    try {
      await fn()
      toast.success(ok)
      void load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  if (loading) return <PageSkeleton />
  if (!inst) {
    return (
      <div className="space-y-4">
        <FleetCloudSubNav />
        <Link to="/fleet-cloud/instances" className="text-[var(--accent)] hover:underline">Back</Link>
      </div>
    )
  }

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <Link to={`/fleet-cloud/instances/${inst.id}`} className="inline-flex items-center gap-2 text-[var(--text-muted)] hover:text-[var(--text-primary)] text-sm">
        <ArrowLeft className="w-4 h-4" /> {inst.name}
      </Link>
      <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <Network className="w-7 h-7 text-[var(--accent)]" />
        Network interfaces
      </h1>
      <p className="text-sm text-[var(--text-muted)] font-mono">{inst.id}</p>

      <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3">
        <h2 className="text-sm font-medium text-[var(--text-secondary)]">Attached interfaces ({ifaces.length})</h2>
        {ifaces.length === 0 ? (
          <p className="text-sm text-[var(--text-muted)]">No interfaces attached.</p>
        ) : (
          <ul className="space-y-2 text-sm">
            {ifaces.map((i) => (
              <li key={i.mac_address} className="flex flex-wrap items-center gap-3 rounded-lg border border-[var(--apple-hairline)] px-3 py-2 font-mono">
                <span className="text-[var(--text-primary)]">{i.ip || '—'}</span>
                <span className="text-[var(--text-muted)] text-xs">MAC {i.mac_address}</span>
                <span className="text-[var(--text-muted)] text-xs">{i.network} · {i.model}</span>
                <button type="button" className={statusActionLinkClasses('error', 'text-xs ml-auto')}
                  onClick={() => void run(() => detachVmNic(inst.id, i.mac_address), 'Interface detached')}>
                  Detach
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3">
        <h2 className="text-sm font-medium text-[var(--text-secondary)]">Attach network</h2>
        <div className="flex flex-wrap gap-2 items-end">
          <select value={attachNet} onChange={(e) => setAttachNet(e.target.value)}
            className="input-field text-sm min-w-[14rem]">
            <option value="">Select network…</option>
            {networks.map((n) => (
              <option key={n.id} value={n.name}>{n.name}</option>
            ))}
          </select>
          <button type="button" disabled={!attachNet}
            className="btn-primary text-sm disabled:opacity-40"
            onClick={() => void run(
              () => attachVmNic(inst.id, attachNet),
              'Interface attached',
            )}>Attach NIC</button>
        </div>
      </section>
      <FleetCloudFooter />
    </PageLayout>
  )
}
