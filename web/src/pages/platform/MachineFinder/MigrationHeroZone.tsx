// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Server } from 'lucide-react'
import type { PlatformHost } from '../../../api/platform'
import type { MachineFinderState } from './useMachineFinder'

function liveVmCountOnHost(hostId: string, vms: MachineFinderState['vms']): number {
  return vms.filter((v) => v.host_id === hostId && v.observed_state !== 'missing').length
}

type Props = {
  state: MachineFinderState
  compact?: boolean
}

export default function MigrationHeroZone({ state, compact }: Props) {
  const { hosts, vms, dropHost, setDropHost, onHostDrop, dragVmId } = state

  if (state.lens !== 'grid' && state.lens !== 'migration') return null

  return (
    <section
      className={`machine-finder-migrate-zone rounded-2xl border border-dashed transition ${
        dragVmId ? 'border-[var(--accent)]/50 bg-[var(--plasma-tint)]' : 'border-[var(--apple-hairline)] bg-[var(--apple-surface)]'
      } ${compact ? 'p-3' : 'p-5'}`}
      data-testid="machine-finder-migrate-zone"
    >
      <div className="flex items-center gap-2 mb-3">
        <Server className="w-4 h-4 text-[var(--link)]" />
        <h3 className="text-sm font-medium text-[var(--text-primary)]">
          {dragVmId ? 'Drop machine on a destination host' : 'Migration zone — drag a machine here'}
        </h3>
      </div>
      <div className={`grid gap-2 ${compact ? 'grid-cols-2 sm:grid-cols-3' : 'grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-6'}`}>
        {hosts.map((h) => (
          <HostDropTarget
            key={h.id}
            host={h}
            vmCount={liveVmCountOnHost(h.id, vms)}
            active={dropHost === h.id}
            onDragOver={() => setDropHost(h.id)}
            onDragLeave={() => setDropHost(null)}
            onDrop={() => onHostDrop(h.id)}
          />
        ))}
      </div>
    </section>
  )
}

function HostDropTarget({
  host,
  vmCount,
  active,
  onDragOver,
  onDragLeave,
  onDrop,
}: {
  host: PlatformHost
  vmCount: number
  active: boolean
  onDragOver: () => void
  onDragLeave: () => void
  onDrop: () => void
}) {
  return (
    <div
      onDragOver={(e) => { e.preventDefault(); onDragOver() }}
      onDragLeave={onDragLeave}
      onDrop={(e) => { e.preventDefault(); onDrop() }}
      className={`rounded-xl border p-3 text-sm transition ${
        active ? 'border-[var(--accent)] bg-[var(--plasma-tint)] scale-[1.02]' : 'border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)]'
      }`}
    >
      <p className="font-medium truncate text-[var(--text-primary)]">{host.hostname}</p>
      <p className="text-xs text-[var(--text-muted)]">{host.state} · {vmCount} VMs</p>
    </div>
  )
}
