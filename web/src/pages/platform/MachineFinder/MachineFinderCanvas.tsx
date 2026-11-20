// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import PlatformEmptyState from '../../../components/platform/PlatformEmptyState'
import { openCenterPopout } from '../../../utils/platformCenterPopout'
import { cinemaPopoutPath } from '../../../utils/consoleExperienceMode'
import { groupVmsBySource } from './groupVmsBySource'
import LivingMachineCard from './LivingMachineCard'
import MachineFinderHeatmapLens from './MachineFinderHeatmapLens'
import MachineFinderTableLens from './MachineFinderTableLens'
import MachineFinderTimelineLens from './MachineFinderTimelineLens'
import MachineFinderTopologyLens from './MachineFinderTopologyLens'
import MigrationHeroZone from './MigrationHeroZone'
import type { MachineFinderState } from './useMachineFinder'

type Props = {
  state: MachineFinderState
}

export default function MachineFinderCanvas({ state }: Props) {
  const {
    lens,
    overlay,
    filteredVms,
    hostMap,
    setSelectedVmId,
    setDragVmId,
    setSshVm,
    error,
    folder,
    pruneBusy,
    pruneMissing,
  } = state

  if (lens === 'topology') return <MachineFinderTopologyLens state={state} />
  if (lens === 'table') return <MachineFinderTableLens state={state} />
  if (lens === 'timeline') return <MachineFinderTimelineLens />
  if (lens === 'heatmap') return <MachineFinderHeatmapLens />
  if (lens === 'migration') {
    return (
      <div className="space-y-4">
        <MigrationHeroZone state={state} />
        <GridLens state={state} compact />
      </div>
    )
  }

  return (
    <>
    <div className="machine-finder-canvas space-y-4 min-h-[40vh]" data-testid="machine-finder-canvas">
      {folder === 'missing' && filteredVms.length > 0 && (
        <div className="rounded-xl border border-amber-500/30 bg-amber-500/10 p-4 text-sm text-amber-800">
          <p className="font-medium">Missing from hypervisor inventory</p>
          <p className="text-xs text-amber-700/80 mt-1">Prune removes stale database rows (admin only).</p>
          <button type="button" className="btn-destructive text-sm mt-2" disabled={pruneBusy} onClick={() => pruneMissing()}>
            {pruneBusy ? 'Pruning…' : 'Prune missing records'}
          </button>
        </div>
      )}

      <GridLens state={state} />

      {!error && filteredVms.length === 0 && (
        <PlatformEmptyState title="No machines" subtitle="Try another smart folder or create a VM.">
          <button type="button" className="btn-primary text-sm mt-3" onClick={() => state.setWizardOpen(true)}>New VM</button>
        </PlatformEmptyState>
      )}

      <MigrationHeroZone state={state} compact />
    </div>
    </>
  )
}

function GridLens({ state, compact }: { state: MachineFinderState; compact?: boolean }) {
  const groups = groupVmsBySource(state.filteredVms, state.hostMap)

  if (groups.length === 0) return null

  return (
    <div className="space-y-6">
      {groups.map((group) => (
        <section key={group.key}>
          <h2 className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)] mb-3">{group.label}</h2>
          {/* `grid-cols-N` resolves to `repeat(N, minmax(0, 1fr))`, which lets a
              track shrink to 0 whenever fixed-width siblings (smart-folders
              sidebar, command center) leave little room for `main`. The fixed
              icon (w-20/w-24) then overflows its cell while the name <p>
              (a normal block box) collapses to that same sliver, making
              `truncate` clip almost the whole label. `auto-fill`/`minmax`
              gives each card a legible floor width and just wraps to fewer
              columns instead of shrinking below it. */}
          <div className={`grid gap-4 nl-stagger ${compact ? 'grid-cols-[repeat(auto-fill,minmax(11rem,1fr))]' : 'grid-cols-[repeat(auto-fill,minmax(13rem,1fr))]'}`}>
            {group.vms.map((vm) => (
              <LivingMachineCard
                key={vm.id}
                vm={vm}
                selected={state.selectedVmId === vm.id}
                overlay={state.overlay}
                onSelect={() => state.setSelectedVmId(vm.id)}
                onDragStart={() => state.setDragVmId(vm.id)}
                onDragEnd={() => { state.setDragVmId(null); state.setDropHost(null) }}
                onSsh={() => state.setSshVm(vm)}
                guestIp={state.displayGuestIp(vm)}
                onDoubleClickTheatre={() => openCenterPopout(cinemaPopoutPath(vm.id))}
                aiSecurity={state.aiSecurity}
              />
            ))}
          </div>
        </section>
      ))}
    </div>
  )
}
