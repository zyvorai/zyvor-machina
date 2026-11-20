// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import {
  Box,
  Play,
  Plus,
  RefreshCw,
  Square,
  Trash2,
  RotateCw,
  Layers,
  X,
} from 'lucide-react'
import {
  createVesselContainer,
  getVesselStatus,
  listVesselContainers,
  reconnectVessel,
  removeVesselContainer,
  restartVesselContainer,
  runVesselWindowsDockur,
  shortId,
  startVesselContainer,
  stopVesselContainer,
  type ContainerSummary,
  type VesselStatus,
} from '../api/vessel'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import { useFocusTrap } from '../hooks/useFocusTrap'
import PageLayout from '../components/PageLayout'
import EmptyState from '../components/EmptyState'
import ConfirmDialog from '../components/ConfirmDialog'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { statusBadgeClasses, statusToneClass } from '../utils/semanticColors'
import { platformStatSubtitle } from '../components/platform/PlatformPageChrome'

function statusTone(status: string): 'ok' | 'warn' | 'error' | 'neutral' | 'info' {
  const s = status.toLowerCase()
  if (s === 'running') return 'ok'
  if (s === 'paused' || s === 'restarting') return 'warn'
  if (s === 'dead' || s === 'exited') return 'error'
  if (s === 'created') return 'info'
  return 'neutral'
}

function parseCommand(raw: string): string[] {
  const t = raw.trim()
  if (!t) return []
  return t.split(/\s+/).filter(Boolean)
}

export default function ContainersPage() {
  const toast = useToastContext()
  const [searchParams, setSearchParams] = useSearchParams()
  const [status, setStatus] = useState<VesselStatus | null>(null)
  const [items, setItems] = useState<ContainerSummary[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<ContainerSummary | null>(null)
  const [showCreate, setShowCreate] = useState(false)
  const [creating, setCreating] = useState(false)
  const [windowsBusy, setWindowsBusy] = useState<
    'win10' | 'win11' | 'windows-server-2022' | 'windows-server-2025' | null
  >(null)
  const [newName, setNewName] = useState('')
  const [newImage, setNewImage] = useState('docker.io/library/nginx:alpine')
  const [newCommand, setNewCommand] = useState('')
  const [startAfterCreate, setStartAfterCreate] = useState(true)
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
      const list = await listVesselContainers(true)
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

  useEffect(() => {
    if (searchParams.get('create') === '1' && status?.connected) {
      setShowCreate(true)
      const next = new URLSearchParams(searchParams)
      next.delete('create')
      setSearchParams(next, { replace: true })
    }
  }, [searchParams, setSearchParams, status?.connected])

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
    const image = newImage.trim()
    if (!name || !image) {
      toast.error('Name and image are required')
      return
    }
    setCreating(true)
    try {
      await createVesselContainer({
        name,
        image,
        command: parseCommand(newCommand),
        start: startAfterCreate,
      })
      toast.success(startAfterCreate ? `Container ${name} created and started` : `Container ${name} created`)
      setShowCreate(false)
      setNewName('')
      setNewCommand('')
      await load()
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setCreating(false)
    }
  }

  const onRunWindows = async (
    guest: 'win10' | 'win11' | 'windows-server-2022' | 'windows-server-2025',
  ) => {
    setWindowsBusy(guest)
    try {
      const r = await runVesselWindowsDockur({ guest, use_golden: true })
      toast.success(
        `${r.guest} running via ${r.engine}${r.golden ? ' (golden disk)' : ''} — viewer ${r.web}, RDP ${r.rdp}`,
      )
      await load()
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setWindowsBusy(null)
    }
  }

  const podsCapable = Boolean(status?.capabilities?.pods)
  const runningCount = items.filter((c) => String(c.status).toLowerCase() === 'running').length
  const filteredItems = items.filter((c) => {
    const q = search.trim().toLowerCase()
    if (!q) return true
    return (
      (c.name || '').toLowerCase().includes(q)
      || c.image.toLowerCase().includes(q)
      || c.status.toLowerCase().includes(q)
      || c.state.toLowerCase().includes(q)
      || shortId(c.id).toLowerCase().includes(q)
    )
  })

  return (
    <PageLayout
      eyebrow="Vessel"
      title="Containers"
      subtitle={<>{platformStatSubtitle([{ label: 'Containers', value: items.length }, { label: 'Running', value: runningCount }, { label: 'Stopped', value: items.length - runningCount }])}<span className="mt-2 block">Local Podman / Docker containers on this host (Vessel). Windows 10/11 run via dockur with KVM.</span></>}
      icon={<Box className="w-5 h-5" />}
      loading={loading && items.length === 0 && !error}
      error={error}
      errorTitle="Container engine unavailable"
      errorHints={[
        'Start Podman or Docker on this host',
        'Or set [vessel].socket in /etc/machina/config.toml',
        'Kubernetes pods remain under K8s Workloads',
      ]}
      onErrorRetry={() => void load()}
      actions={
        <div className="flex items-center gap-2">
          {podsCapable && (
            <Link
              to="/containers/pods"
              className="inline-flex items-center gap-1.5 btn-secondary text-sm text-[var(--text-primary)] hover:bg-[var(--apple-fill-tertiary)]"
            >
              <Layers className="w-3.5 h-3.5" /> Pods
            </Link>
          )}
          <button
            type="button"
            disabled={!status?.connected || windowsBusy !== null}
            onClick={() => void onRunWindows('win11')}
            className="inline-flex items-center gap-1.5 btn-secondary text-sm disabled:opacity-40"
            title="Start dockurr/windows in Podman or Docker (uses golden qcow2 when present)"
          >
            {windowsBusy === 'win11' ? (
              <RefreshCw className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <Play className="w-3.5 h-3.5" />
            )}
            Win 11
          </button>
          <button
            type="button"
            disabled={!status?.connected || windowsBusy !== null}
            onClick={() => void onRunWindows('win10')}
            className="inline-flex items-center gap-1.5 btn-secondary text-sm disabled:opacity-40"
            title="Start dockurr/windows in Podman or Docker (uses golden qcow2 when present)"
          >
            {windowsBusy === 'win10' ? (
              <RefreshCw className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <Play className="w-3.5 h-3.5" />
            )}
            Win 10
          </button>
          <button
            type="button"
            disabled={!status?.connected || windowsBusy !== null}
            onClick={() => void onRunWindows('windows-server-2022')}
            className="inline-flex items-center gap-1.5 btn-secondary text-sm disabled:opacity-40"
            title="Windows Server 2022 via dockur"
          >
            {windowsBusy === 'windows-server-2022' ? (
              <RefreshCw className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <Play className="w-3.5 h-3.5" />
            )}
            Server 2022
          </button>
          <button
            type="button"
            disabled={!status?.connected || windowsBusy !== null}
            onClick={() => void onRunWindows('windows-server-2025')}
            className="inline-flex items-center gap-1.5 btn-secondary text-sm disabled:opacity-40"
            title="Windows Server 2025 via dockur"
          >
            {windowsBusy === 'windows-server-2025' ? (
              <RefreshCw className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <Play className="w-3.5 h-3.5" />
            )}
            Server 2025
          </button>
          <button
            type="button"
            disabled={!status?.connected}
            onClick={() => setShowCreate(true)}
            className="inline-flex items-center gap-1.5 btn-primary text-sm disabled:opacity-40"
          >
            <Plus className="w-3.5 h-3.5" /> Create container
          </button>
          <button
            type="button"
            onClick={() =>
              void reconnectVessel()
                .then(() => load())
                .catch((e) => toast.error(formatUserError(e)))
            }
            className="inline-flex items-center gap-1.5 btn-secondary text-sm text-[var(--text-primary)] hover:bg-[var(--apple-fill-tertiary)]"
          >
            <RefreshCw className="w-3.5 h-3.5" /> Reconnect
          </button>
          <button
            type="button"
            onClick={() => void load()}
            className="btn-primary text-sm"
          >
            <RefreshCw className="w-3.5 h-3.5" /> Refresh
          </button>
        </div>
      }
    >
      <TahoeToolbar
        search={search}
        onSearchChange={setSearch}
        placeholder="Search containers…"
        trailing={
          status?.connected ? (
            <span className="text-sm text-[var(--text-muted)] shrink-0 pr-2">
              {filteredItems.length} container{filteredItems.length !== 1 ? 's' : ''}
              {items.length > 0 ? ` · ${runningCount} running` : ''}
              {status.engine ? ` · ${status.engine}` : ''}
            </span>
          ) : null
        }
      />

      {!loading && status?.connected && items.length === 0 && (
        <EmptyState
          icon={<Box className="w-8 h-8" />}
          title="No containers"
          description="Run a Linux image, or start Windows 10/11 via dockur (Podman preferred, Docker fallback). Golden disks from Golden Forge are reused when present."
          primaryAction={
            <div className="flex flex-wrap gap-2">
              <button type="button" onClick={() => void onRunWindows('win11')} className="btn-primary text-sm">
                Run Windows 11
              </button>
              <button type="button" onClick={() => setShowCreate(true)} className="btn-secondary text-sm">
                Create container
              </button>
            </div>
          }
        />
      )}

      {items.length > 0 && (
        <TahoeTableWrap>
          <table className="apple-table text-sm" aria-label="Containers">
            <thead>
              <tr>
                <th scope="col">Name</th>
                <th scope="col">Image</th>
                <th scope="col">Status</th>
                <th scope="col">ID</th>
                <th scope="col" className="text-right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {filteredItems.length === 0 && (
                <tr><td colSpan={5} className="text-center text-[var(--text-muted)]">No containers match your search.</td></tr>
              )}
              {filteredItems.map((c) => {
                const tone = statusTone(c.status)
                const id = c.id
                const disabled = busy === id
                return (
                  <tr key={id}>
                    <td className="text-[var(--text-primary)] font-medium">
                      {c.name || '—'}
                      {(c.labels?.['machina.io/windows-dockur'] || /dockurr\/windows/i.test(c.image)) && (
                        <span className={`ml-2 text-[10px] uppercase tracking-wide ${statusToneClass('info')}`}>
                          dockur
                        </span>
                      )}
                    </td>
                    <td className="text-[var(--text-secondary)] font-mono text-xs max-w-[14rem] truncate">
                      {c.image}
                    </td>
                    <td>
                      <span className={statusBadgeClasses(tone)}>{c.status}</span>
                      <span className={`ml-2 text-xs ${statusToneClass(tone)}`}>{c.state}</span>
                    </td>
                    <td className="font-mono text-xs text-[var(--text-muted)]">{shortId(id)}</td>
                    <td>
                      <div className="flex justify-end gap-1">
                        <button
                          type="button"
                          disabled={disabled}
                          title="Start"
                          onClick={() => void runAction(id, 'Start', () => startVesselContainer(id))}
                          className="p-1.5 rounded-lg hover:bg-emerald-500/20 text-emerald-400 disabled:opacity-40"
                        >
                          <Play className="w-3.5 h-3.5" />
                        </button>
                        <button
                          type="button"
                          disabled={disabled}
                          title="Stop"
                          onClick={() => void runAction(id, 'Stop', () => stopVesselContainer(id))}
                          className="p-1.5 rounded-lg hover:bg-amber-500/20 text-amber-400 disabled:opacity-40"
                        >
                          <Square className="w-3.5 h-3.5" />
                        </button>
                        <button
                          type="button"
                          disabled={disabled}
                          title="Restart"
                          onClick={() =>
                            void runAction(id, 'Restart', () => restartVesselContainer(id))
                          }
                          className="p-1.5 rounded-lg hover:bg-[var(--accent)]/20 text-[var(--link)] disabled:opacity-40"
                        >
                          <RotateCw className="w-3.5 h-3.5" />
                        </button>
                        <button
                          type="button"
                          disabled={disabled}
                          title="Remove"
                          onClick={() => setDeleteTarget(c)}
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
            className="w-full max-w-2xl max-h-[90dvh] overflow-y-auto rounded-2xl border border-white/10 bg-[var(--apple-surface)] p-6 shadow-xl"
            role="dialog"
            aria-modal="true"
            aria-labelledby="create-container-title"
          >
            <div className="flex items-center justify-between mb-4">
              <h2 id="create-container-title" className="text-lg font-semibold text-[var(--text-primary)]">
                Create container
              </h2>
              <button
                type="button"
                onClick={() => setShowCreate(false)}
                className="p-1 rounded-lg hover:bg-white/10 text-[var(--text-muted)]"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <div className="space-y-3">
              <div>
                <label className="block text-sm text-[var(--text-muted)] mb-1" htmlFor="ctr-name">
                  Name
                </label>
                <input
                  id="ctr-name"
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                  className="w-full rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-surface)] px-3 py-2 text-[var(--text-primary)]"
                  placeholder="my-app"
                  autoFocus
                />
              </div>
              <div>
                <label className="block text-sm text-[var(--text-muted)] mb-1" htmlFor="ctr-image">
                  Image
                </label>
                <input
                  id="ctr-image"
                  value={newImage}
                  onChange={(e) => setNewImage(e.target.value)}
                  className="w-full rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-surface)] px-3 py-2 text-[var(--text-primary)] font-mono text-sm"
                  placeholder="docker.io/library/nginx:alpine"
                />
              </div>
              <div>
                <label className="block text-sm text-[var(--text-muted)] mb-1" htmlFor="ctr-cmd">
                  Command <span className="text-[var(--text-muted)]">(optional)</span>
                </label>
                <input
                  id="ctr-cmd"
                  value={newCommand}
                  onChange={(e) => setNewCommand(e.target.value)}
                  className="w-full rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-surface)] px-3 py-2 text-[var(--text-primary)] font-mono text-sm"
                  placeholder="nginx -g daemon off;"
                />
              </div>
              <label className="flex items-center gap-2 text-sm text-[var(--text-secondary)]">
                <input
                  type="checkbox"
                  checked={startAfterCreate}
                  onChange={(e) => setStartAfterCreate(e.target.checked)}
                  className="rounded border-[var(--apple-hairline)]"
                />
                Start after create
              </label>
            </div>
            <div className="flex justify-end gap-2 mt-5">
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
        title="Remove container"
        message={
          deleteTarget
            ? `Force-remove container “${deleteTarget.name || shortId(deleteTarget.id)}”?`
            : ''
        }
        confirmLabel="Remove"
        variant="danger"
        onCancel={() => setDeleteTarget(null)}
        onConfirm={() => {
          const t = deleteTarget
          setDeleteTarget(null)
          if (!t) return
          void runAction(t.id, 'Remove', () => removeVesselContainer(t.id, true))
        }}
      />
    </PageLayout>
  )
}
