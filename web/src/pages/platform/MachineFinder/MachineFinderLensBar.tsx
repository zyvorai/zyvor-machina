// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { MachineFinderLens, MachineFinderOverlay } from './machineFinderTypes'
import type { MachineFinderState } from './useMachineFinder'

const LENSES: { id: MachineFinderLens; label: string }[] = [
  { id: 'grid', label: 'Grid' },
  { id: 'gallery', label: 'Gallery' },
  { id: 'table', label: 'Table' },
  { id: 'topology', label: 'Topology' },
  { id: 'timeline', label: 'Timeline' },
  { id: 'heatmap', label: 'Heatmap' },
  { id: 'migration', label: 'Migration' },
]

const OVERLAYS: { id: MachineFinderOverlay; label: string }[] = [
  { id: 'default', label: 'Default' },
  { id: 'health', label: 'Health' },
  { id: 'backup', label: 'Backup' },
  { id: 'network', label: 'Network' },
  { id: 'security', label: 'Security' },
  { id: 'cost', label: 'Cost' },
  { id: 'migration', label: 'Migration' },
]

type Props = {
  state: MachineFinderState
}

export default function MachineFinderLensBar({ state }: Props) {
  const { lens, overlay, setLens, setOverlay, hasGpuVms } = state

  const overlays: { id: MachineFinderOverlay; label: string }[] = hasGpuVms
    ? [...OVERLAYS, { id: 'gpu', label: 'GPU' }]
    : OVERLAYS

  return (
    <div className="machine-finder-lens-bar flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between" data-testid="machine-finder-lens-bar">
      <div className="flex flex-wrap gap-1">
        {LENSES.map((l) => (
          <button
            key={l.id}
            type="button"
            className={`px-2.5 py-1 rounded-lg text-xs font-medium transition ${
              lens === l.id
                ? 'bg-[var(--plasma-tint)] text-[var(--plasma-strong)] ring-1 ring-[color-mix(in_srgb,var(--plasma)_35%,transparent)]'
                : 'text-[var(--text-muted)] hover:bg-[var(--surface-hover)]'
            }`}
            onClick={() => setLens(l.id)}
          >
            {l.label}
          </button>
        ))}
      </div>
      {(lens === 'grid' || lens === 'migration') && (
        <div className="flex flex-wrap gap-1">
          {overlays.map((o) => (
            <button
              key={o.id}
              type="button"
              className={`px-2 py-0.5 max-sm:min-h-9 max-sm:px-3 rounded text-xs uppercase tracking-wide ${
                overlay === o.id
                  ? 'bg-[var(--verdant-tint)] text-[var(--verdant)] ring-1 ring-[color-mix(in_srgb,var(--verdant)_30%,transparent)]'
                  : 'text-[var(--text-faint)] hover:text-[var(--text-secondary)]'
              }`}
              onClick={() => setOverlay(o.id)}
            >
              {o.label}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
