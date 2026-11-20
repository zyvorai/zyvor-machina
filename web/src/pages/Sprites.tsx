// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import {
  createSprite,
  deleteSprite,
  listSpriteGoldenImages,
  listSprites,
  pauseSprite,
  resumeSprite,
  snapshotSprite,
  restoreSprite,
  type SpriteBackend,
  type SpriteHandle,
} from '../api/sprites'
import { useToastContext } from '../contexts/ToastContext'
import ConfirmDialog from '../components/ConfirmDialog'
import EmptyState from '../components/EmptyState'
import PageLayout from '../components/PageLayout'
import { Globe, Pause, Play, Camera, RotateCcw, Plus, RefreshCw, Trash2, X, Zap } from 'lucide-react'
import { formatUserError } from '../utils/apiError'
import { statusBadgeClasses } from '../utils/semanticColors'

type Tone = 'ok' | 'warn' | 'error' | 'info' | 'neutral'
import { useFocusTrap } from '../hooks/useFocusTrap'

/** Sprites live seconds-to-minutes — poll faster than a regular VM list so
 * expiry countdowns and the reaper tearing one down feel live. Matches the
 * daemon reaper's own 5s scan interval (daemon/src/sprite_registry.rs). */
const POLL_MS = 5000

const TTL_PRESETS: { label: string; seconds: number }[] = [
  { label: '1 minute', seconds: 60 },
  { label: '5 minutes', seconds: 300 },
  { label: '15 minutes', seconds: 900 },
  { label: '30 minutes', seconds: 1800 },
  { label: '1 hour', seconds: 3600 },
]

function stateTone(state: string): Tone {
  switch (state) {
    case 'running': return 'ok'
    case 'booting': return 'info'
    case 'paused': return 'warn'
    case 'suspended': return 'neutral'
    case 'reaping': return 'warn'
    default: return 'neutral'
  }
}

function backendLabel(backend: string): string {
  if (backend === 'cloudhypervisor') return 'Cloud Hypervisor'
  if (backend === 'firecracker') return 'Firecracker'
  return 'Libvirt'
}

/** Use-case guidance shown under the backend picker — helps pick between the three
 * VMM backends without needing to know their internals up front. */
function backendHint(backend: string): string {
  if (backend === 'firecracker') {
    return 'Fastest boot, smallest footprint, minimal attack surface. Best for short-lived, high-density, or security-sensitive workloads — no GPU or complex device support.'
  }
  if (backend === 'cloudhypervisor') {
    return 'Still lightweight and fast-booting, with broader virtio device support than Firecracker. A good middle ground when a sprite needs more device flexibility but should stay minimal.'
  }
  return 'Full QEMU/KVM feature set — GPU/USB/PCI passthrough, snapshots, live migration. Best for longer-lived or more complex workloads; slower boot and heavier footprint than the other two.'
}

/** "expires in 4m 12s" / "expired" — recomputed on every poll tick, not a live per-second ticker. */
function timeUntil(iso: string): string {
  const ms = new Date(iso).getTime() - Date.now()
  if (!Number.isFinite(ms)) return '—'
  if (ms <= 0) return 'expired'
  const totalSeconds = Math.floor(ms / 1000)
  const m = Math.floor(totalSeconds / 60)
  const s = totalSeconds % 60
  return m > 0 ? `${m}m ${s}s` : `${s}s`
}

export default function SpritesPage() {
  const [sprites, setSprites] = useState<SpriteHandle[]>([])
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<SpriteHandle | null>(null)
  const [createOpen, setCreateOpen] = useState(false)
  const [, setTick] = useState(0)
  const toast = useToastContext()

  const load = useCallback(async () => {
    try {
      setLoadError(null)
      setSprites(await listSprites())
    } catch (e: unknown) {
      setLoadError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    load()
    const id = setInterval(load, POLL_MS)
    return () => clearInterval(id)
  }, [load])

  // Redraw expiry countdowns between polls without re-fetching.
  useEffect(() => {
    const id = setInterval(() => setTick((t) => t + 1), 1000)
    return () => clearInterval(id)
  }, [])

  const handleDelete = async () => {
    if (!deleteTarget) return
    const target = deleteTarget
    setDeleteTarget(null)
    try {
      await deleteSprite(target.sprite_id)
      toast.success(`Deleted sprite '${target.sprite_id}'`)
      load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const handlePause = async (s: SpriteHandle) => {
    try {
      await pauseSprite(s.sprite_id)
      toast.success(`Paused '${s.sprite_id}'`)
      load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const handleResume = async (s: SpriteHandle) => {
    try {
      await resumeSprite(s.sprite_id)
      toast.success(`Resumed '${s.sprite_id}'`)
      load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const handleSnapshot = async (s: SpriteHandle) => {
    try {
      await snapshotSprite(s.sprite_id)
      toast.success(`Suspended '${s.sprite_id}' to a snapshot`)
      load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const handleRestore = async (s: SpriteHandle) => {
    try {
      await restoreSprite(s.sprite_id)
      toast.success(`Restored '${s.sprite_id}'`)
      load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  return (
    <PageLayout
      eyebrow="Hypervisor"
      title="Sprites"
      subtitle="Instant, disposable sandbox VMs — TTL-reaped, no persistent state."
      icon={<Zap className="w-6 h-6" />}
      actions={
        <div className="flex items-center gap-2">
          <button onClick={load} className="p-2 hover:bg-[var(--surface-hover)] rounded transition" title="Refresh" aria-label="Refresh">
            <RefreshCw className="w-4 h-4" />
          </button>
          <button
            onClick={() => setCreateOpen(true)}
            className="btn-primary text-sm inline-flex items-center gap-1.5"
          >
            <Plus className="w-4 h-4" />
            New Sprite
          </button>
        </div>
      }
      contentLoading={loading}
      error={loadError}
      errorTitle="Failed to load sprites"
      onErrorRetry={load}
      onErrorDismiss={() => setLoadError(null)}
    >
      {loadError ? null : sprites.length === 0 ? (
        <EmptyState
          icon={<Zap className="w-6 h-6" />}
          title="No sprites running"
          description="Sprites are throwaway sandbox VMs cloned from a golden image and torn down automatically after their TTL — useful for AI-agent or CI sandboxes that don't need to persist."
        />
      ) : (
        <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-x-auto">
          <table className="w-full" aria-label="Sprites">
            <thead>
              <tr className="border-b border-[var(--apple-hairline)] text-left text-sm text-[var(--text-muted)]">
                <th scope="col" className="px-6 py-3">Sprite</th>
                <th scope="col" className="px-6 py-3">State</th>
                <th scope="col" className="px-6 py-3">Backend</th>
                <th scope="col" className="px-6 py-3 hidden md:table-cell">vsock CID</th>
                <th scope="col" className="px-6 py-3 hidden md:table-cell">Network</th>
                <th scope="col" className="px-6 py-3">Expires</th>
                <th scope="col" className="px-6 py-3 text-right">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-[var(--apple-hairline)]/50">
              {sprites.map((s) => (
                <tr key={s.sprite_id} className="hover:bg-[var(--surface-hover)]/50">
                  <td className="px-6 py-3 font-mono text-xs">{s.sprite_id}</td>
                  <td className="px-6 py-3">
                    <span className={`inline-flex items-center gap-1 px-2 py-0.5 rounded text-xs font-medium ${statusBadgeClasses(stateTone(s.state))}`}>
                      {s.state}
                    </span>
                  </td>
                  <td className="px-6 py-3 text-sm text-[var(--text-secondary)]">{backendLabel(s.backend ?? 'libvirt')}</td>
                  <td className="px-6 py-3 text-sm text-[var(--text-muted)] hidden md:table-cell">{s.vsock_cid ?? '—'}</td>
                  <td className="px-6 py-3 text-sm text-[var(--text-muted)] hidden md:table-cell">
                    {s.network_egress ? (
                      <span className="inline-flex items-center gap-1 text-[var(--accent)]" title="Attached to the default NAT network">
                        <Globe className="w-3.5 h-3.5" />
                        Egress
                      </span>
                    ) : (
                      <span className="text-[var(--text-muted)]">vsock only</span>
                    )}
                  </td>
                  <td className="px-6 py-3 text-sm text-[var(--text-muted)]">{timeUntil(s.expires_at)}</td>
                  <td className="px-6 py-3">
                    <div className="flex items-center justify-end gap-1">
                      {s.backend !== 'libvirt' && s.state === 'running' && (
                        <button
                          onClick={() => handlePause(s)}
                          className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition"
                          title="Pause"
                          aria-label="Pause"
                        >
                          <Pause className="w-4 h-4 text-[var(--text-secondary)]" />
                        </button>
                      )}
                      {s.backend !== 'libvirt' && s.state === 'paused' && (
                        <>
                          <button
                            onClick={() => handleResume(s)}
                            className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition"
                            title="Resume"
                            aria-label="Resume"
                          >
                            <Play className="w-4 h-4 text-[var(--text-secondary)]" />
                          </button>
                          <button
                            onClick={() => handleSnapshot(s)}
                            className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition"
                            title="Suspend to snapshot"
                            aria-label="Suspend to snapshot"
                          >
                            <Camera className="w-4 h-4 text-[var(--text-secondary)]" />
                          </button>
                        </>
                      )}
                      {s.backend !== 'libvirt' && s.state === 'running' && (
                        <button
                          onClick={() => handleSnapshot(s)}
                          className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition"
                          title="Suspend to snapshot"
                          aria-label="Suspend to snapshot"
                        >
                          <Camera className="w-4 h-4 text-[var(--text-secondary)]" />
                        </button>
                      )}
                      {s.backend !== 'libvirt' && s.state === 'suspended' && (
                        <button
                          onClick={() => handleRestore(s)}
                          className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition"
                          title="Restore from snapshot"
                          aria-label="Restore from snapshot"
                        >
                          <RotateCcw className="w-4 h-4 text-[var(--text-secondary)]" />
                        </button>
                      )}
                      <button
                        onClick={() => setDeleteTarget(s)}
                        className="p-1.5 hover:bg-red-600/20 rounded transition"
                        title="Delete"
                        aria-label="Delete"
                      >
                        <Trash2 className="w-4 h-4 text-red-400" />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <ConfirmDialog
        open={!!deleteTarget}
        title="Delete Sprite"
        message={`Delete sprite '${deleteTarget?.sprite_id}'? This tears it down immediately instead of waiting for its TTL.`}
        confirmLabel="Delete"
        onConfirm={handleDelete}
        onCancel={() => setDeleteTarget(null)}
      />
      <NewSpriteModal
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        onCreated={() => { setCreateOpen(false); load() }}
      />
    </PageLayout>
  )
}

function NewSpriteModal({ open, onClose, onCreated }: { open: boolean; onClose: () => void; onCreated: () => void }) {
  const toast = useToastContext()
  const panelRef = useRef<HTMLDivElement>(null)
  useFocusTrap(panelRef, open, onClose)

  const [images, setImages] = useState<string[]>([])
  const [imagesError, setImagesError] = useState<string | null>(null)
  const [goldenImage, setGoldenImage] = useState('')
  const [vcpus, setVcpus] = useState('1')
  const [memoryMb, setMemoryMb] = useState('512')
  const [ttlSeconds, setTtlSeconds] = useState(String(TTL_PRESETS[1].seconds))
  const [backend, setBackend] = useState<SpriteBackend>('libvirt')
  const [networkEgress, setNetworkEgress] = useState(false)
  const [submitting, setSubmitting] = useState(false)

  useEffect(() => {
    if (!open) return
    setImagesError(null)
    listSpriteGoldenImages()
      .then((names) => {
        setImages(names)
        setGoldenImage((prev) => (prev && names.includes(prev) ? prev : names[0] ?? ''))
      })
      .catch((e: unknown) => setImagesError(formatUserError(e)))
  }, [open])

  const handleSubmit = async () => {
    if (!goldenImage) return
    setSubmitting(true)
    try {
      const handle = await createSprite({
        golden_image: goldenImage,
        vcpus: parseInt(vcpus, 10) || undefined,
        memory_mb: parseInt(memoryMb, 10) || undefined,
        ttl_seconds: parseInt(ttlSeconds, 10) || undefined,
        backend,
        network_egress: networkEgress,
      })
      toast.success(`Sprite '${handle.sprite_id}' booting`)
      onCreated()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSubmitting(false)
    }
  }

  if (!open) return null

  return (
    <div
      className="fixed inset-0 z-[95] flex items-center justify-center p-4 bg-black/60"
      onClick={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        ref={panelRef}
        className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-xl shadow-xl w-full max-w-md flex flex-col"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby="new-sprite-title"
      >
        <div className="p-4 border-b border-[var(--apple-hairline)] flex items-center justify-between gap-2">
          <h2 id="new-sprite-title" className="text-lg font-semibold text-[var(--text-primary)] flex items-center gap-2">
            <Zap className="w-5 h-5 text-[var(--accent)]" />
            New Sprite
          </h2>
          <button type="button" className="p-1.5 rounded-lg hover:bg-[var(--apple-fill-tertiary)] text-[var(--text-muted)]" onClick={onClose} aria-label="Close">
            <X className="w-4 h-4" />
          </button>
        </div>
        <div className="p-4 space-y-4 text-sm">
          <div className="space-y-1.5">
            <label className="text-xs font-medium text-[var(--text-muted)] uppercase tracking-wide" htmlFor="sprite-golden-image">
              Golden image
            </label>
            {imagesError ? (
              <p className="text-xs text-red-400">{imagesError}</p>
            ) : images.length === 0 ? (
              <p className="text-xs text-[var(--text-muted)]">
                No golden images found. Copy a qcow2 into the sprite-images registry on the daemon host first.
              </p>
            ) : (
              <select
                id="sprite-golden-image"
                value={goldenImage}
                onChange={(e) => setGoldenImage(e.target.value)}
                className="w-full bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-[var(--text-primary)]"
              >
                {images.map((name) => (
                  <option key={name} value={name}>{name}</option>
                ))}
              </select>
            )}
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <label className="text-xs font-medium text-[var(--text-muted)] uppercase tracking-wide" htmlFor="sprite-vcpus">vCPUs</label>
              <input
                id="sprite-vcpus"
                type="number"
                min={1}
                max={8}
                value={vcpus}
                onChange={(e) => setVcpus(e.target.value)}
                className="w-full bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-[var(--text-primary)]"
              />
            </div>
            <div className="space-y-1.5">
              <label className="text-xs font-medium text-[var(--text-muted)] uppercase tracking-wide" htmlFor="sprite-memory">Memory (MB)</label>
              <input
                id="sprite-memory"
                type="number"
                min={1}
                max={8192}
                step={128}
                value={memoryMb}
                onChange={(e) => setMemoryMb(e.target.value)}
                className="w-full bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-[var(--text-primary)]"
              />
            </div>
          </div>

          <div className="space-y-1.5">
            <label className="text-xs font-medium text-[var(--text-muted)] uppercase tracking-wide" htmlFor="sprite-ttl">Time to live</label>
            <select
              id="sprite-ttl"
              value={ttlSeconds}
              onChange={(e) => setTtlSeconds(e.target.value)}
              className="w-full bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-[var(--text-primary)]"
            >
              {TTL_PRESETS.map((p) => (
                <option key={p.seconds} value={p.seconds}>{p.label}</option>
              ))}
            </select>
          </div>

          <div className="space-y-1.5">
            <span className="text-xs font-medium text-[var(--text-muted)] uppercase tracking-wide">Backend</span>
            <div className="flex gap-2">
              {(['libvirt', 'cloudhypervisor', 'firecracker'] as const).map((b) => (
                <button
                  key={b}
                  type="button"
                  onClick={() => setBackend(b)}
                  className={`flex-1 px-3 py-2 rounded-lg text-sm font-medium border transition ${
                    backend === b
                      ? 'bg-[var(--accent)] border-[var(--accent)] text-white'
                      : 'bg-[var(--apple-fill-tertiary)] border-[var(--apple-hairline)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]'
                  }`}
                >
                  {backendLabel(b)}
                </button>
              ))}
            </div>
            <p className="text-xs text-[var(--text-muted)] mt-0.5">{backendHint(backend)}</p>
          </div>

          <label className="flex items-start gap-2.5 cursor-pointer">
            <input
              type="checkbox"
              checked={networkEgress}
              onChange={(e) => setNetworkEgress(e.target.checked)}
              className="mt-0.5 w-4 h-4 rounded border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)] text-[var(--accent)] focus:ring-[var(--accent)]"
            />
            <span className="text-sm text-[var(--text-secondary)]">
              Network egress
              <span className="block text-xs text-[var(--text-muted)] mt-0.5">
                Attach to the host&rsquo;s default NAT network for outbound internet access. Shares that network with
                regular VMs on this host — no per-sprite isolation.
              </span>
            </span>
          </label>
        </div>
        <div className="p-4 border-t border-[var(--apple-hairline)] flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="px-3 py-2 rounded-lg text-sm font-medium text-[var(--text-secondary)] hover:bg-[var(--apple-fill-tertiary)] transition"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={!goldenImage || submitting}
            onClick={handleSubmit}
            className="btn-primary text-sm disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {submitting ? 'Creating…' : 'Create'}
          </button>
        </div>
      </div>
    </div>
  )
}
