// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import RingGauge from '../../../components/kit/RingGauge'
import Reveal from '../../../components/Reveal'
import type { MissionControlFleetState } from './useMissionControlFleet'

/** Capacity at a glance: CPU, memory and storage as ring gauges. */
export default function MissionControlCapacity({ state }: { state: MissionControlFleetState }) {
  const { capacity, loading, error } = state
  if (loading || error || !capacity) return null
  const mem = capacity.memory_total_mib > 0 ? (capacity.memory_used_mib / capacity.memory_total_mib) * 100 : null
  const storage = capacity.storage_capacity_gib && capacity.storage_capacity_gib > 0 ? ((capacity.storage_used_gib ?? 0) / capacity.storage_capacity_gib) * 100 : null
  return (
    <Reveal>
      <section className="apple-section apple-section--tight" aria-label="Capacity" data-testid="mission-control-capacity">
        <div className="nl-capacity">
          <div className="min-w-0">
            <p className="apple-eyebrow">Capacity</p>
            <h2 className="apple-display apple-display--sm">Headroom</h2>
            <p className="apple-lede">
              {capacity.estimated_small_vms_addable != null ? `Room for about ${capacity.estimated_small_vms_addable} more small VMs.` : 'Fleet-wide utilisation across online hosts.'}
            </p>
          </div>
          <div className="nl-capacity-rings">
            <RingGauge value={Number.isFinite(capacity.avg_cpu_percent) ? capacity.avg_cpu_percent : null} label="CPU" sub="average" />
            <RingGauge value={mem} label="Memory" sub={`${Math.round(capacity.memory_used_mib / 1024)} of ${Math.round(capacity.memory_total_mib / 1024)} GiB`} />
            <RingGauge value={storage} label="Storage" sub={capacity.storage_capacity_gib ? `${Math.round(capacity.storage_used_gib ?? 0)} of ${Math.round(capacity.storage_capacity_gib)} GiB` : 'not reported'} />
          </div>
        </div>
      </section>
    </Reveal>
  )
}
