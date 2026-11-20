// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import { Box, Layers, Play, Plus, RefreshCw, Square, Trash2, X } from 'lucide-react'
import {
  createVesselPod,
  getVesselStatus,
  listVesselPods,
  removeVesselPod,
  shortId,
  startVesselPod,
  stopVesselPod,
  type PodSummary,
  type VesselStatus,
} from '../api/vessel'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { useFocusTrap } from '../hooks/useFocusTrap'
import PageLayout from '../components/PageLayout'
import EmptyState from '../components/EmptyState'
import ConfirmDialog from '../components/ConfirmDialog'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { statusBadgeClasses } from '../utils/semanticColors'
import { platformStatSubtitle } from '../components/platform/PlatformPageChrome'

function podTone(status: string): 'ok' | 'warn' | 'error' | 'neutral' | 'info' {
  const s = status.toLowerCase()
  if (s === 'running') return 'ok'
  if (s === 'degraded' || s === 'paused') return 'warn'
  if (s === 'dead' || s === 'exited' || s === 'stopped') return 'error'
  if (s === 'created') return 'info'
  return 'neutral'
}

export default function ContainerPodsPage() {
  const toast = useToastContext()
  const [status, setStatus] = useState<VesselStatus | null>(null)
  const [items, setItems] = useState<PodSummary[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<PodSummary | null>(null)
  const [showCreate, setShowCreate] = useState(false)
  const [newName, setNewName] = useState('')
  const [creating, setCreating] = useState(false)
  const [search, setSearch] = useState('')
  const createRef = useRef<HTMLDivElement>(null)
  useFocusTrap(createRef, showCreate, () => setShowCreate(false))

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const st = await getVesselStatus()
      setStatus(st)
      if (!st.connected) {
        setItems([])
        setError(st.error || 'Container engine not connected')
        return
      }
      if (!st.capabilities?.pods) {
        setItems([])
        setError('Podman pods require a Podman engine (Docker does not support pods).')
        return
      }
      const list = await listVesselPods()
      setItems(list)
    } catch (e) {
      setError(formatUserError(e))
      setItems([])
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const runAction = async (id: string, label: string, fn: () => Promise<unknown>) => {
    setBusy(id)
    try {
      await fn()
      toast.success(`${label} ok`)
      await load()
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(null)
    }
  }

  const onCreate = async () => {
    const name = newName.trim()
    if (!name) {
      toast.error('Pod name is required')
      return
    }
    setCreating(true)
    try {
      await createVesselPod(name)
      toast.success(`Pod ${name} created`)
      setShowCreate(false)
      setNewName('')
      await load()
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setCreating(false)
    }
  }

  const filteredItems = items.filter((p) => {
    const q = search.trim().toLowerCase()
    if (!q) return true
    return (
      p.name.toLowerCase().includes(q)
      || p.status.toLowerCase().includes(q)
      || shortId(p.id).toLowerCase().includes(q)
    )
  })

  return (
    <PageLayout
      eyebrow="Vessel"
      title="Container Pods"
      subtitle={<>{platformStatSubtitle([{ label: 'Pods', value: items.length }])}<span className="mt-2 block">Podman pods on this host — shared network namespace for grouped containers.</span></>}
      icon={<Layers className="w-5 h-5" />}
      loading={loading && items.length === 0 && !error}
      error={error}
      errorTitle="Pods unavailable"
      errorHints={[
        'Pods require Podman (not Docker)',
        'Use Containers for Docker/Podman container lifecycle',
        'Kubernetes pods are under /k8s/workloads',
      ]}
      onErrorRetry={() => void load()}
      actions={
        <div className="flex items-center gap-2">
          <Link
            to="/containers"
            className="inline-flex items-center gap-1.5 btn-secondary text-sm text-[var(--text-primary)] hover:bg-[var(--apple-fill-tertiary)]"
          >
            <Box className="w-3.5 h-3.5" /> Containers
          </Link>
          <button
            type="button"
            disabled={!status?.capabilities?.pods}
            onClick={() => setShowCreate(true)}
            className="btn-primary text-sm disabled:opacity-40"
          >
            <Plus className="w-3.5 h-3.5" /> Create pod
          </button>
          <button
            type="button"
            onClick={() => void load()}
            className="inline-flex items-center gap-1.5 btn-secondary text-sm text-[var(--text-primary)] hover:bg-[var(--apple-fill-tertiary)]"
          >
            <RefreshCw className="w-3.5 h-3.5" /> Refresh
          </button>
        </div>
      }
    >
      <TahoeToolbar
        search={search}
        onSearchChange={setSearch}
        placeholder="Search pods…"
        trailing={
          search ? (
            <span aria-live="polite" className="text-sm text-[var(--text-muted)] shrink-0 pr-2">
              {filteredItems.length} pod{filteredItems.length !== 1 ? 's' : ''}
            </span>
          ) : null
        }
      />

      {!loading && !error && items.length === 0 && (
        <EmptyState
          icon={<Layers className="w-8 h-8" />}
          title="No pods"
          description="Create a Podman pod to group containers with a shared network namespace."
          primaryAction={
            <button
              type="button"
              onClick={() => setShowCreate(true)}
              className="btn-primary text-sm"
            >
              Create pod
            </button>
          }
        />
      )}

      {items.length > 0 && (
        <TahoeTableWrap>
          <table className="apple-table text-sm" aria-label="Container pods">
            <thead>
              <tr>
                <th scope="col">Name</th>
                <th scope="col">Status</th>
                <th scope="col">Containers</th>
                <th scope="col">ID</th>
                <th scope="col" className="text-right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {filteredItems.length === 0 && (
                <tr><td colSpan={5} className="text-center text-[var(--text-muted)]">No pods match your search.</td></tr>
              )}
              {filteredItems.map((p) => {
                const tone = podTone(p.status)
                const disabled = busy === p.id
                return (
                  <tr key={p.id}>
                    <td className="text-[var(--text-primary)] font-medium">{p.name}</td>
                    <td>
                      <span className={statusBadgeClasses(tone)}>{p.status}</span>
                    </td>
                    <td className="text-[var(--text-muted)]">{p.containers.length}</td>
                    <td className="font-mono text-xs text-[var(--text-muted)]">{shortId(p.id)}</td>
                    <td>
                      <div className="flex justify-end gap-1">
                        <button
                          type="button"
                          disabled={disabled}
                          title="Start"
                          onClick={() => void runAction(p.id, 'Start', () => startVesselPod(p.id))}
                          className="p-1.5 rounded-lg hover:bg-emerald-500/20 text-emerald-400 disabled:opacity-40"
                        >
                          <Play className="w-3.5 h-3.5" />
                        </button>
                        <button
                          type="button"
                          disabled={disabled}
                          title="Stop"
                          onClick={() => void runAction(p.id, 'Stop', () => stopVesselPod(p.id))}
                          className="p-1.5 rounded-lg hover:bg-amber-500/20 text-amber-400 disabled:opacity-40"
                        >
                          <Square className="w-3.5 h-3.5" />
                        </button>
                        <button
                          type="button"
                          disabled={disabled}
                          title="Remove"
                          onClick={() => setDeleteTarget(p)}
                          className="p-1.5 rounded-lg hover:bg-rose-500/20 text-rose-400 disabled:opacity-40"
                        >
                          <Trash2 className="w-3.5 h-3.5" />
                        </button>
                      </div>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </TahoeTableWrap>
      )}

      {showCreate && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
          <div
            ref={createRef}
            className="w-full max-w-xl max-h-[90dvh] overflow-y-auto rounded-2xl border border-white/10 bg-[var(--apple-surface)] p-6 shadow-xl"
            role="dialog"
            aria-modal="true"
            aria-labelledby="create-pod-title"
          >
            <div className="flex items-center justify-between mb-4">
              <h2 id="create-pod-title" className="text-lg font-semibold text-[var(--text-primary)]">
                Create Podman pod
              </h2>
              <button
                type="button"
                onClick={() => setShowCreate(false)}
                className="p-1 rounded-lg hover:bg-white/10 text-[var(--text-muted)]"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <label className="block text-sm text-[var(--text-muted)] mb-1" htmlFor="pod-name">
              Name
            </label>
            <input
              id="pod-name"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              className="w-full rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-surface)] px-3 py-2 text-[var(--text-primary)] mb-4"
              placeholder="my-pod"
              autoFocus
            />
            <div className="flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setShowCreate(false)}
                className="btn-secondary text-sm text-[var(--text-secondary)]"
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={creating}
                onClick={() => void onCreate()}
                className="btn-primary text-sm disabled:opacity-40"
              >
                {creating ? 'Creating…' : 'Create'}
              </button>
            </div>
          </div>
        </div>
      )}

      <ConfirmDialog
        open={deleteTarget !== null}
        title="Remove pod"
        message={deleteTarget ? `Force-remove pod “${deleteTarget.name}”?` : ''}
        confirmLabel="Remove"
        variant="danger"
        onCancel={() => setDeleteTarget(null)}
        onConfirm={() => {
          const t = deleteTarget
          setDeleteTarget(null)
          if (!t) return
          void runAction(t.id, 'Remove', () => removeVesselPod(t.id, true))
        }}
      />
    </PageLayout>
  )
}
