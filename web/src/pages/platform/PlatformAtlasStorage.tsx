// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Atlas — Zyvor storage control plane console. Browse backends/volumes/
// snapshots/backups provisioned through Atlas and drive the write path.

import { useCallback, useEffect, useState } from 'react'
import {
  Database,
  Loader2,
  Plus,
  RefreshCw,
  Trash2,
} from 'lucide-react'
import ConfirmDialog from '../../components/ConfirmDialog'
import PlatformPageChrome, { platformStatSubtitle } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel, MacSegmentedControl } from '../../components/platform/mac/PlatformMacUi'
import { TahoeTableWrap } from '../../components/platform/tahoe/TahoeListKit'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import {
  backupAtlasVolume,
  createAtlasVolume,
  deleteAtlasBackup,
  deleteAtlasSnapshot,
  deleteAtlasVolume,
  getAtlasStatus,
  listAtlasBackends,
  listAtlasBackups,
  listAtlasJobs,
  listAtlasSnapshots,
  listAtlasVolumes,
  restoreAtlasSnapshot,
  snapshotAtlasVolume,
  type AtlasBackend,
  type AtlasBackup,
  type AtlasJob,
  type AtlasSnapshot,
  type AtlasStatus,
  type AtlasVolume,
} from '../../api/platformAtlas'

const TABS = ['Backends', 'Volumes', 'Snapshots', 'Backups', 'Jobs'] as const
type Tab = (typeof TABS)[number]

function fmtBytes(n?: number | null): string {
  if (n == null) return '—'
  const gib = n / (1024 * 1024 * 1024)
  if (gib >= 1) return `${gib.toFixed(gib >= 10 ? 0 : 1)} GiB`
  const mib = n / (1024 * 1024)
  return `${mib.toFixed(0)} MiB`
}

function StatePill({ value }: { value?: string | null }) {
  const v = (value || 'unknown').toLowerCase()
  const tone =
    v === 'available' || v === 'ready' || v === 'bound' || v === 'verified' || v === 'succeeded'
      ? 'text-emerald-400 bg-emerald-500/10'
      : v === 'failed'
        ? 'text-rose-400 bg-rose-500/10'
        : 'text-amber-400 bg-amber-500/10'
  return <span className={`text-xs px-2 py-0.5 rounded-full ${tone}`}>{value || 'unknown'}</span>
}

export default function PlatformAtlasStorage() {
  const toast = useToastContext()
  const [tab, setTab] = useState<Tab>('Volumes')
  const [status, setStatus] = useState<AtlasStatus | null>(null)
  const [backends, setBackends] = useState<AtlasBackend[]>([])
  const [volumes, setVolumes] = useState<AtlasVolume[]>([])
  const [snapshots, setSnapshots] = useState<AtlasSnapshot[]>([])
  const [backups, setBackups] = useState<AtlasBackup[]>([])
  const [jobs, setJobs] = useState<AtlasJob[]>([])
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState<string | null>(null)
  const [pendingDeleteVolume, setPendingDeleteVolume] = useState<AtlasVolume | null>(null)
  const [pendingDeleteSnapshot, setPendingDeleteSnapshot] = useState<AtlasSnapshot | null>(null)
  const [pendingDeleteBackup, setPendingDeleteBackup] = useState<AtlasBackup | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const st = await getAtlasStatus()
      setStatus(st)
      if (!st.enabled || !st.reachable) return
      const [b, v, s, bk, j] = await Promise.all([
        listAtlasBackends().catch(() => []),
        listAtlasVolumes().catch(() => []),
        listAtlasSnapshots().catch(() => []),
        listAtlasBackups().catch(() => []),
        listAtlasJobs().catch(() => []),
      ])
      setBackends(b)
      setVolumes(v)
      setSnapshots(s)
      setBackups(bk)
      setJobs(j)
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => {
    void load()
  }, [load])

  const run = async (key: string, fn: () => Promise<unknown>, ok: string) => {
    setBusy(key)
    try {
      await fn()
      toast.success(ok)
      await load()
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(null)
    }
  }

  const onCreateVolume = () => {
    const name = window.prompt('New volume name')
    if (!name) return
    const sizeGib = Number(window.prompt('Size (GiB)', '10'))
    if (!sizeGib || sizeGib <= 0) return
    void run(
      'create',
      () => createAtlasVolume({ name, size_bytes: Math.round(sizeGib * 1024 * 1024 * 1024) }),
      `Volume ${name} creating`,
    )
  }

  const disabled = status && !status.enabled
  const unreachable = status && status.enabled && !status.reachable

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      title="Storage (Atlas)"
      subtitle={
        status?.enabled && status.reachable
          ? platformStatSubtitle([
              { label: 'Backends', value: String(backends.length) },
              { label: 'Volumes', value: String(volumes.length) },
              { label: 'Snapshots', value: String(snapshots.length) },
              { label: 'Backups', value: String(backups.length) },
              { label: 'Auth', value: status?.authenticated ? 'JWT' : 'open' },
            ])
          : (status?.summary ?? 'Zyvor storage control plane')
      }
      icon={<Database className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={
        <div className="flex items-center gap-2">
          <button type="button" className="btn-secondary text-xs" onClick={() => void load()} aria-label="Refresh">
            <RefreshCw className="w-4 h-4" />
          </button>
          <button
            type="button"
            className="btn-primary text-sm inline-flex items-center gap-1.5"
            onClick={onCreateVolume}
            disabled={!!disabled || !!unreachable || busy === 'create'}
          >
            <Plus className="w-4 h-4" /> New volume
          </button>
        </div>
      }
    >
      {loading ? (
        <div className="flex items-center justify-center py-20 text-[var(--text-muted)]">
          <Loader2 className="w-5 h-5 animate-spin mr-2" /> Loading Atlas…
        </div>
      ) : disabled ? (
        <MacGlassPanel title="Atlas disabled">
          <p className="text-sm text-[var(--text-muted)]">
            The Atlas storage integration is disabled. Set <code>ATLAS_ENABLED=1</code> and{' '}
            <code>ATLAS_BASE_URL</code> on the controller to connect a Zyvor storage control plane.
          </p>
        </MacGlassPanel>
      ) : unreachable ? (
        <MacGlassPanel title="Atlas unreachable">
          <p className="text-sm text-[var(--text-muted)]">
            Could not reach the Atlas gateway at <code>{status?.base_url}</code>. Check the gateway and
            <code> ATLAS_BASE_URL</code>.
          </p>
        </MacGlassPanel>
      ) : (
        <div className="space-y-4">
          <MacSegmentedControl
            options={TABS.map((t) => ({ value: t, label: t }))}
            value={tab}
            onChange={setTab}
          />

          {tab === 'Backends' && (
            <MacGlassPanel title="Storage backends" subtitle="Registered Atlas drivers (Ceph / NFS / ZFS).">
              <div className="divide-y divide-white/[0.04]">
                {backends.length === 0 && <p className="text-sm text-[var(--text-muted)] py-4">No backends registered.</p>}
                {backends.map((b) => (
                  <div key={b.id} className="flex items-center justify-between py-3">
                    <div>
                      <p className="text-sm text-[var(--text-primary)]">{b.name}</p>
                      <p className="text-xs text-[var(--text-muted)]">
                        {b.backend_type} · {b.mode}
                      </p>
                    </div>
                    <StatePill value={b.status} />
                  </div>
                ))}
              </div>
            </MacGlassPanel>
          )}

          {tab === 'Volumes' && (
            <MacGlassPanel title="Volumes" subtitle="Backend volumes provisioned through Atlas.">
              {volumes.length === 0 ? (
                <p className="text-sm text-[var(--text-muted)] py-4">No volumes.</p>
              ) : (
                <TahoeTableWrap>
                  <table className="w-full text-sm" aria-label="Atlas volumes">
                    <thead>
                      <tr className="text-left text-[var(--text-muted)] border-b border-white/[0.06]">
                        <th scope="col" className="py-2 pr-2">Name</th>
                        <th scope="col" className="py-2 pr-2">Size</th>
                        <th scope="col" className="py-2 pr-2">State</th>
                        <th scope="col" className="py-2 text-right">Actions</th>
                      </tr>
                    </thead>
                    <tbody>
                      {volumes.map((v) => (
                        <tr key={v.id} className="border-b border-white/[0.04]">
                          <td className="py-3 pr-2">
                            <p className="text-[var(--text-primary)] truncate">{v.name}</p>
                            <p className="text-xs text-[var(--text-muted)] truncate">{v.backend_native_id ?? v.id}</p>
                          </td>
                          <td className="py-3 pr-2 text-[var(--text-muted)]">{fmtBytes(v.size_bytes)}</td>
                          <td className="py-3 pr-2"><StatePill value={v.state} /></td>
                          <td className="py-3 text-right">
                            <div className="flex items-center justify-end gap-2 shrink-0">
                              <button
                                type="button"
                                className="btn-secondary text-xs"
                                disabled={busy === v.id}
                                onClick={() =>
                                  void run(v.id, () => snapshotAtlasVolume(v.id), `Snapshot of ${v.name} queued`)
                                }
                              >
                                Snapshot
                              </button>
                              <button
                                type="button"
                                className="btn-secondary text-xs"
                                disabled={busy === v.id}
                                onClick={() =>
                                  void run(v.id, () => backupAtlasVolume({ volume_id: v.id }), `Backup of ${v.name} queued`)
                                }
                              >
                                Backup
                              </button>
                              <button
                                type="button"
                                className="btn-secondary text-xs text-red-600"
                                disabled={busy === v.id}
                                aria-label="Delete volume"
                                onClick={() => setPendingDeleteVolume(v)}
                              >
                                <Trash2 className="w-3.5 h-3.5" />
                              </button>
                            </div>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </TahoeTableWrap>
              )}
            </MacGlassPanel>
          )}

          {tab === 'Snapshots' && (
            <MacGlassPanel title="Snapshots" subtitle="Point-in-time snapshots of Atlas volumes.">
              {snapshots.length === 0 ? (
                <p className="text-sm text-[var(--text-muted)] py-4">No snapshots.</p>
              ) : (
                <TahoeTableWrap>
                  <table className="w-full text-sm" aria-label="Atlas snapshots">
                    <thead>
                      <tr className="text-left text-[var(--text-muted)] border-b border-white/[0.06]">
                        <th scope="col" className="py-2 pr-2">Name</th>
                        <th scope="col" className="py-2 pr-2">Volume</th>
                        <th scope="col" className="py-2 pr-2">State</th>
                        <th scope="col" className="py-2 text-right">Actions</th>
                      </tr>
                    </thead>
                    <tbody>
                      {snapshots.map((s) => (
                        <tr key={s.id} className="border-b border-white/[0.04]">
                          <td className="py-3 pr-2 text-[var(--text-primary)] truncate">{s.name}</td>
                          <td className="py-3 pr-2 text-xs text-[var(--text-muted)] truncate">{s.volume_id}</td>
                          <td className="py-3 pr-2"><StatePill value={s.state} /></td>
                          <td className="py-3 text-right">
                            <div className="flex items-center justify-end gap-2 shrink-0">
                              <button
                                type="button"
                                className="btn-secondary text-xs"
                                disabled={busy === s.id}
                                onClick={() =>
                                  void run(s.id, () => restoreAtlasSnapshot(s.id), `Restore from ${s.name} queued`)
                                }
                              >
                                Restore
                              </button>
                              <button
                                type="button"
                                className="btn-secondary text-xs text-red-600"
                                disabled={busy === s.id}
                                aria-label="Delete snapshot"
                                onClick={() => setPendingDeleteSnapshot(s)}
                              >
                                <Trash2 className="w-3.5 h-3.5" />
                              </button>
                            </div>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </TahoeTableWrap>
              )}
            </MacGlassPanel>
          )}

          {tab === 'Backups' && (
            <MacGlassPanel title="Backups" subtitle="Volume backups written to Atlas RGW buckets (S3).">
              <div className="divide-y divide-white/[0.04]">
                {backups.length === 0 && <p className="text-sm text-[var(--text-muted)] py-4">No backups.</p>}
                {backups.map((b) => (
                  <div key={b.id} className="flex items-center justify-between py-3 gap-3">
                    <div className="min-w-0">
                      <p className="text-sm text-[var(--text-primary)] truncate">{b.object_key ?? b.id}</p>
                      <p className="text-xs text-[var(--text-muted)] truncate">
                        vol {b.volume_id} · {b.format ?? 'manifest'}
                      </p>
                    </div>
                    <div className="flex items-center gap-2 shrink-0">
                      <StatePill value={b.state} />
                      <button
                        type="button"
                        className="btn-secondary text-xs text-red-600"
                        disabled={busy === b.id}
                        aria-label="Delete backup"
                        onClick={() => setPendingDeleteBackup(b)}
                      >
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            </MacGlassPanel>
          )}

          {tab === 'Jobs' && (
            <MacGlassPanel title="Jobs" subtitle="Async Atlas operations (create, snapshot, backup, restore).">
              <div className="divide-y divide-white/[0.04]">
                {jobs.length === 0 && <p className="text-sm text-[var(--text-muted)] py-4">No recent jobs.</p>}
                {jobs.map((j) => {
                  const id = j.id ?? j.job_id ?? '?'
                  return (
                    <div key={id} className="flex items-center justify-between py-3 gap-3">
                      <div className="min-w-0">
                        <p className="text-sm text-[var(--text-primary)] truncate">{j.job_type ?? id}</p>
                        {j.error && <p className="text-xs text-rose-400 truncate">{j.error}</p>}
                      </div>
                      <div className="flex items-center gap-2 shrink-0">
                        <span className="text-xs text-[var(--text-muted)]">{j.progress_percent ?? 0}%</span>
                        <StatePill value={j.state} />
                      </div>
                    </div>
                  )
                })}
              </div>
            </MacGlassPanel>
          )}
        </div>
      )}
      <ConfirmDialog
        open={!!pendingDeleteVolume}
        title="Delete volume"
        message={pendingDeleteVolume ? `Delete volume ${pendingDeleteVolume.name}?` : ''}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setPendingDeleteVolume(null)}
        onConfirm={() => {
          if (!pendingDeleteVolume) return
          const target = pendingDeleteVolume
          setPendingDeleteVolume(null)
          void run(target.id, () => deleteAtlasVolume(target.id), `Volume ${target.name} deleting`)
        }}
      />
      <ConfirmDialog
        open={!!pendingDeleteSnapshot}
        title="Delete snapshot"
        message={pendingDeleteSnapshot ? `Delete snapshot ${pendingDeleteSnapshot.name}?` : ''}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setPendingDeleteSnapshot(null)}
        onConfirm={() => {
          if (!pendingDeleteSnapshot) return
          const target = pendingDeleteSnapshot
          setPendingDeleteSnapshot(null)
          void run(target.id, () => deleteAtlasSnapshot(target.id), `Snapshot ${target.name} deleting`)
        }}
      />
      <ConfirmDialog
        open={!!pendingDeleteBackup}
        title="Delete backup"
        message="Delete this backup?"
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setPendingDeleteBackup(null)}
        onConfirm={() => {
          if (!pendingDeleteBackup) return
          const target = pendingDeleteBackup
          setPendingDeleteBackup(null)
          void run(target.id, () => deleteAtlasBackup(target.id), 'Backup deleting')
        }}
      />
    </PlatformPageChrome>
  )
}
