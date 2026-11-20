// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link } from 'react-router'
import { Monitor, Server, ShieldCheck } from 'lucide-react'
import type { FleetLinuxHostItem, PlatformHost } from '../../../api/platform'
import { statusPillClasses } from '../../../utils/semanticColors'
import { formatAge, heartbeatAgeSecs, hostAttention, isControllerHost } from '../../../utils/hostAttention'
import RingGauge from '../../kit/RingGauge'

type Props = {
  host: PlatformHost
  linux?: FleetLinuxHostItem
  selected?: boolean
  onSelect?: () => void
  /** Injected for tests; defaults to the current time. */
  now?: number
}

const TRANSPORT_LABEL = { mtls: 'mTLS', token: 'Token', plaintext: 'Plain' } as const

export function memoryPercent(host: Pick<PlatformHost, 'memory_used_mib' | 'memory_total_mib'>): number | null {
  return host.memory_total_mib && host.memory_total_mib > 0
    ? Math.round(((host.memory_used_mib ?? 0) / host.memory_total_mib) * 100)
    : null
}

/** One machine: who it is, whether it is well, and how loaded. Every field is real data. */
export default function HostFleetCard({ host, linux, selected, onSelect, now }: Props) {
  const attention = hostAttention(host, now)
  const worst = attention.find((a) => a.severity !== 'info')
  const linuxPressure = linux?.status === 'pressure' || linux?.status === 'thermal'
  const tone = worst?.severity === 'error' ? 'error' : worst || linuxPressure ? 'warn' : host.maintenance_mode ? 'info' : 'ok'
  const label = worst?.severity === 'error'
    ? (host.state === 'online' ? 'Needs attention' : host.state)
    : host.maintenance_mode ? 'Maintenance' : worst ? 'Needs attention' : linuxPressure ? 'Under pressure' : 'Healthy'
  const age = formatAge(heartbeatAgeSecs(host.last_heartbeat_at, now))
  const validation = host.validation_status || 'pending'
  const role = isControllerHost(host) ? 'Controller' : 'Node'

  return (
    <article
      className={`mc-host-fleet-card rounded-2xl border p-5 transition-all duration-200 cursor-pointer hover:-translate-y-0.5 ${
        selected ? 'border-[var(--accent)]/50 bg-[var(--accent)]/5' : 'border-[var(--apple-hairline)] bg-[var(--apple-surface)] hover:bg-[var(--apple-fill-tertiary)]/40'
      }`}
      onClick={onSelect}
      data-testid={`host-fleet-card-${host.id}`}
      aria-selected={selected ? true : undefined}
    >
      <header className="flex items-start justify-between gap-2 mb-3">
        <div className="min-w-0">
          <h2 className="font-semibold text-[var(--text-primary)] flex items-center gap-2 m-0">
            <span className="grid place-items-center w-8 h-8 shrink-0 rounded-xl bg-[var(--accent)] text-white shadow-sm"><Server className="w-4 h-4" /></span>
            <span className="truncate">{host.hostname}</span>
          </h2>
          <p className="text-xs mt-1 font-mono text-[var(--accent)]" data-testid="host-card-ip">{host.address || '—'}</p>
        </div>
        <span className={statusPillClasses(tone)} data-testid="host-card-state">{label}</span>
      </header>

      <div className="flex flex-wrap items-center gap-1.5 mb-4 text-[11px]">
        <span className="rounded-md border border-[var(--apple-hairline)] px-1.5 py-0.5 text-[var(--text-secondary)]" data-testid="host-card-role">{role}</span>
        <span className="text-[var(--text-muted)]" data-testid="host-card-heartbeat">Heartbeat {age}</span>
        <span className={`rounded-md border px-1.5 py-0.5 ${statusPillClasses(validation === 'passed' ? 'ok' : validation === 'failed' ? 'error' : 'neutral')}`} data-testid="host-card-validation">
          {validation === 'passed' ? 'Validated' : validation === 'failed' ? 'Validation failed' : 'Validation pending'}
        </span>
        {host.transport ? (
          <span className="inline-flex items-center gap-1 rounded-md border border-[var(--apple-hairline)] px-1.5 py-0.5 text-[var(--text-secondary)]" data-testid="host-card-transport">
            <ShieldCheck className="w-3 h-3" /> {TRANSPORT_LABEL[host.transport]}
          </span>
        ) : null}
        {host.agent_version ? <span className="text-[var(--text-muted)]" data-testid="host-card-agent">agent {host.agent_version}</span> : null}
      </div>

      <div className="grid grid-cols-3 items-center gap-2 mb-4">
        <div className="text-center">
          <p className="m-0 text-3xl font-semibold tracking-tight tabular-nums text-[var(--text-primary)]">{host.vm_count}</p>
          <p className="m-0 text-xs text-[var(--text-muted)]">VM{host.vm_count === 1 ? '' : 's'}</p>
        </div>
        <RingGauge value={host.state === 'online' ? Math.round(host.cpu_percent ?? 0) : null} label="CPU" size={64} />
        <RingGauge value={host.state === 'online' ? memoryPercent(host) : null} label="Memory" size={64} />
      </div>

      {(host.site || host.rack || (host.tags && host.tags.length > 0)) ? (
        <div className="flex flex-wrap gap-1.5 mb-3 text-[11px]" data-testid="host-card-labels">
          {host.site ? <span className="rounded-full bg-[var(--apple-fill-tertiary)] px-2 py-0.5 text-[var(--text-secondary)]">{host.site}{host.rack ? ` / ${host.rack}` : ''}</span> : null}
          {!host.site && host.rack ? <span className="rounded-full bg-[var(--apple-fill-tertiary)] px-2 py-0.5 text-[var(--text-secondary)]">rack {host.rack}</span> : null}
          {(host.tags ?? []).slice(0, 4).map((t) => <span key={t} className="rounded-full bg-[var(--accent-soft)] px-2 py-0.5 text-[var(--accent)]">{t}</span>)}
        </div>
      ) : null}

      <footer className="flex flex-wrap gap-2" onClick={(e) => e.stopPropagation()}>
        <Link to={`/platform/hosts/${host.id}`} className="btn-secondary text-xs">Open host</Link>
        <Link to={`/platform/vms?lens=topology&host=${encodeURIComponent(host.id)}`} className="btn-secondary text-xs inline-flex items-center gap-1"><Monitor className="w-3 h-3" /> Machines</Link>
      </footer>
    </article>
  )
}
