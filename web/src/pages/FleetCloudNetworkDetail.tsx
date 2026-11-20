// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { ArrowLeft, Network, Trash2 } from 'lucide-react'
import { deleteNetwork, getNetwork, type NativeNetwork } from '../api/nativeNetworks'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import PageSkeleton from '../components/PageSkeleton'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { statusDestructiveButtonClasses, statusSurfaceClasses, statusToneClass } from '../utils/semanticColors'
import { useBreadcrumbName } from '../contexts/BreadcrumbNameContext'

// Native network detail — no old external-cloud gate component in the way any
// more (the daemon's external-cloud-client integration has since been
// fully removed). No rename (the native networks API has no name-update
// field — vlan_id/bridge/segment_id only).
export default function FleetCloudNetworkDetailPage() {
  return <FleetCloudNetworkDetailContent />
}

function FleetCloudNetworkDetailContent() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const toast = useToastContext()
  const [net, setNet] = useState<NativeNetwork | null>(null)
  const [loading, setLoading] = useState(true)
  const [deleteOpen, setDeleteOpen] = useState(false)
  useBreadcrumbName(net?.name)
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    if (!id) return
    // Last-response-wins: only the newest load may commit so a stale fetch for a
    // prior network can't overwrite the one now shown.
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setLoading(true)
    try {
      const network = await getNetwork(id)
      if (!alive()) return
      setNet(network)
    } catch (e: unknown) {
      if (!alive()) return
      toast.error(formatUserError(e))
      setNet(null)
    } finally {
      if (alive()) setLoading(false)
    }
  }, [id, toast])

  useEffect(() => { void load() }, [load])

  if (loading) return <PageSkeleton />
  if (!net) {
    return (
      <div className="space-y-4">
        <FleetCloudSubNav />
        <p className="text-[var(--text-muted)]">Network not found.</p>
        <Link to="/fleet-cloud/networking" className="text-[var(--accent)] hover:underline">Back</Link>
      </div>
    )
  }

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <Link to="/fleet-cloud/networking" className="inline-flex items-center gap-2 text-[var(--text-muted)] hover:text-[var(--text-primary)] text-sm">
        <ArrowLeft className="w-4 h-4" /> Networking
      </Link>
      <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <Network className="w-7 h-7 text-[var(--accent)]" />
        {net.name}
      </h1>
      <dl className="grid sm:grid-cols-2 gap-4 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 text-sm">
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">ID</dt><dd className="font-mono text-[var(--text-primary)] mt-1 break-all">{net.id}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Backend</dt><dd className="text-[var(--text-primary)] mt-1">{net.backend}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">VLAN</dt><dd className="text-[var(--text-primary)] mt-1">{net.vlan_id ?? '—'}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Bridge</dt><dd className="text-[var(--text-primary)] mt-1">{net.bridge ?? '—'}</dd></div>
      </dl>
      <button type="button" className={statusDestructiveButtonClasses('text-sm inline-flex items-center gap-1')}
        onClick={() => setDeleteOpen(true)}>
        <Trash2 className="w-4 h-4" /> Delete
      </button>
      {deleteOpen && (
        <div className={`rounded-xl p-4 space-y-3 ${statusSurfaceClasses('error')}`}>
          <p className={`text-sm ${statusToneClass('error')}`}>Delete network <span className="font-mono">{net.name}</span>? Ports on it must be removed first.</p>
          <div className="flex gap-2">
            <button type="button" className="btn-destructive text-sm"
              onClick={async () => {
                try {
                  await deleteNetwork(net.id)
                  toast.success('Network deleted')
                  navigate('/fleet-cloud/networking')
                } catch (e: unknown) {
                  toast.error(formatUserError(e))
                  setDeleteOpen(false)
                }
              }}>Delete</button>
            <button type="button" className="btn-secondary text-sm"
              onClick={() => setDeleteOpen(false)}>Cancel</button>
          </div>
        </div>
      )}
      <FleetCloudFooter />
    </PageLayout>
  )
}
