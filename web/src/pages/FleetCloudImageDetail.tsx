// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useParams } from 'react-router'
import { ArrowLeft, HardDrive } from 'lucide-react'
import { listTemplates, type NativeTemplate } from '../api/nativeTemplates'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import ImageSharing from '../components/platform/ImageSharing'
import PageLayout from '../components/PageLayout'
import PageSkeleton from '../components/PageSkeleton'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { useBreadcrumbName } from '../contexts/BreadcrumbNameContext'

// Native golden-image catalog — there's no old external-cloud gate component to
// gate it behind (the daemon's external-cloud-client integration has since
// been fully removed); see FleetCloudImages.tsx.
export default function FleetCloudImageDetailPage() {
  return <FleetCloudImageDetailContent />
}

function FleetCloudImageDetailContent() {
  const { id } = useParams<{ id: string }>()
  const toast = useToastContext()
  const [image, setImage] = useState<NativeTemplate | null>(null)
  const [loading, setLoading] = useState(true)
  useBreadcrumbName(image?.name)
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    if (!id) return
    // Last-response-wins: only the newest load may commit so a stale fetch for a
    // prior image can't overwrite the one now shown.
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setLoading(true)
    try {
      // No single-id lookup route exists on the controller (only get-by-name+version) — load the
      // catalog and find this one by its own id.
      const list = await listTemplates()
      if (!alive()) return
      setImage(list.find((t) => t.id === id) ?? null)
    } catch (e: unknown) {
      if (!alive()) return
      toast.error(formatUserError(e))
      setImage(null)
    } finally {
      if (alive()) setLoading(false)
    }
  }, [id, toast])

  useEffect(() => {
    void load()
  }, [load])

  if (loading) {
    return <PageSkeleton />
  }

  if (!image) {
    return (
      <div className="space-y-4">
        <FleetCloudSubNav />
        <p className="text-[var(--text-muted)]">Image not found.</p>
        <Link to="/fleet-cloud/images" className="text-[var(--accent)] hover:underline">Back to images</Link>
      </div>
    )
  }

  return (
    <PageLayout
      hideHeader
      className="w-full max-w-none"
      prepend={<><FleetCloudSubNav /></>}
    >
      <Link to="/fleet-cloud/images" className="inline-flex items-center gap-2 text-[var(--text-muted)] hover:text-[var(--text-primary)] text-sm">
        <ArrowLeft className="w-4 h-4" /> Images
      </Link>
      <p className="apple-eyebrow">Fleet Cloud</p>
      <h1 className="page-title flex items-center gap-3">
        <HardDrive className="w-7 h-7 text-[var(--accent)]" />
        {image.name}
      </h1>
      <dl className="grid sm:grid-cols-2 gap-4 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 text-sm">
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">ID</dt><dd className="font-mono text-[var(--text-primary)] mt-1 break-all">{image.id}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Status</dt><dd className="text-[var(--text-primary)] mt-1">{image.approval_status}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Version</dt><dd className="text-[var(--text-primary)] mt-1">{image.version}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">OS family</dt><dd className="text-[var(--text-primary)] mt-1">{image.os_family || '—'}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Category</dt><dd className="text-[var(--text-primary)] mt-1">{image.category}</dd></div>
        <div><dt className="text-xs text-[var(--text-muted)] uppercase">Cloud-init</dt><dd className="text-[var(--text-primary)] mt-1">{image.cloud_init ? 'yes' : 'no'}</dd></div>
        <div className="sm:col-span-2"><dt className="text-xs text-[var(--text-muted)] uppercase">Source disk</dt><dd className="font-mono text-[var(--text-primary)] mt-1 break-all">{image.source_disk}</dd></div>
        {image.description && (
          <div className="sm:col-span-2"><dt className="text-xs text-[var(--text-muted)] uppercase">Description</dt><dd className="text-[var(--text-primary)] mt-1">{image.description}</dd></div>
        )}
      </dl>
      <ImageSharing key={image.id} image={image} />
      <FleetCloudFooter />
    </PageLayout>
  )
}
