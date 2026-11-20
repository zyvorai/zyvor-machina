// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Cloud, Loader2, RefreshCw } from 'lucide-react'
import { getVmGuestObservability, type GuestObservabilitySnapshot } from '../../api/platform'
import { formatUserError } from '../../utils/apiError'
import { statusPillClasses } from '../../utils/semanticColors'

type Props = {
  vmId: string
  className?: string
  /** Prefill from guest health so the strip is not empty on first paint. */
  initial?: GuestObservabilitySnapshot | null
}

export default function GuestObservabilityStrip({ vmId, className = '', initial }: Props) {
  const [obs, setObs] = useState<GuestObservabilitySnapshot | null>(initial ?? null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (initial) setObs(initial)
  }, [initial])

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const r = await getVmGuestObservability(vmId)
      setObs(r as GuestObservabilitySnapshot)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [vmId])

  useEffect(() => {
    void refresh()
  }, [refresh])

  const ipv4 =
    obs?.ip_addresses?.filter((a) => a.ip_type !== 'ipv6' && !a.address.startsWith('127.')) ?? []

  return (
    <div className={`rounded-xl border border-white/[0.06] bg-[var(--apple-surface)] p-3 text-sm space-y-2 ${className}`}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)] flex items-center gap-1">
          <Cloud className="w-3.5 h-3.5" /> Live guest observability
        </p>
        <button
          type="button"
          className="btn-secondary text-xs inline-flex items-center gap-1"
          disabled={loading}
          aria-label="Refresh guest observability"
          onClick={() => void refresh()}
        >
          {loading ? <Loader2 className="w-3 h-3 animate-spin" /> : <RefreshCw className="w-3 h-3" />}
          Refresh
        </button>
      </div>
      {error && <p className="text-xs text-amber-600/90">{error}</p>}
      {!obs && !error && loading && (
        <p className="text-xs text-[var(--text-muted)] flex items-center gap-2">
          <Loader2 className="w-3 h-3 animate-spin" /> Pulling guest agent snapshot…
        </p>
      )}
      {!obs && !error && !loading && (
        <p className="text-xs text-[var(--text-muted)]">
          Guest agent did not return observability data yet. Refresh after the agent is active.
        </p>
      )}
      {obs && (
        <dl className="grid gap-2 text-xs sm:grid-cols-2">
          {(obs.os_pretty_name || obs.os_kernel) && (
            <div className="sm:col-span-2">
              <dt className="text-[var(--text-muted)]">Guest OS</dt>
              <dd className="text-[var(--text-primary)]">
                {[obs.os_pretty_name, obs.os_kernel, obs.os_arch].filter(Boolean).join(' · ')}
              </dd>
            </div>
          )}
          {obs.cloud_init_status && (
            <div>
              <dt className="text-[var(--text-muted)]">Cloud-init</dt>
              <dd className="text-[var(--text-primary)] font-mono">{obs.cloud_init_status}</dd>
            </div>
          )}
          {obs.hostname && (
            <div>
              <dt className="text-[var(--text-muted)]">Hostname</dt>
              <dd className="text-[var(--text-primary)] font-mono">{obs.hostname}</dd>
            </div>
          )}
          {obs.time && (
            <div>
              <dt className="text-[var(--text-muted)]">Clock skew</dt>
              <dd className="text-[var(--text-primary)] font-mono">
                {Math.abs(obs.time.delta_ms) < 1000
                  ? `${obs.time.delta_ms} ms`
                  : `${(obs.time.delta_ms / 1000).toFixed(1)} s`}
              </dd>
            </div>
          )}
          {(obs.users?.length ?? 0) > 0 && (
            <div className="sm:col-span-2">
              <dt className="text-[var(--text-muted)] mb-1">Sessions</dt>
              <dd className="flex flex-wrap gap-1">
                {obs.users!.slice(0, 8).map((u) => (
                  <span key={`${u.username}-${u.login_time ?? ''}`} className={statusPillClasses('neutral')}>
                    {u.username}
                  </span>
                ))}
              </dd>
            </div>
          )}
          {ipv4.length > 0 && (
            <div className="sm:col-span-2">
              <dt className="text-[var(--text-muted)] mb-1">Internal IPs</dt>
              <dd className="flex flex-wrap gap-1 font-mono text-[var(--text-secondary)]">
                {ipv4.slice(0, 6).map((a) => (
                  <span key={`${a.name}-${a.address}`} className={statusPillClasses('neutral')}>
                    {a.address}
                    <span className="text-[var(--text-muted)]"> · {a.name}</span>
                  </span>
                ))}
              </dd>
            </div>
          )}
          {(obs.filesystems?.length ?? 0) > 0 && (
            <div className="sm:col-span-2">
              <dt className="text-[var(--text-muted)] mb-1">Filesystems</dt>
              <dd className="flex flex-wrap gap-1">
                {obs.filesystems!.slice(0, 6).map((fs) => {
                  const pct = fs.total_bytes > 0 ? Math.round((fs.used_bytes / fs.total_bytes) * 100) : 0
                  return (
                    <span key={fs.mountpoint} className={statusPillClasses(pct > 90 ? 'warn' : 'neutral')}>
                      {fs.mountpoint} {pct}%
                    </span>
                  )
                })}
              </dd>
            </div>
          )}
          {obs.fs_freeze?.frozen && (
            <div className="sm:col-span-2">
              <dt className="text-[var(--text-muted)]">Filesystem freeze</dt>
              <dd className="text-amber-700/90">{obs.fs_freeze.detail}</dd>
            </div>
          )}
        </dl>
      )}
    </div>
  )
}
