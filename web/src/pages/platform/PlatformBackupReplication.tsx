// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Cloud } from 'lucide-react'
import OperatingSurfaceLayout from '../../components/platform/OperatingSurfaceLayout'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import PlatformPageChrome, { PlatformBackLink } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import { listBackupTargets, listPlatformHosts, type BackupTarget, type PlatformHost } from '../../api/platform'
import {
  fetchBackupReplica,
  listBackupReplicas,
  runBackupReplication,
  type BackupReplica,
} from '../../api/platformFleetFeatures'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

const mib = (b: number) => `${(b / 1048576).toFixed(1)} MiB`

export default function PlatformBackupReplication() {
  const toast = useToastContext()
  const [replicas, setReplicas] = useState<BackupReplica[]>([])
  const [targets, setTargets] = useState<BackupTarget[]>([])
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [targetId, setTargetId] = useState('')
  const [fetchHost, setFetchHost] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      const [r, t, h] = await Promise.all([listBackupReplicas(), listBackupTargets(), listPlatformHosts()])
      setReplicas(r)
      setTargets(t.filter((x) => x.kind === 's3'))
      setHosts(h)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const run = async () => {
    if (!targetId) return
    setBusy(true)
    try {
      const res = await runBackupReplication(targetId)
      toast.success(`Started ${res.started.length} copy(ies)${res.skipped.length ? `, skipped ${res.skipped.length}` : ''}`)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const fetchBack = async (id: string) => {
    if (!fetchHost) return
    try {
      await fetchBackupReplica(id, fetchHost)
      toast.success('Fetch started')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/backups" label="Backup & Restore" />}
      title="Off-site backup replication"
      subtitle="Backups copied to S3-compatible storage at another site, verified by size and SHA-256 checksum."
      icon={<Cloud className="w-6 h-6 text-[var(--text-muted)]" />}
      loading={loading && replicas.length === 0}
    >
      <OperatingSurfaceLayout testId="platform-backup-replication-page">
        <MacGlassPanel title="Replicate now" subtitle="Copies each VM's newest backup that has no copy on the target yet (up to 10).">
          <div className="flex flex-wrap gap-3 items-center">
            <select className="input text-sm" aria-label="S3 target" value={targetId} onChange={(e) => setTargetId(e.target.value)}>
              <option value="">Choose an S3 target…</option>
              {targets.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
            </select>
            <button type="button" className="btn-primary text-sm" disabled={!targetId || busy} onClick={() => void run()}>
              {busy ? 'Starting…' : 'Replicate newest backups'}
            </button>
          </div>
          {targets.length === 0 && !loading && (
            <p className="text-xs text-[var(--text-muted)] mt-2">No S3 backup target yet. Add one under Backup &amp; Restore. Set auto_replicate in its config to copy new backups automatically.</p>
          )}
        </MacGlassPanel>
        <MacGlassPanel
          title="Copies"
          subtitle={`${replicas.length} replica(s)`}
          action={
            <select className="input text-xs" aria-label="Fetch onto host" value={fetchHost} onChange={(e) => setFetchHost(e.target.value)}>
              <option value="">Fetch onto host…</option>
              {hosts.map((h) => <option key={h.id} value={h.id}>{h.hostname}</option>)}
            </select>
          }
        >
          {replicas.length === 0 && !loading ? (
            <PlatformEmptyState icon={Cloud} title="No copies yet" subtitle="Replicate a backup to see it here." />
          ) : (
            <ul className="divide-y divide-white/[0.04] -mx-1">
              {replicas.map((r) => (
                <MacListRow
                  key={r.id}
                  title={r.remote_uri || r.backup_id}
                  subtitle={`${r.status}${r.status === 'verified' ? ` · ${mib(r.bytes)} in ${r.files} file(s) · ${r.checksummed ? 'checksummed' : 'size-verified only'}` : r.message ? ` · ${r.message}` : ''}${r.fetch_status ? ` · fetch ${r.fetch_status}${r.fetch_path ? ` at ${r.fetch_path}` : ''}` : ''}`}
                  badge={
                    r.status === 'verified' ? (
                      <button type="button" className="btn-secondary text-xs" disabled={!fetchHost} onClick={() => void fetchBack(r.id)}>
                        Fetch
                      </button>
                    ) : undefined
                  }
                />
              ))}
            </ul>
          )}
        </MacGlassPanel>
      </OperatingSurfaceLayout>
    </PlatformPageChrome>
  )
}
