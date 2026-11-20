// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, useCallback, useRef } from 'react'
import {
  fetchBackups, triggerBackup, restoreBackup, deleteBackup, verifyBackup,
  getBackupStatus,
  getSchedule, setSchedule, downloadBackupUrl,
  BackupInfo, BackupRequest, VerifyResult, ScheduleInfo, BackupStatus,
} from '../api/backup'
import { listVMs, VmInfo } from '../api/vm'
import { useToastContext } from '../contexts/ToastContext'
import ConfirmDialog from '../components/ConfirmDialog'
import { GlassModal } from '../components/glass/GlassModal'
import PageLayout from '../components/PageLayout'
import EmptyState from '../components/EmptyState'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { formatUserError } from '../utils/apiError'
import { statusBgClass, statusToneClass } from '../utils/semanticColors'
import {
  Archive, Trash2, RotateCcw, RefreshCw, Play, HardDrive, Server,
  Download, ShieldCheck, Clock, ToggleLeft, ToggleRight, CheckCircle,
  XCircle, AlertCircle, Loader2, Layers,
} from 'lucide-react'

function StatusBadge({ status }: { status: string }) {
  switch (status) {
    case 'completed':
      return <span className={`flex items-center gap-1 text-xs ${statusToneClass('ok')}`}><CheckCircle className="w-3 h-3" /> Done</span>
    case 'running':
      return <span className={`flex items-center gap-1 text-xs ${statusToneClass('info')}`}><Loader2 className="w-3 h-3 animate-spin" /> Running</span>
    case 'failed':
      return <span className={`flex items-center gap-1 text-xs ${statusToneClass('error')}`}><XCircle className="w-3 h-3" /> Failed</span>
    default:
      return <span className="flex items-center gap-1 text-[var(--text-muted)] text-xs"><AlertCircle className="w-3 h-3" /> {status}</span>
  }
}

export default function BackupsPage() {
  const [backups, setBackups] = useState<BackupInfo[]>([])
  const [vms, setVms] = useState<VmInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [running, setRunning] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<BackupInfo | null>(null)
  const [restoreTarget, setRestoreTarget] = useState<BackupInfo | null>(null)
  const [verifyResult, setVerifyResult] = useState<VerifyResult | null>(null)
  const [verifying, setVerifying] = useState<string | null>(null)
  const [schedule, setScheduleState] = useState<ScheduleInfo | null>(null)
  const [restoring, setRestoring] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const toast = useToastContext()
  const backupTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // Backup form state
  const [vmName, setVmName] = useState('')
  const [withDisks, setWithDisks] = useState(false)
  const [incremental, setIncremental] = useState(false)
  const [nfsTarget, setNfsTarget] = useState('')
  const [retain, setRetain] = useState(7)
  const [showForm, setShowForm] = useState(false)
  const [statusDetail, setStatusDetail] = useState<Record<string, BackupStatus>>({})
  const [statusBusy, setStatusBusy] = useState<string | null>(null)
  const [search, setSearch] = useState('')

  const load = useCallback(async () => {
    try {
      setLoading(true)
      const [bResult, vResult, sResult] = await Promise.allSettled([fetchBackups(), listVMs(), getSchedule()])
      if (bResult.status === 'fulfilled') setBackups(bResult.value)
      else toast.error(`Failed to load backups: ${formatUserError(bResult.reason)}`)
      if (vResult.status === 'fulfilled') setVms(vResult.value)
      if (sResult.status === 'fulfilled') setScheduleState(sResult.value)
    } catch (e: unknown) {
      toast.error(`${formatUserError(e)}`)
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => { load() }, [load])

  // Cleanup setTimeout on unmount
  useEffect(() => {
    return () => { if (backupTimerRef.current) clearTimeout(backupTimerRef.current) }
  }, [])

  const refreshBackupStatus = useCallback(async (id: string) => {
    setStatusBusy(id)
    try {
      const st = await getBackupStatus(id)
      setStatusDetail((prev) => ({ ...prev, [id]: st }))
      setBackups((prev) => prev.map((b) => (
        b.id === id
          ? { ...b, status: st.status, progress: st.progress.replace(/%$/, ''), status_message: st.message }
          : b
      )))
      return st
    } finally {
      setStatusBusy(null)
    }
  }, [])

  // Poll live status for running backups (GET /backups/{id}/status).
  // Depend on the *set* of running ids (a stable string), not the whole
  // `backups` array — otherwise each status update mutates `backups`, re-runs
  // the effect, and re-fires the poll immediately, bypassing the 3s interval.
  const runningIdsKey = backups
    .filter((b) => b.status === 'running')
    .map((b) => b.id)
    .sort()
    .join(',')
  useEffect(() => {
    if (!runningIdsKey) return
    const ids = runningIdsKey.split(',')
    const poll = () => {
      void Promise.all(ids.map((id) => refreshBackupStatus(id))).catch(() => undefined)
    }
    poll()
    const interval = setInterval(poll, 3000)
    return () => clearInterval(interval)
  }, [runningIdsKey, refreshBackupStatus])

  const handleBackup = async () => {
    setRunning(true)
    toast.info('Backup started...')
    try {
      const req: BackupRequest = { with_disks: withDisks, retain, incremental }
      if (vmName) req.vm_name = vmName
      if (nfsTarget) req.nfs_target = nfsTarget
      const result = await triggerBackup(req)
      toast.success(`Backup triggered successfully: ${result.backup_id}`)
      setShowForm(false)
      if (backupTimerRef.current) clearTimeout(backupTimerRef.current)
      backupTimerRef.current = setTimeout(() => load(), 2000)
    } catch (e: unknown) {
      toast.error(`${formatUserError(e)}`)
    } finally {
      setRunning(false)
    }
  }

  const handleRestore = async () => {
    if (!restoreTarget || restoring) return
    setRestoring(true)
    toast.info('Restore started in background...')
    try {
      await restoreBackup({ backup_id: restoreTarget.id, backend: restoreTarget.backend })
      toast.success(`Restore completed from '${restoreTarget.id}'`)
      load()
    } catch (e: unknown) {
      toast.error(`${formatUserError(e)}`)
    } finally {
      setRestoring(false)
    }
    setRestoreTarget(null)
  }

  const handleDelete = async () => {
    if (!deleteTarget || deleting) return
    setDeleting(true)
    try {
      await deleteBackup(deleteTarget.id, deleteTarget.backend)
      toast.success(`Deleted backup '${deleteTarget.id}'`)
      load()
    } catch (e: unknown) {
      toast.error(`${formatUserError(e)}`)
    } finally {
      setDeleting(false)
    }
    setDeleteTarget(null)
  }

  const handleVerify = async (b: BackupInfo) => {
    setVerifying(b.id)
    try {
      const result = await verifyBackup(b.id)
      setVerifyResult(result)
      if (result.verified) {
        toast.success(`Backup '${b.id}' verified: ${result.files_ok} files OK`)
      } else {
        toast.error(`Backup '${b.id}' verification failed: ${result.files_failed} files`)
      }
    } catch (e: unknown) {
      toast.error(`${formatUserError(e)}`)
    } finally {
      setVerifying(null)
    }
  }

  const handleToggleSchedule = async () => {
    if (!schedule) return
    try {
      await setSchedule(!schedule.enabled)
      toast.success(schedule.enabled ? 'Backup timer disabled' : 'Backup timer enabled')
      load()
    } catch (e: unknown) {
      toast.error(`${formatUserError(e)}`)
    }
  }

  const filteredBackups = backups.filter((b) => {
    const q = search.trim().toLowerCase()
    if (!q) return true
    return (
      b.id.toLowerCase().includes(q)
      || b.status.toLowerCase().includes(q)
      || (b.vm_filter || '').toLowerCase().includes(q)
      || (b.nfs_target || '').toLowerCase().includes(q)
    )
  })

  return (
    <PageLayout
      eyebrow="Hypervisor"
      title="Backups"
      icon={<Archive className="w-6 h-6" />}
      actions={
        <>
          <button onClick={load} className="p-2 hover:bg-[var(--surface-hover)] rounded transition" title="Refresh" aria-label="Refresh"><RefreshCw className="w-4 h-4" /></button>
          <button
            onClick={() => setShowForm(!showForm)}
            className="btn-primary text-sm inline-flex items-center gap-2"
          >
            <Play className="w-4 h-4" />
            New Backup
          </button>
        </>
      }
      contentLoading={loading}
    >

      {/* Schedule card */}
      {schedule && schedule.installed && (
        <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 p-4 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <Clock className="w-5 h-5 text-[var(--text-muted)]" />
            <div>
              <span className="text-sm font-medium">Scheduled Backup</span>
              <span className="text-xs text-[var(--text-muted)] ml-2">Daily at 2:00 AM</span>
              {schedule.next_run && schedule.enabled && (
                <span className="text-xs text-[var(--text-muted)] ml-2">Next: {schedule.next_run}</span>
              )}
            </div>
          </div>
          <button onClick={handleToggleSchedule} className="flex items-center gap-2 text-sm" title={schedule.enabled ? 'Disable timer' : 'Enable timer'}>
            {schedule.enabled ? (
              <><ToggleRight className={`w-6 h-6 ${statusToneClass('ok')}`} /> <span className={statusToneClass('ok')}>Enabled</span></>
            ) : (
              <><ToggleLeft className="w-6 h-6 text-[var(--text-muted)]" /> <span className="text-[var(--text-muted)]">Disabled</span></>
            )}
          </button>
        </div>
      )}

      {/* Backup form */}
      {showForm && (
        <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 p-6 space-y-4">
          <h2 className="text-lg font-semibold">Create Backup</h2>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div>
              <label className="block text-sm text-[var(--text-muted)] mb-1">VM (leave empty for all)</label>
              <select
                aria-label="VM (leave empty for all)"
                value={vmName}
                onChange={(e) => setVmName(e.target.value)}
                className="w-full bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-sm focus:border-[var(--accent)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--accent)_45%,transparent)]"
              >
                <option value="">All VMs</option>
                {vms.map((vm) => (
                  <option key={vm.name} value={vm.name}>{vm.name}</option>
                ))}
              </select>
            </div>
            <div>
              <label className="block text-sm text-[var(--text-muted)] mb-1">NFS Target (optional)</label>
              <input
                aria-label="NFS target (optional)"
                type="text"
                value={nfsTarget}
                onChange={(e) => setNfsTarget(e.target.value)}
                placeholder="192.168.1.100:/backups"
                className="w-full bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-sm focus:border-[var(--accent)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--accent)_45%,transparent)]"
              />
            </div>
            <div>
              <label className="block text-sm text-[var(--text-muted)] mb-1">Retention (keep last N)</label>
              <input
                aria-label="Retention count"
                type="number"
                value={retain}
                onChange={(e) => setRetain(parseInt(e.target.value) || 0)}
                min={0}
                className="w-full bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-sm focus:border-[var(--accent)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--accent)_45%,transparent)]"
              />
            </div>
            <div className="flex flex-col gap-3 pt-4">
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={withDisks}
                  onChange={(e) => { setWithDisks(e.target.checked); if (!e.target.checked) setIncremental(false) }}
                  className="w-4 h-4 rounded bg-[var(--apple-surface)] border-[var(--apple-hairline)] text-[var(--accent)] focus:ring-[var(--machina-status-info)]"
                />
                <span className="text-sm">Include disk images</span>
                {withDisks && <span className={`text-xs ${statusToneClass('warn')}`}>May be very large</span>}
              </label>
              {withDisks && (
                <label className="flex items-center gap-2 cursor-pointer ml-6">
                  <input
                    type="checkbox"
                    checked={incremental}
                    onChange={(e) => setIncremental(e.target.checked)}
                    className="w-4 h-4 rounded bg-[var(--apple-surface)] border-[var(--apple-hairline)] text-[var(--accent)] focus:ring-[var(--machina-status-info)]"
                  />
                  <span className="text-sm flex items-center gap-1"><Layers className="w-3 h-3" /> Incremental (hardlink unchanged)</span>
                </label>
              )}
            </div>
          </div>
          <div className="flex justify-end gap-2 pt-2">
            <button onClick={() => setShowForm(false)} className="px-4 py-2 text-sm text-[var(--text-muted)] hover:text-[var(--text-primary)] transition">Cancel</button>
            <button
              onClick={handleBackup}
              disabled={running}
              className="flex items-center gap-2 btn-primary text-sm disabled:opacity-50"
            >
              {running ? <RefreshCw className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4" />}
              {running ? 'Running...' : 'Start Backup'}
            </button>
          </div>
        </div>
      )}

      <TahoeToolbar
        search={search}
        onSearchChange={setSearch}
        placeholder="Search backups…"
        trailing={
          search ? (
            <span aria-live="polite" className="text-sm text-[var(--text-muted)] shrink-0 pr-2">
              {filteredBackups.length} backup{filteredBackups.length !== 1 ? 's' : ''}
            </span>
          ) : null
        }
      />

      {/* Backup list */}
      {backups.length === 0 ? (
        <EmptyState title="No backups" description="Create a backup to protect your VMs. Backups are stored at the configured target path." />
      ) : (
        <TahoeTableWrap>
          <table className="apple-table" aria-label="Backup jobs">
            <thead>
              <tr>
                <th scope="col">Backup ID</th>
                <th scope="col">Status</th>
                <th scope="col">Scope</th>
                <th scope="col" className="hidden md:table-cell">VMs</th>
                <th scope="col" className="hidden lg:table-cell">Target</th>
                <th scope="col">Disks</th>
                <th scope="col">Size</th>
                <th scope="col" className="text-right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {filteredBackups.length === 0 && (
                <tr><td colSpan={8} className="text-center text-[var(--text-muted)]">No backups match your search.</td></tr>
              )}
              {filteredBackups.map((b) => (
                <tr key={b.id}>
                  <td className="font-medium font-mono text-sm">{b.id}</td>
                  <td>
                    <StatusBadge status={b.status} />
                    {b.status === 'running' && b.progress !== '' && (
                      <div className="mt-1 w-20 bg-[var(--surface-hover)] rounded-full h-1.5">
                        <div className={`h-1.5 rounded-full transition-all ${statusBgClass('info')}`} style={{ width: `${parseInt(b.progress) || 0}%` }} />
                      </div>
                    )}
                    {(statusDetail[b.id]?.message || b.status_message) && (
                      <p className="text-[11px] text-[var(--text-muted)] mt-1 max-w-[12rem] truncate" title={statusDetail[b.id]?.message || b.status_message}>
                        {statusDetail[b.id]?.message || b.status_message}
                      </p>
                    )}
                  </td>
                  <td className="text-sm">
                    {b.vm_filter === 'all' ? (
                      <span className={`flex items-center gap-1 ${statusToneClass('info')}`}><Server className="w-3 h-3" /> All</span>
                    ) : (
                      <span className={`flex items-center gap-1 ${statusToneClass('ok')}`}><Server className="w-3 h-3" /> {b.vm_filter}{b.backend === 'fluxvm' && <span className="text-[10px] text-[var(--text-muted)]"> · FluxVM</span>}</span>
                    )}
                  </td>
                  <td className="text-sm text-[var(--text-muted)] hidden md:table-cell">{b.vm_count}</td>
                  <td className="text-sm text-[var(--text-muted)] hidden lg:table-cell">
                    {b.nfs_target === 'local' ? 'Local' : b.nfs_target}
                  </td>
                  <td className="text-sm">
                    {b.with_disks ? (
                      <span className={`flex items-center gap-1 ${statusToneClass('warn')}`}><HardDrive className="w-3 h-3" /> Yes</span>
                    ) : (
                      <span className="text-[var(--text-muted)]">No</span>
                    )}
                  </td>
                  <td className="text-sm text-[var(--text-muted)]">{b.size}</td>
                  <td>
                    <div className="flex items-center justify-end gap-1">
                      {b.status === 'running' && (
                        <button
                          type="button"
                          data-testid={`backup-status-${b.id}`}
                          onClick={() => void refreshBackupStatus(b.id).catch((e: unknown) => toast.error(formatUserError(e)))}
                          disabled={statusBusy === b.id}
                          className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition"
                          title="Refresh live backup status"
                          aria-label="Refresh live backup status"
                        >
                          {statusBusy === b.id ? (
                            <Loader2 className="w-4 h-4 animate-spin text-[var(--text-secondary)]" />
                          ) : (
                            <RefreshCw className="w-4 h-4 text-[var(--text-secondary)]" />
                          )}
                        </button>
                      )}
                      {b.has_checksums && (
                        <button
                          onClick={() => handleVerify(b)}
                          disabled={verifying === b.id}
                          className="p-1.5 hover:bg-green-600/20 rounded transition"
                          title="Verify checksums"
                          aria-label="Verify checksums"
                        >
                          {verifying === b.id ? (
                            <Loader2 className={`w-4 h-4 animate-spin ${statusToneClass('ok')}`} />
                          ) : (
                            <ShieldCheck className={`w-4 h-4 ${statusToneClass('ok')}`} />
                          )}
                        </button>
                      )}
                      {b.backend !== 'fluxvm' && <a
                        href={downloadBackupUrl(b.id)}
                        className="p-2.5 hover:bg-[var(--accent-soft)] rounded transition"
                        title="Download as tar.gz"
                        aria-label="Download as tar.gz"
                      >
                        <Download className="w-4 h-4 text-[var(--accent)]" />
                      </a>}
                      <button onClick={() => setRestoreTarget(b)} className="p-1.5 hover:bg-white/10 rounded transition" title="Restore" aria-label="Restore">
                        <RotateCcw className={`w-4 h-4 ${statusToneClass('info')}`} />
                      </button>
                      <button onClick={() => setDeleteTarget(b)} className="p-1.5 hover:bg-red-600/20 rounded transition" title="Delete" aria-label="Delete">
                        <Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </TahoeTableWrap>
      )}

      {/* Verify result dialog */}
      {verifyResult && (
        <GlassModal open onClose={() => setVerifyResult(null)} ariaLabel="Backup verification result"
          footer={<button onClick={() => setVerifyResult(null)} className="w-full btn-secondary text-sm transition">Close</button>}>
          <h3 className="text-lg font-semibold mb-4 flex items-center gap-2">
              {verifyResult.verified ? (
                <><CheckCircle className={`w-5 h-5 ${statusToneClass('ok')}`} /> Verification Passed</>
              ) : (
                <><XCircle className={`w-5 h-5 ${statusToneClass('error')}`} /> Verification Failed</>
              )}
          </h3>
            <div className="space-y-2 text-sm">
              <div className="flex justify-between"><span className="text-[var(--text-muted)]">Backup</span><span className="font-mono">{verifyResult.backup_id}</span></div>
              <div className="flex justify-between"><span className="text-[var(--text-muted)]">Files checked</span><span>{verifyResult.files_checked}</span></div>
              <div className="flex justify-between"><span className="text-[var(--text-muted)]">OK</span><span className={statusToneClass('ok')}>{verifyResult.files_ok}</span></div>
              {verifyResult.files_failed > 0 && (
                <>
                  <div className="flex justify-between"><span className="text-[var(--text-muted)]">Failed</span><span className={statusToneClass('error')}>{verifyResult.files_failed}</span></div>
                  <div className={`mt-2 bg-[var(--apple-surface)] rounded p-2 text-xs font-mono max-h-32 overflow-y-auto ${statusToneClass('error')} opacity-80`}>
                    {verifyResult.failed_files.map((f) => <div key={f}>{f}</div>)}
                  </div>
                </>
              )}
            </div>
        </GlassModal>
      )}

      <ConfirmDialog
        open={!!deleteTarget}
        title="Delete Backup"
        message={`Delete backup '${deleteTarget?.id}'? This cannot be undone.`}
        confirmLabel="Delete"
        onConfirm={handleDelete}
        onCancel={() => setDeleteTarget(null)}
      />
      <ConfirmDialog
        open={!!restoreTarget}
        title="Restore Backup"
        message={`Restore VM/network/pool definitions from backup '${restoreTarget?.id}'? Existing definitions with the same name will be updated.`}
        confirmLabel="Restore"
        onConfirm={handleRestore}
        onCancel={() => setRestoreTarget(null)}
      />
    </PageLayout>
  )
}
