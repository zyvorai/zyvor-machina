// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Link } from 'react-router'
import { Check, Disc, Download, Plus, ShieldAlert, ShieldCheck, Upload, X } from 'lucide-react'
import ConfirmDialog from '../../components/ConfirmDialog'
import PlatformFilterPills from '../../components/platform/PlatformFilterPills'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton, platformStatSubtitle } from '../../components/platform/PlatformPageChrome'
import { TahoeListEmpty, TahoeTableWrap, TahoeToolbar } from '../../components/platform/tahoe/TahoeListKit'
import { MacSheet } from '../../components/platform/mac/PlatformMacUi'
import {
  approveContentImage,
  createContentImage,
  listContentImages,
  rejectContentImage,
  type ContentImage,
} from '../../api/platform'
import { uploadIso, downloadIsoFromUrl, listJobs, type JobSummary } from '../../api/extras'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusBadgeClasses, statusSurfaceClasses, statusToneClass } from '../../utils/semanticColors'

const CATEGORIES = [
  'Operating Systems',
  'Ubuntu',
  'Debian',
  'Rocky Linux',
  'Windows Server',
  'Custom Appliances',
] as const

function guessCategory(name: string): string {
  const n = name.toLowerCase()
  if (n.includes('ubuntu')) return 'Ubuntu'
  if (n.includes('debian')) return 'Debian'
  if (n.includes('rocky') || n.includes('alma') || n.includes('rhel')) return 'Rocky Linux'
  if (n.includes('windows')) return 'Windows Server'
  return 'Custom Appliances'
}

function lifecycleBadge(status: string) {
  if (status === 'available') return { label: 'Approved', tone: statusToneClass('ok'), icon: ShieldCheck }
  if (status === 'pending') return { label: 'Pending approval', tone: statusToneClass('warn'), icon: ShieldAlert }
  if (status === 'rejected') return { label: 'Rejected', tone: statusToneClass('error'), icon: ShieldAlert }
  return { label: status, tone: statusToneClass('neutral'), icon: ShieldAlert }
}

export default function PlatformContent() {
  const toast = useToastContext()
  const [rows, setRows] = useState<ContentImage[]>([])
  const [error, setError] = useState<string | null>(null)
  const [category, setCategory] = useState<string>('Operating Systems')
  const [filter, setFilter] = useState<'all' | 'pending' | 'available'>('all')
  const [name, setName] = useState('ubuntu-24.04.iso')
  const [path, setPath] = useState('/var/lib/libvirt/images/ubuntu-24.04.iso')
  const [description, setDescription] = useState('')
  const [sheetOpen, setSheetOpen] = useState(false)
  const [rejectTargetId, setRejectTargetId] = useState<string | null>(null)
  const [uploadMode, setUploadMode] = useState<'file' | 'url' | 'path'>('file')
  const [downloadUrl, setDownloadUrl] = useState('')
  const [downloadJobs, setDownloadJobs] = useState<JobSummary[]>([])
  const [uploadFile, setUploadFile] = useState<File | null>(null)
  const [uploadPct, setUploadPct] = useState(0)
  const [uploading, setUploading] = useState(false)
  const [search, setSearch] = useState('')
  const uploadAbortRef = useRef<AbortController | null>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)

  const load = useCallback(async () => {
    setError(null)
    try {
      setRows(await listContentImages())
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const pending = useMemo(() => rows.filter((r) => r.status === 'pending'), [rows])

  const filtered = useMemo(() => {
    let list = rows
    if (filter === 'pending') list = list.filter((r) => r.status === 'pending')
    if (filter === 'available') list = list.filter((r) => r.status === 'available')
    if (category !== 'Operating Systems') {
      list = list.filter((r) => (r.category ?? guessCategory(r.name)) === category)
    }
    const q = search.trim().toLowerCase()
    if (q) {
      list = list.filter((r) =>
        r.name.toLowerCase().includes(q)
        || r.path.toLowerCase().includes(q)
        || (r.description?.toLowerCase().includes(q) ?? false),
      )
    }
    return list
  }, [rows, category, filter, search])

  const add = async () => {
    try {
      await createContentImage({
        name,
        kind: 'iso',
        path,
        category: guessCategory(name),
        description: description || undefined,
      })
      toast.success('ISO submitted for approval')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  /**
   * Send the file to the hypervisor, then register the resulting host path in the
   * library. Registration is a separate call, so a successful upload whose
   * registration fails still leaves the ISO on the host — the message says so
   * rather than implying the bytes were lost.
   */
  const uploadAndRegister = async () => {
    if (!uploadFile) return
    const controller = new AbortController()
    uploadAbortRef.current = controller
    setUploading(true)
    setUploadPct(0)
    let uploadedPath = ''
    try {
      const res = await uploadIso(uploadFile, {
        signal: controller.signal,
        onProgress: (pct) => setUploadPct(pct),
      })
      uploadedPath = res.path
      await createContentImage({
        name: res.name,
        kind: 'iso',
        path: res.path,
        category: guessCategory(res.name),
        description: description || undefined,
        size_gib: Math.max(1, Math.round(res.size_bytes / (1024 * 1024 * 1024))),
      })
      toast.success(`${res.name} uploaded and submitted for approval`)
      setUploadFile(null)
      if (fileInputRef.current) fileInputRef.current.value = ''
      setSheetOpen(false)
      await load()
    } catch (e: unknown) {
      toast.error(
        uploadedPath
          ? `Uploaded to ${uploadedPath} but could not add it to the library: ${formatUserError(e)}`
          : formatUserError(e),
      )
    } finally {
      setUploading(false)
      setUploadPct(0)
      uploadAbortRef.current = null
    }
  }

  // Downloads that have already been added to the library, so a poll tick does
  // not register the same finished job over and over.
  const registeredJobsRef = useRef<Set<string>>(new Set())

  /** Poll the job registry while any download is still running. */
  const refreshDownloadJobs = useCallback(async () => {
    try {
      const all = await listJobs()
      const isoJobs = all.filter((j) => j.kind === 'iso_download')
      setDownloadJobs(isoJobs)

      // A finished download lands on the hypervisor but is invisible to the
      // library until it is registered — uploads did this and downloads did not,
      // so a downloaded ISO could not be picked in the Create-from-ISO wizard.
      for (const j of isoJobs) {
        if (j.status !== 'completed' || !j.target_path) continue
        if (registeredJobsRef.current.has(j.id)) continue
        registeredJobsRef.current.add(j.id)
        const isoName = j.target_path.split('/').pop() || j.target_path
        try {
          await createContentImage({
            name: isoName,
            kind: 'iso',
            path: j.target_path,
            category: guessCategory(isoName),
            size_gib: j.bytes_total ? Math.max(1, Math.round(j.bytes_total / (1024 * 1024 * 1024))) : undefined,
          })
          toast.success(`${isoName} added to the library`)
          await load()
        } catch {
          // Most often a duplicate path from a re-download — the ISO is on the
          // host either way, so this must not surface as a scary error.
        }
      }
    } catch {
      // A transient failure here must not blank the list the operator is watching.
    }
  }, [load, toast])

  useEffect(() => {
    void refreshDownloadJobs()
  }, [refreshDownloadJobs])

  useEffect(() => {
    if (!downloadJobs.some((j) => j.status === 'running')) return
    const t = setInterval(() => void refreshDownloadJobs(), 2000)
    return () => clearInterval(t)
  }, [downloadJobs, refreshDownloadJobs])

  const startDownload = async () => {
    const url = downloadUrl.trim()
    if (!url) return
    try {
      const started = await downloadIsoFromUrl({ url, overwrite: true })
      toast.success(`Downloading ${started.name} — track it below`)
      setDownloadUrl('')
      await refreshDownloadJobs()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const approve = async (id: string) => {
    try {
      await approveContentImage(id)
      toast.success('ISO approved for production use')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const reject = async (id: string) => {
    try {
      await rejectContentImage(id)
      toast.success('ISO rejected')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setRejectTargetId(null)
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/infrastructure" label="Infrastructure" />}
      title="Content Library"
      subtitle={
        <span className="flex flex-col gap-1">
          <span className="text-[var(--text-muted)]">ISO library with approval inbox — upload golden images for templates.</span>
          {rows.length > 0 && platformStatSubtitle([
            { label: 'Images', value: rows.length },
            { label: 'Pending', value: pending.length },
            { label: 'Approved', value: rows.filter((r) => r.status === 'available').length },
          ])}
        </span>
      }
      icon={<Disc className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={
        <>
          {pending.length > 0 && (
            <span className={`px-2 py-1 rounded-full text-xs border ${statusBadgeClasses('warn')}`}>{pending.length} pending</span>
          )}
          <PlatformRefreshButton onClick={() => void load()} />
          <Link to="/platform/create-iso" className="btn-secondary flex items-center gap-2 text-sm">
            <Disc className="w-4 h-4" /> Create from ISO
          </Link>
          <button type="button" className="btn-primary text-sm flex items-center gap-2" onClick={() => setSheetOpen(true)}><Plus className="w-4 h-4" /> Upload</button>
        </>
      }
      contentClassName="space-y-4"
    >

      {pending.length > 0 && (
        <section className={`card p-4 ${statusSurfaceClasses('warn')}`}>
          <h2 className={`text-sm font-semibold mb-3 ${statusToneClass('warn')}`}>Approval queue ({pending.length})</h2>
          <div className="space-y-2">
            {pending.map((r) => (
              <div key={r.id} className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] px-4 py-3">
                <div>
                  <p className="font-medium">{r.name}</p>
                  <p className="text-xs text-[var(--text-muted)] font-mono truncate max-w-md">{r.path}</p>
                  {r.submitted_by && <p className="text-[10px] text-[var(--text-faint)] mt-1">Submitted by {r.submitted_by}</p>}
                </div>
                <div className="flex gap-2">
                  <button type="button" className="btn-primary text-xs flex items-center gap-1" onClick={() => void approve(r.id)}>
                    <Check className="w-3 h-3" /> Approve
                  </button>
                  <button type="button" className="btn-destructive text-xs flex items-center gap-1" onClick={() => setRejectTargetId(r.id)}>
                    <X className="w-3 h-3" /> Reject
                  </button>
                </div>
              </div>
            ))}
          </div>
        </section>
      )}

      <TahoeToolbar
        search={search}
        onSearchChange={setSearch}
        placeholder="Search images…"
        trailing={
          <div className="flex flex-wrap items-center gap-2">
            <PlatformFilterPills
              value={filter}
              onChange={(id) => setFilter(id as typeof filter)}
              options={[
                { id: 'all', label: 'All' },
                { id: 'pending', label: 'Pending', count: pending.length },
                { id: 'available', label: 'Approved' },
              ]}
            />
            <div className="flex flex-wrap gap-1">
              {CATEGORIES.map((c) => (
                <button
                  key={c}
                  type="button"
                  onClick={() => setCategory(c)}
                  className={`px-3 py-1 rounded-full text-xs border transition-colors ${
                    category === c
                      ? 'bg-[var(--accent)] border-[var(--accent)] text-white'
                      : 'border-[var(--apple-hairline)] text-[var(--text-muted)] hover:border-[var(--border-strong)]'
                  }`}
                >
                  {c}
                </button>
              ))}
            </div>
          </div>
        }
      />

      {filtered.length === 0 ? (
        <TahoeListEmpty
          icon={Disc}
          title={search || filter !== 'all' || category !== 'Operating Systems' ? 'No images match' : 'No images'}
          description="Upload an ISO or register a host path for administrator approval."
          primaryAction={{ label: 'Upload', onClick: () => setSheetOpen(true) }}
        />
      ) : (
        <TahoeTableWrap>
          <table className="apple-table w-full text-sm" aria-label="Content library">
            <thead>
              <tr>
                <th scope="col">Name</th>
                <th scope="col">Status</th>
                <th scope="col">Kind</th>
                <th scope="col">Category</th>
                <th scope="col">Path</th>
                <th scope="col" className="text-right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((r) => {
                const badge = lifecycleBadge(r.status)
                const BadgeIcon = badge.icon
                return (
                  <tr key={r.id}>
                    <td>
                      <p className="font-semibold">{r.name}</p>
                      {r.description && <p className="text-xs text-[var(--text-muted)] mt-0.5">{r.description}</p>}
                    </td>
                    <td>
                      <span className={`text-[10px] flex items-center gap-1 ${badge.tone}`}>
                        <BadgeIcon className="w-3 h-3" /> {badge.label}
                      </span>
                    </td>
                    <td className="text-xs uppercase text-[var(--text-muted)]">{r.kind}</td>
                    <td className="text-xs text-[var(--text-muted)]">{r.category ?? guessCategory(r.name)}</td>
                    <td className="text-xs font-mono text-[var(--text-faint)] max-w-[200px] truncate" title={r.path}>{r.path}</td>
                    <td className="text-right">
                      <div className="flex flex-wrap justify-end gap-1">
                        {r.status === 'pending' && (
                          <>
                            <button type="button" className="btn-primary text-xs" onClick={() => void approve(r.id)}>Approve</button>
                            <button type="button" className="btn-destructive text-xs" onClick={() => setRejectTargetId(r.id)}>Reject</button>
                          </>
                        )}
                        {r.status === 'available' && r.kind === 'iso' && (
                          <Link
                            to={`/platform/create-iso?iso_path=${encodeURIComponent(r.path)}`}
                            className={`btn-secondary text-xs ${hubLinkClasses()}`}
                          >
                            Create VM
                          </Link>
                        )}
                      </div>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </TahoeTableWrap>
      )}

      <MacSheet
        open={sheetOpen}
        onClose={() => { if (!uploading) setSheetOpen(false) }}
        title="Add image"
        subtitle="Upload an ISO from this computer, or register one already on a hypervisor. Approval is required for production."
      >
        <div className="space-y-3">
          <PlatformFilterPills
            options={[
              { id: 'file', label: 'Upload from this computer' },
              { id: 'url', label: 'Download from a URL' },
              { id: 'path', label: 'Register a host path' },
            ]}
            value={uploadMode}
            onChange={(v) => setUploadMode(v as 'file' | 'url' | 'path')}
          />

          {uploadMode === 'file' ? (
            <>
              {/* Not the shared `input` class: that styles a text field, and its
                  placeholder renders underneath the native file-picker button. */}
              <input
                ref={fileInputRef}
                type="file"
                accept=".iso,application/x-cd-image"
                aria-label="ISO file"
                className="w-full rounded-xl border border-white/10 bg-[var(--apple-surface)]/50 p-2 text-sm text-[var(--text-secondary)] file:mr-3 file:cursor-pointer file:rounded-lg file:border-0 file:bg-[var(--accent)]/20 file:px-3 file:py-1.5 file:text-sm file:font-medium file:text-[var(--link)] hover:file:bg-[var(--accent)]/30 disabled:opacity-50"
                disabled={uploading}
                onChange={(e) => setUploadFile(e.target.files?.[0] ?? null)}
              />
              {uploadFile && (
                <p className="text-xs text-[var(--text-muted)]">
                  {uploadFile.name} — {(uploadFile.size / (1024 * 1024 * 1024)).toFixed(2)} GiB
                </p>
              )}
              <input className="input w-full" aria-label="Description" placeholder="description (optional)" value={description} onChange={(e) => setDescription(e.target.value)} disabled={uploading} />
              {uploading && (
                <div className="space-y-1">
                  <div className="h-2 w-full rounded-full bg-white/10 overflow-hidden">
                    <div className="h-full bg-[var(--accent)] transition-all" style={{ width: `${uploadPct}%` }} />
                  </div>
                  <p className="text-xs text-[var(--text-muted)]">Uploading… {uploadPct}% — keep this tab open.</p>
                </div>
              )}
              <div className="flex gap-2">
                <button
                  type="button"
                  className="btn-primary text-sm flex-1 flex items-center justify-center gap-2"
                  disabled={!uploadFile || uploading}
                  onClick={() => void uploadAndRegister()}
                >
                  <Upload className="w-4 h-4" /> {uploading ? 'Uploading…' : 'Upload'}
                </button>
                {uploading && (
                  <button type="button" className="btn-secondary text-sm" onClick={() => uploadAbortRef.current?.abort()}>
                    Cancel
                  </button>
                )}
              </div>
            </>
          ) : uploadMode === 'url' ? (
            <>
              <input
                className="input w-full font-mono text-xs"
                aria-label="ISO URL"
                placeholder="https://releases.ubuntu.com/…/ubuntu-24.04-live-server-amd64.iso"
                value={downloadUrl}
                onChange={(e) => setDownloadUrl(e.target.value)}
              />
              <p className="text-xs text-[var(--text-muted)]">
                The hypervisor fetches this directly — far faster than uploading from your laptop, and it keeps
                running if you close this tab. Several downloads can run at once.
              </p>
              <button type="button" className="btn-primary text-sm w-full flex items-center justify-center gap-2" disabled={!downloadUrl.trim()} onClick={() => void startDownload()}>
                <Download className="w-4 h-4" /> Start download
              </button>

              {downloadJobs.length > 0 && (
                <div className="space-y-2 pt-2 border-t border-white/[0.06]">
                  <p className="text-xs text-[var(--text-muted)]">Downloads</p>
                  {downloadJobs.slice(0, 6).map((j) => {
                    const pct = j.bytes_total && j.bytes_total > 0
                      ? Math.min(100, Math.round(((j.bytes_done ?? 0) / j.bytes_total) * 100))
                      : null
                    return (
                      <div key={j.id} className="space-y-1">
                        <div className="flex items-center justify-between gap-2 text-xs">
                          <span className="truncate text-[var(--text-secondary)]">{j.title.replace('Download ISO: ', '')}</span>
                          <span className={
                            j.status === 'failed' ? 'text-red-600'
                              : j.status === 'completed' ? 'text-emerald-600'
                                : 'text-[var(--text-muted)]'
                          }>
                            {j.status === 'running'
                              ? (pct !== null ? `${pct}%` : `${Math.round((j.bytes_done ?? 0) / (1024 * 1024))} MB`)
                              : j.status}
                          </span>
                        </div>
                        <div className="h-1.5 w-full rounded-full bg-white/10 overflow-hidden">
                          <div
                            className={`h-full transition-all ${j.status === 'failed' ? 'bg-red-500' : j.status === 'completed' ? 'bg-emerald-500' : 'bg-[var(--accent)]'} ${pct === null && j.status === 'running' ? 'animate-pulse' : ''}`}
                            style={{ width: j.status === 'completed' ? '100%' : `${pct ?? 100}%` }}
                          />
                        </div>
                        {j.error ? <p className="text-[11px] text-red-600/90">{j.error}</p> : null}
                      </div>
                    )
                  })}
                </div>
              )}
            </>
          ) : (
            <>
              <input className="input w-full" aria-label="Image name" placeholder="name" value={name} onChange={(e) => setName(e.target.value)} />
              <input className="input w-full" aria-label="Host path" placeholder="host path" value={path} onChange={(e) => setPath(e.target.value)} />
              <input className="input w-full" aria-label="Description" placeholder="description (optional)" value={description} onChange={(e) => setDescription(e.target.value)} />
              <button type="button" className="btn-primary text-sm w-full" onClick={async () => { await add(); setSheetOpen(false) }}>Submit for approval</button>
            </>
          )}
        </div>
      </MacSheet>
      <ConfirmDialog
        open={rejectTargetId !== null}
        title="Reject ISO"
        message={`Reject "${rows.find((r) => r.id === rejectTargetId)?.name}"? It will not be available for VM creation.`}
        confirmLabel="Reject"
        variant="danger"
        onCancel={() => setRejectTargetId(null)}
        onConfirm={() => { if (rejectTargetId) void reject(rejectTargetId) }}
      />
    </PlatformPageChrome>
  )
}
