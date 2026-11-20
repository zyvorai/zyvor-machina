// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { useSearchParams } from 'react-router'
import { Boxes, ClipboardList, FolderOpen, HardDrive, RefreshCw, Trash2 } from 'lucide-react'
import PageLayout from '../components/PageLayout'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import KubeVirtQcow2Modal from '../components/KubeVirtQcow2Modal'
import { usePlatformInfo } from '../contexts/PlatformInfoContext'
import EmptyState from '../components/EmptyState'
import {
  buildVirtImageDisk,
  deleteDiskImage,
  getVirtBuilderNotes,
  getVirtImageOutputRoots,
  ImageFile,
  listDiskImages,
  listMkosiWorkspaces,
  listVirtBuilderTemplates,
  probeVirtBuilderTemplate,
  type MkosiWorkspace,
  VirtBuilderListResponse,
} from '../api/extras'
import { startVirtImageBuildJob, streamJobLogs } from '../api/jobs'
import { BrowseHostPathModal } from '../components/BrowseHostPathModal'
import { BuildStepTimeline } from '../components/BuildStepTimeline'
import ConfirmDialog from '../components/ConfirmDialog'
import { useToastContext } from '../contexts/ToastContext'
import { computeVirtImageBuildTimeline, VIRT_IMAGE_TIMELINE_LABELS } from '../utils/buildProgress'
import { formatUserError } from '../utils/apiError'
import { libvirtErrorHints } from '../utils/libvirtHints'
import { statusDestructiveButtonClasses, statusToneClass } from '../utils/semanticColors'

function formatBytes(b: number): string {
  if (b === 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(b) / Math.log(1024))
  return `${(b / Math.pow(1024, i)).toFixed(1)} ${units[i]}`
}

function suggestQcow2Path(parentDir: string, templateName: string): string {
  const base = parentDir.replace(/\/+$/, '')
  const safe = templateName.replace(/[^a-zA-Z0-9._-]+/g, '-').slice(0, 48)
  return `${base}/machina-vb-${safe}-${Date.now().toString(36)}.qcow2`
}

export default function DiskImagesPage() {
  const [images, setImages] = useState<ImageFile[]>([])
  const [scanDirectories, setScanDirectories] = useState<string[]>([])
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [deleting, setDeleting] = useState<string | null>(null)
  const [confirmPath, setConfirmPath] = useState<string | null>(null)
  const [vbCatalog, setVbCatalog] = useState<VirtBuilderListResponse | null>(null)
  const [outputRoots, setOutputRoots] = useState<{ allowed_prefixes: string[]; effective_tmpdir: string } | null>(
    null,
  )

  const [vbOs, setVbOs] = useState('')
  const [vbOutput, setVbOutput] = useState('')
  const [vbBuilding, setVbBuilding] = useState(false)
  const [vbLog, setVbLog] = useState<string[]>([])
  const [vbOk, setVbOk] = useState(false)
  const [vbFailed, setVbFailed] = useState(false)
  const [outBrowseOpen, setOutBrowseOpen] = useState(false)
  const [vbProbe, setVbProbe] = useState<{ name_valid: boolean; in_cached_catalog: boolean; hint?: string | null } | null>(null)
  const [vbProbeBusy, setVbProbeBusy] = useState(false)
  const [vbNotes, setVbNotes] = useState<string | null>(null)
  const [vbNotesBusy, setVbNotesBusy] = useState(false)
  const [mkosiWorkspaces, setMkosiWorkspaces] = useState<MkosiWorkspace[]>([])
  const [directBuildBusy, setDirectBuildBusy] = useState(false)
  const [kvPath, setKvPath] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [searchParams, setSearchParams] = useSearchParams()

  const toast = useToastContext()
  const { info, lastEvent, refreshKey } = usePlatformInfo()

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setLoadError(null)
      const [r, vb, roots, mkosi] = await Promise.all([
        listDiskImages(),
        listVirtBuilderTemplates().catch(() => null),
        getVirtImageOutputRoots().catch(() => null),
        listMkosiWorkspaces().catch(() => [] as MkosiWorkspace[]),
      ])
      setImages(r.files)
      setScanDirectories(r.scan_directories)
      setVbCatalog(vb)
      setOutputRoots(roots)
      setMkosiWorkspaces(mkosi)
    } catch (e: unknown) {
      const msg = formatUserError(e)
      setLoadError(msg)
      toast.error(`Failed to load disk images: ${msg}`)
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => {
    void load()
  }, [load])

  useEffect(() => {
    if (!lastEvent) return
    if (lastEvent.kind.startsWith('kubevirt.qcow2.')) void load()
  }, [refreshKey, lastEvent, load])

  useEffect(() => {
    if (searchParams.get('kv') !== 'open') return
    const path = searchParams.get('path')
    if (path) {
      setKvPath(path)
    } else if (images.length > 0) {
      const first = images.find((i) => i.format === 'qcow2' || i.path.toLowerCase().endsWith('.qcow2'))
      if (first) setKvPath(first.path)
    }
    const next = new URLSearchParams(searchParams)
    next.delete('kv')
    next.delete('path')
    setSearchParams(next, { replace: true })
  }, [searchParams, images, setSearchParams])

  useEffect(() => {
    if (!vbCatalog) return
    setVbOs((prev) => {
      if (prev) return prev
      const first = vbCatalog.items[0]?.name ?? vbCatalog.templates[0]
      return first ?? ''
    })
  }, [vbCatalog])

  const templateOptions = useMemo(() => {
    if (!vbCatalog) return []
    const fromItems = (vbCatalog.items ?? []).map((i) => i.name)
    if (fromItems.length) return [...fromItems].sort((a, b) => a.localeCompare(b))
    return [...vbCatalog.templates].sort((a, b) => a.localeCompare(b))
  }, [vbCatalog])

  const vbAllowed = Boolean(vbCatalog && vbCatalog.virt_builder_allowed !== false)
  const vbReady = Boolean(vbCatalog && vbAllowed && vbCatalog.virt_builder_installed !== false && templateOptions.length)

  const vibTimeline = useMemo(() => {
    if (!vbBuilding && vbLog.length === 0) return null
    const status = vbBuilding ? 'running' : vbOk ? 'completed' : vbFailed ? 'failed' : 'running'
    return computeVirtImageBuildTimeline(vbLog, status)
  }, [vbBuilding, vbLog, vbOk, vbFailed])

  const runProbe = async () => {
    const os = vbOs.trim()
    if (!os) return
    setVbProbeBusy(true)
    setVbProbe(null)
    try {
      setVbProbe(await probeVirtBuilderTemplate(os))
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setVbProbeBusy(false)
    }
  }

  const loadNotes = async () => {
    const os = vbOs.trim()
    if (!os) return
    setVbNotesBusy(true)
    setVbNotes(null)
    try {
      const r = await getVirtBuilderNotes(os)
      setVbNotes(r.notes)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setVbNotesBusy(false)
    }
  }

  const runDirectBuild = async () => {
    const os = vbOs.trim()
    const output = vbOutput.trim()
    if (!os || !output.startsWith('/')) return
    setDirectBuildBusy(true)
    try {
      const r = await buildVirtImageDisk({ os, output })
      toast.success(`Direct build finished: ${r.path}`)
      void load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setDirectBuildBusy(false)
    }
  }

  const runVirtImageBuild = async () => {
    const os = vbOs.trim()
    const output = vbOutput.trim()
    if (!os) {
      toast.warning('Choose a virt-builder template (OS)')
      return
    }
    if (!output.startsWith('/')) {
      toast.warning('Output path must be an absolute path on the hypervisor')
      return
    }
    if (!output.toLowerCase().endsWith('.qcow2')) {
      toast.warning('Output should be a new .qcow2 path (file must not exist yet)')
      return
    }
    setVbOk(false)
    setVbFailed(false)
    setVbBuilding(true)
    setVbLog([`[machina] Starting virt-image-build job for template “${os}”…`])
    try {
      const { id } = await startVirtImageBuildJob({ os, output })
      setVbLog((prev) => [...prev, `[machina] Job ${id} — live output (also under Jobs):`])
      await streamJobLogs(id, {
        onLogChunk: (chunk) => {
          const lines = chunk.split('\n').filter((l) => l.length > 0)
          if (lines.length) setVbLog((prev) => [...prev, ...lines])
        },
        onComplete: () => {
          setVbOk(true)
          setVbBuilding(false)
          toast.success('Disk image build finished — refresh the list or open Jobs for the path.')
          void load()
        },
        onError: (msg) => {
          setVbFailed(true)
          setVbBuilding(false)
          toast.error(msg)
        },
      })
    } catch (e: unknown) {
      setVbFailed(true)
      setVbBuilding(false)
      toast.error(formatUserError(e))
    }
  }

  const handleDelete = async () => {
    if (!confirmPath) return
    setDeleting(confirmPath)
    setConfirmPath(null)
    try {
      await deleteDiskImage(confirmPath)
      toast.success(`Deleted ${confirmPath.split('/').pop()}`)
      setImages((prev) => prev.filter((i) => i.path !== confirmPath))
    } catch (e: unknown) {
      toast.error(`Delete failed: ${formatUserError(e)}`)
    } finally {
      setDeleting(null)
    }
  }

  const totalBytes = images.reduce((s, i) => s + i.size_bytes, 0)

  const filteredImages = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return images
    return images.filter(
      (img) =>
        img.name.toLowerCase().includes(q)
        || img.path.toLowerCase().includes(q)
        || img.format.toLowerCase().includes(q),
    )
  }, [images, search])

  return (
    <PageLayout
      eyebrow="Hypervisor"
      title="Disk Images"
      subtitle={
        loading
          ? 'Scanning hypervisor pools and defaults…'
          : `${images.length} image${images.length !== 1 ? 's' : ''} · ${formatBytes(totalBytes)} total — ISOs, qcow2, and templates visible on this hypervisor host.`
      }
      icon={<HardDrive className="w-6 h-6" />}
      loading={loading && images.length === 0 && !loadError}
      actions={
        <button
          onClick={() => void load()}
          className="p-2 hover:bg-[var(--surface-hover)] rounded transition"
          title="Refresh"
          aria-label="Refresh"
        >
          <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
        </button>
      }
      error={loadError}
      errorTitle="Failed to load disk images"
      errorHints={loadError ? libvirtErrorHints(loadError) : undefined}
      technicalDetail={loadError}
      errorTone="red"
      onErrorRetry={() => void load()}
      onErrorDismiss={() => setLoadError(null)}
      contentClassName="space-y-6"
    >
      {!loading && scanDirectories.length > 0 && (
        <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 bg-[var(--apple-fill-tertiary)] px-4 py-3 text-xs text-[var(--text-muted)] space-y-2">
          <p>
            Scanned directories (libvirt storage pools plus host defaults):{' '}
            <span className="text-[var(--text-secondary)] font-mono break-all">{scanDirectories.join(', ')}</span>
          </p>
          <p className={`border-t pt-2 mt-2 ${statusToneClass('warn')}`}>
            <strong className="opacity-90">mkosi temp:</strong> failed image builds may leave large folders under{' '}
            <code className="opacity-80">/var/tmp/machina-mkosi-ws/</code>. Remove stale ones when you no longer
            need logs to free host space (successful builds clean up unless{' '}
            <code className="opacity-80">MACHINA_MKOSI_KEEP_WORKSPACE</code> is set).
          </p>
        </div>
      )}

      {!loading && (
        <div className="rounded-xl border border-[var(--accent)]/40 bg-[var(--apple-surface)] p-6 space-y-4">
          <div className="flex flex-col gap-1 sm:flex-row sm:items-start sm:justify-between">
            <div>
              <h2 className="text-lg font-semibold text-[var(--text-primary)] flex items-center gap-2">
                <ClipboardList className="w-5 h-5 text-[var(--accent)]" aria-hidden />
                Build disk (virt-image-build)
              </h2>
              <p className="text-sm text-[var(--text-muted)] max-w-3xl mt-1">
                Runs <code className="text-[var(--text-secondary)]">virt-builder</code> on the daemon host as a background job — same
                log stream as <Link to="/jobs" className="text-[var(--accent)] hover:underline">Jobs</Link> and the step
                timeline used on Create VM.
              </p>
            </div>
            <Link
              to="/jobs"
              className="inline-flex shrink-0 items-center gap-1.5 rounded-lg border border-[var(--apple-hairline)] px-3 py-2 text-sm text-[var(--text-primary)] hover:bg-[var(--apple-fill-tertiary)]"
            >
              <ClipboardList className="w-4 h-4" />
              Open Jobs
            </Link>
          </div>

          {!vbCatalog ? (
            <p className="text-sm text-[var(--text-muted)]">
              Could not load virt-builder catalog (daemon unreachable or browse API error).
            </p>
          ) : (
            <>
          {!vbAllowed ? (
            <p className={`text-sm ${statusToneClass('warn')}`}>
              virt-builder is disabled in daemon config (<code className="text-[var(--text-secondary)]">virt_builder_allowed</code>).
            </p>
          ) : vbCatalog.virt_builder_installed === false ? (
            <p className={`text-sm ${statusToneClass('warn')}`}>
              <code className="text-[var(--text-secondary)]">virt-builder</code> is not available on this host (install libguestfs
              tools).
            </p>
          ) : vbCatalog.catalog_error ? (
            <p className="text-sm text-rose-600/90">Catalog: {vbCatalog.catalog_error}</p>
          ) : templateOptions.length === 0 ? (
            <p className="text-sm text-[var(--text-muted)]">No virt-builder templates returned from the host.</p>
          ) : (
            <>
              {outputRoots?.allowed_prefixes?.length ? (
                <p className="text-xs text-[var(--text-muted)]">
                  Allowed output prefixes:{' '}
                  <span className="font-mono text-[var(--text-muted)] break-all">{outputRoots.allowed_prefixes.join(', ')}</span>
                  {outputRoots.effective_tmpdir ? (
                    <>
                      {' '}
                      · TMPDIR: <span className="font-mono text-[var(--text-muted)]">{outputRoots.effective_tmpdir}</span>
                    </>
                  ) : null}
                </p>
              ) : null}
              <div className="grid gap-4 sm:grid-cols-2">
                <div>
                  <label htmlFor="vb-os" className="block text-sm text-[var(--text-muted)] mb-1">
                    Template (OS)
                  </label>
                  <select
                    id="vb-os"
                    value={vbOs}
                    onChange={(e) => setVbOs(e.target.value)}
                    disabled={vbBuilding}
                    className="input-field w-full"
                  >
                    {templateOptions.map((name) => (
                      <option key={name} value={name}>
                        {name}
                      </option>
                    ))}
                  </select>
                </div>
                <div>
                  <label htmlFor="vb-out" className="block text-sm text-[var(--text-muted)] mb-1">
                    New qcow2 path (absolute, must not exist)
                  </label>
                  <div className="flex gap-2">
                    <input
                      id="vb-out"
                      type="text"
                      value={vbOutput}
                      onChange={(e) => setVbOutput(e.target.value)}
                      disabled={vbBuilding}
                      className="input-field flex-1 font-mono text-sm"
                      placeholder="/var/lib/libvirt/images/my-new-disk.qcow2"
                    />
                    <button
                      type="button"
                      disabled={vbBuilding}
                      onClick={() => setOutBrowseOpen(true)}
                      className="shrink-0 inline-flex items-center gap-1.5 rounded-lg border border-[var(--apple-hairline)] px-3 py-2 text-sm text-[var(--text-primary)] hover:bg-[var(--apple-fill-tertiary)] disabled:opacity-50"
                      title="Pick a folder, then we fill a suggested filename"
                    >
                      <FolderOpen className="w-4 h-4" />
                      Folder
                    </button>
                  </div>
                </div>
              </div>
              <div className="flex flex-wrap gap-2">
                <button
                  type="button"
                  disabled={!vbReady || vbBuilding || directBuildBusy}
                  onClick={() => void runVirtImageBuild()}
                  className="btn-primary text-sm disabled:opacity-50"
                >
                  {vbBuilding ? 'Building…' : 'Start disk build job'}
                </button>
                <button
                  type="button"
                  disabled={!vbReady || vbBuilding || directBuildBusy || !vbOutput.trim()}
                  data-testid="virt-direct-build"
                  onClick={() => void runDirectBuild()}
                  className="inline-flex items-center rounded-lg border border-[var(--accent)]/40 px-4 py-2.5 text-sm text-[var(--accent)] hover:bg-[var(--accent-soft)] disabled:opacity-50"
                >
                  {directBuildBusy ? 'Building…' : 'Direct build (sync)'}
                </button>
                <button
                  type="button"
                  disabled={!vbOs.trim() || vbProbeBusy}
                  data-testid="virt-probe-template"
                  onClick={() => void runProbe()}
                  className="inline-flex items-center rounded-lg border border-[var(--apple-hairline)] px-4 py-2.5 text-sm text-[var(--text-primary)] hover:bg-[var(--apple-fill-tertiary)] disabled:opacity-50"
                >
                  {vbProbeBusy ? 'Probing…' : 'Probe template'}
                </button>
                <button
                  type="button"
                  disabled={!vbOs.trim() || vbNotesBusy}
                  data-testid="virt-template-notes"
                  onClick={() => void loadNotes()}
                  className="inline-flex items-center rounded-lg border border-[var(--apple-hairline)] px-4 py-2.5 text-sm text-[var(--text-primary)] hover:bg-[var(--apple-fill-tertiary)] disabled:opacity-50"
                >
                  {vbNotesBusy ? 'Loading…' : 'Template notes'}
                </button>
              </div>
              {vbProbe && (
                <p className="text-xs text-[var(--text-muted)]" data-testid="virt-probe-result">
                  Probe: {vbProbe.name_valid ? 'valid name' : 'invalid name'}
                  {vbProbe.in_cached_catalog ? ' · in catalog' : ' · not in catalog'}
                  {vbProbe.hint ? ` — ${vbProbe.hint}` : ''}
                </p>
              )}
              {vbNotes && (
                <pre className="text-xs text-[var(--text-secondary)] whitespace-pre-wrap rounded border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 p-3" data-testid="virt-notes-body">{vbNotes}</pre>
              )}
            </>
          )}

          {(vbBuilding || vbLog.length > 0) && vibTimeline && (
            <div className="space-y-2 rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-3">
              <h3 className="text-xs font-semibold uppercase tracking-wide text-[var(--accent)]">Build progress</h3>
              <BuildStepTimeline
                steps={VIRT_IMAGE_TIMELINE_LABELS}
                activeIndex={vibTimeline.activeIndex}
                allComplete={vibTimeline.allComplete}
                failed={vibTimeline.failed}
                variant="slate"
              />
              <pre className="max-h-64 overflow-y-auto rounded border border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)] p-2 font-mono text-[11px] text-[var(--text-primary)] whitespace-pre-wrap break-all">
                {vbLog.join('\n')}
              </pre>
            </div>
          )}
            </>
          )}
        </div>
      )}

      {!loading && mkosiWorkspaces.length > 0 && (
        <div className="rounded-xl border border-violet-800/40 bg-[var(--apple-surface)] p-6 space-y-3" data-testid="mkosi-workspaces-panel">
          <h2 className="text-lg font-semibold text-[var(--text-primary)] flex items-center gap-2">
            <Boxes className="w-5 h-5 text-[var(--accent)]" aria-hidden />
            mkosi workspaces
          </h2>
          <p className="text-sm text-[var(--text-muted)]">Discovered under host mkosi-defs paths (GET /browse/mkosi-workspaces).</p>
          <ul className="text-sm text-[var(--text-secondary)] space-y-2">
            {mkosiWorkspaces.map((ws) => (
              <li key={ws.path} className="rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-surface)] px-3 py-2">
                <span className="font-mono text-[var(--link)]">{ws.name}</span>
                <span className="text-[var(--text-muted)] text-xs block mt-0.5">{ws.path}</span>
                {ws.images.length > 0 && (
                  <span className="text-xs text-[var(--text-muted)]">Images: {ws.images.join(', ')}</span>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}

      {loading ? (
        <div className="flex justify-center py-16">
          <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-[var(--accent)]" />
        </div>
      ) : images.length === 0 ? (
        <EmptyState
          icon={<HardDrive className="w-6 h-6" />}
          title="No disk images found"
          description={
            scanDirectories.length > 0
              ? `Scanned: ${scanDirectories.join(', ')}. Build a disk below or copy qcow2/ISO into a pool path.`
              : 'Connect to the daemon to discover storage pool paths on this host.'
          }
          primaryAction={
            <Link to="/create" className="btn-primary text-sm">
              Create VM
            </Link>
          }
          secondaryAction={
            <Link to="/import" className="px-4 py-2 rounded-lg border border-[var(--apple-hairline)] text-[var(--text-secondary)] hover:bg-[var(--apple-fill-tertiary)] text-sm">
              Import VM
            </Link>
          }
        />
      ) : (
        <>
          <TahoeToolbar
            search={search}
            onSearchChange={setSearch}
            placeholder="Search images…"
            trailing={
              search ? (
                <span aria-live="polite" className="text-sm text-[var(--text-muted)] shrink-0 pr-2">
                  {filteredImages.length} image{filteredImages.length !== 1 ? 's' : ''}
                </span>
              ) : null
            }
          />
          <TahoeTableWrap>
            <table className="apple-table" aria-label="Disk images">
              <thead>
                <tr>
                  <th scope="col">Name</th>
                  <th scope="col">Format</th>
                  <th scope="col">Size</th>
                  <th scope="col" className="hidden md:table-cell">Path</th>
                  <th scope="col" className="text-right">Actions</th>
                </tr>
              </thead>
              <tbody>
                {filteredImages.length === 0 && (
                  <tr><td colSpan={5} className="text-center text-[var(--text-muted)]">No images match your search.</td></tr>
                )}
                {filteredImages.map((img) => (
                  <tr key={img.path} className="group">
                    <td>
                      <div className="flex items-center gap-2">
                        <HardDrive className="w-4 h-4 text-[var(--text-muted)] shrink-0" />
                        <span className="text-sm font-medium text-[var(--text-primary)]">{img.name}</span>
                      </div>
                    </td>
                    <td>
                      <span className="px-2 py-0.5 rounded text-xs font-mono bg-[var(--surface-hover)]/60 text-[var(--text-secondary)]">
                        {img.format}
                      </span>
                    </td>
                    <td className="text-sm text-[var(--text-secondary)]">{formatBytes(img.size_bytes)}</td>
                    <td className="hidden md:table-cell text-xs text-[var(--text-muted)] font-mono max-w-xs truncate">
                      {img.path}
                    </td>
                    <td className="text-right">
                      <span className="inline-flex items-center justify-end gap-1">
                      {(img.format === 'qcow2' || img.path.toLowerCase().endsWith('.qcow2')) && (
                        <button
                          type="button"
                          onClick={() => setKvPath(img.path)}
                          className="opacity-0 group-hover:opacity-100 inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-[var(--accent)]/20 hover:bg-[var(--accent)]/40 text-[var(--link)] hover:text-[var(--link-hover)] text-xs font-medium transition mr-1"
                          title="Upload to Kubernetes (KubeVirt + CDI)"
                        >
                          <Boxes className="w-3.5 h-3.5" />
                          KubeVirt
                        </button>
                      )}
                      <button
                        onClick={() => setConfirmPath(img.path)}
                        disabled={deleting === img.path}
                        className={`opacity-0 group-hover:opacity-100 inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs font-medium transition disabled:opacity-40 ${statusDestructiveButtonClasses('hover:opacity-90')}`}
                        title="Delete image file"
                      >
                        {deleting === img.path ? (
                          <span className="animate-spin inline-block w-3 h-3 border border-red-400 border-t-transparent rounded-full" />
                        ) : (
                          <Trash2 className="w-3.5 h-3.5" />
                        )}
                        Delete
                      </button>
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        </>
      )}

      <BrowseHostPathModal
        open={outBrowseOpen}
        onClose={() => setOutBrowseOpen(false)}
        title="Choose output folder on hypervisor"
        pickDirectory
        canSelectFile={() => false}
        onSelectPath={(dir) => {
          setVbOutput(suggestQcow2Path(dir, vbOs.trim() || 'disk'))
        }}
      />

      <KubeVirtQcow2Modal open={!!kvPath} qcow2Path={kvPath ?? ''} onClose={() => setKvPath(null)} />

      <ConfirmDialog
        open={!!confirmPath}
        title="Delete disk image"
        message={`Permanently delete file from host filesystem? Any VM still referencing it will fail to start.\n\n${confirmPath ?? ''}`}
        confirmLabel="Delete file"
        variant="danger"
        onConfirm={handleDelete}
        onCancel={() => setConfirmPath(null)}
      />
    </PageLayout>
  )
}
