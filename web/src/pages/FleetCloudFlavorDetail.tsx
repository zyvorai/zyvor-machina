// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useParams } from 'react-router'
import { ArrowLeft, Cpu, Loader2 } from 'lucide-react'
import { getFlavor, type NativeFlavor } from '../api/flavors'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import PageSkeleton from '../components/PageSkeleton'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { useBreadcrumbName } from '../contexts/BreadcrumbNameContext'

// Native flavor catalog — there's no old external-cloud gate component in the
// way (the daemon's external-cloud-client integration has since been
// fully removed); see FleetCloudFlavors.tsx.
export default function FleetCloudFlavorDetailPage() {
  return <FleetCloudFlavorDetailContent />
}

function FleetCloudFlavorDetailContent() {
  const { id } = useParams<{ id: string }>()
  const toast = useToastContext()
  const [flavor, setFlavor] = useState<NativeFlavor | null>(null)
  const [loading, setLoading] = useState(true)
  useBreadcrumbName(flavor?.name)
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    if (!id) return
    // Last-response-wins: only the newest load may commit so a stale fetch for a
    // prior flavor can't overwrite the one now shown.
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setLoading(true)
    try {
      const f = await getFlavor(id)
      if (!alive()) return
      setFlavor(f)
    } catch (e: unknown) {
      if (!alive()) return
      toast.error(formatUserError(e))
      setFlavor(null)
    } finally {
      if (alive()) setLoading(false)
    }
  }, [id, toast])

  useEffect(() => { void load() }, [load])

  if (loading) return <PageSkeleton />
  if (!flavor) {
    return (
      <div className="space-y-4">
        <FleetCloudSubNav />
        <Link to="/fleet-cloud/flavors" className="text-[var(--accent)] hover:underline">Back</Link>
      </div>
    )
  }

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <Link to="/fleet-cloud/flavors" className="inline-flex items-center gap-2 text-[var(--text-muted)] hover:text-[var(--text-primary)] text-sm">
        <ArrowLeft className="w-4 h-4" /> Flavors
      </Link>
      <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <Cpu className="w-7 h-7 text-[var(--accent)]" />
        {flavor.name}
      </h1>
      <dl className="grid sm:grid-cols-2 gap-4 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 text-sm font-mono">
        <div><dt className="text-xs text-[var(--text-muted)] uppercase font-sans">ID</dt><dd className="text-[var(--text-primary)] mt-1">{flavor.id}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase font-sans">vCPU</dt><dd className="text-[var(--text-primary)] mt-1">{flavor.vcpus}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase font-sans">RAM</dt><dd className="text-[var(--text-primary)] mt-1">{flavor.memory_mib} MiB</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase font-sans">Disk</dt><dd className="text-[var(--text-primary)] mt-1">{flavor.disk_gib} GiB</dd></div>
      </dl>
      <Link to="/fleet-cloud/create" className="btn-primary text-sm">Create instance with this flavor</Link>
      <FleetCloudFooter />
    </PageLayout>
  )
}
