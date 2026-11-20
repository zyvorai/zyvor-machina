// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { createTemplate, deleteTemplate, listTemplates, type NativeTemplate } from '../api/nativeTemplates'
import { useToastContext } from '../contexts/ToastContext'
import ConfirmDialog from '../components/ConfirmDialog'
import FleetCloudFooter from '../components/FleetCloudFooter'
import { Cloud, RefreshCw, Plus, Trash2 } from 'lucide-react'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import PageLayout from '../components/PageLayout'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { formatUserError } from '../utils/apiError'
import { statusToneClass } from '../utils/semanticColors'
import { platformStatSubtitle } from '../components/platform/PlatformPageChrome'

// Native golden-image catalog — no old external-cloud gate component in the way
// any more (the daemon's external-cloud-client integration has since been
// fully removed). See api/nativeTemplates.ts: unlike Glance there's no
// byte-upload here (register an existing on-host disk path, or publish one
// from a running VM).
export default function FleetCloudImagesPage() {
  return <FleetCloudImagesContent />
}

function FleetCloudImagesContent() {
  const [images, setImages] = useState<NativeTemplate[]>([])
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<NativeTemplate | null>(null)
  const [deleting, setDeleting] = useState(false)
  const [name, setName] = useState('')
  const [version, setVersion] = useState('1.0')
  const [sourceDisk, setSourceDisk] = useState('')
  const [creating, setCreating] = useState(false)
  const [search, setSearch] = useState('')
  const toast = useToastContext()

  const load = useCallback(async () => {
    try {
      setLoadError(null)
      const list = await listTemplates()
      setImages(list)
    } catch (e: unknown) {
      const msg = formatUserError(e)
      setLoadError(msg)
      toast.error(`Failed to load images: ${msg}`)
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => { void load() }, [load])

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return images
    return images.filter(
      (img) =>
        img.name.toLowerCase().includes(q) ||
        img.id.toLowerCase().includes(q) ||
        img.source_disk.toLowerCase().includes(q),
    )
  }, [images, search])

  const handleDelete = async () => {
    if (!deleteTarget) return
    setDeleting(true)
    try {
      await deleteTemplate(deleteTarget.name, deleteTarget.version)
      toast.success(`Deleted image '${deleteTarget.name}'`)
      setDeleteTarget(null)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setDeleting(false)
    }
  }

  return (
    <PageLayout
      eyebrow="Fleet Cloud"
      prepend={<><FleetCloudSubNav /></>}
      title="Images"
      subtitle={platformStatSubtitle([{ label: 'Images', value: images.length }])}
      icon={<Cloud className="w-7 h-7 text-[var(--accent)]" />}
      error={loadError}
      errorTitle="Failed to load images"
      technicalDetail={loadError}
      errorTone="red"
      onErrorRetry={() => void load()}
      onErrorDismiss={() => setLoadError(null)}
      actions={
        <div className="flex gap-2 flex-wrap">
          <Link
            to="/disk-images"
            className="btn-secondary text-sm inline-flex items-center gap-2"
          >
            Manage disk images
          </Link>
          <Link
            to="/fleet-cloud/create"
            className="btn-primary text-sm inline-flex items-center gap-2"
          >
            <Plus className="w-4 h-4" />
            Boot instance
          </Link>
          <button
            type="button"
            onClick={() => { setLoading(true); void load() }}
            className="btn-secondary text-sm inline-flex items-center gap-2"
          >
            <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
            Refresh
          </button>
        </div>
      }
    >
      <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3">
        <h2 className="text-sm font-medium text-[var(--text-secondary)] flex items-center gap-2"><Plus className="w-4 h-4" /> Register image</h2>
        <div className="grid sm:grid-cols-3 gap-3">
          <input aria-label="Image name" value={name} onChange={(e) => setName(e.target.value)} placeholder="Name"
            className="input-field text-sm" />
          <input aria-label="Version" value={version} onChange={(e) => setVersion(e.target.value)} placeholder="Version"
            className="input-field text-sm" />
          <input aria-label="Source disk path" value={sourceDisk} onChange={(e) => setSourceDisk(e.target.value)} placeholder="/path/to/golden.qcow2"
            className="input-field text-sm font-mono" />
        </div>
        <button type="button" disabled={creating || !name.trim() || !sourceDisk.trim()}
          className="btn-primary text-sm disabled:opacity-40"
          onClick={async () => {
            setCreating(true)
            try {
              await createTemplate({ name: name.trim(), version: version.trim() || '1.0', source_disk: sourceDisk.trim() })
              toast.success('Image registered')
              setName(''); setSourceDisk('')
              void load()
            } catch (e: unknown) { toast.error(formatUserError(e)) } finally { setCreating(false) }
          }}>{creating ? 'Registering…' : 'Register'}</button>
      </div>

      <TahoeToolbar
        search={search}
        onSearchChange={setSearch}
        placeholder="Search name, ID, or source…"
      />

      <TahoeTableWrap>
        <table className="apple-table" aria-label="Images">
          <thead>
            <tr>
              <th scope="col">Name</th>
              <th scope="col">Version</th>
              <th scope="col">Status</th>
              <th scope="col">Source</th>
              <th scope="col" className="w-16" />
            </tr>
          </thead>
          <tbody>
            {loading && filtered.length === 0 && (
              <tr><td colSpan={5} className="text-center text-[var(--text-muted)]">Loading…</td></tr>
            )}
            {!loading && filtered.length === 0 && (
              <tr><td colSpan={5} className="text-center text-[var(--text-muted)]">
                {search.trim() ? 'No images match your search.' : 'No images found.'}
              </td></tr>
            )}
            {filtered.map((img) => (
              <tr key={img.id}>
                <td>
                  <Link to={`/fleet-cloud/images/${img.id}`} className="apple-link font-medium">
                    {img.name}
                  </Link>
                  <div className="text-xs text-[var(--text-muted)] font-mono truncate max-w-xs">{img.id}</div>
                </td>
                <td className="text-[var(--text-secondary)]">{img.version}</td>
                <td className="text-[var(--text-secondary)]">{img.approval_status}</td>
                <td className="text-[var(--text-muted)] font-mono text-xs truncate max-w-xs">{img.source_disk}</td>
                <td>
                  <button
                    type="button"
                    title="Delete image"
                    onClick={() => setDeleteTarget(img)}
                    className={`p-2 rounded hover:bg-[color-mix(in_srgb,var(--machina-status-error)_25%,transparent)] ${statusToneClass('error')}`}
                  >
                    <Trash2 className="w-4 h-4" />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </TahoeTableWrap>

      <FleetCloudFooter />

      <ConfirmDialog
        open={!!deleteTarget}
        title="Delete image"
        message={`Permanently delete ${deleteTarget?.name} from the image catalog?`}
        confirmLabel={deleting ? 'Deleting…' : 'Delete'}
        variant="danger"
        onConfirm={handleDelete}
        onCancel={() => setDeleteTarget(null)}
      />
    </PageLayout>
  )
}
