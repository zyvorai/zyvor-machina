// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { getFleetHeatmap, type FleetHeatmap } from '../../../api/ai'
import { formatUserError } from '../../../utils/apiError'

export default function MachineFinderHeatmapLens() {
  const [data, setData] = useState<FleetHeatmap | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    void getFleetHeatmap()
      .then(setData)
      .catch((e: unknown) => setError(formatUserError(e)))
  }, [])

  if (error) return <p className="text-sm text-red-600 p-4">{error}</p>
  if (!data) return <p className="text-sm text-[var(--text-muted)] p-4">Loading heatmap…</p>

  const cells = data.hosts ?? []

  return (
    <div className="card p-4 space-y-3" data-testid="machine-finder-heatmap">
      <div className="flex items-center justify-between">
        <h2 className="font-medium text-[var(--text-primary)]">Fleet heatmap</h2>
        <Link to="/platform/zyra" className="btn-secondary text-xs">Open Zyra OS</Link>
      </div>
      {data.hotspots?.length > 0 && (
        <p className="text-sm text-amber-700/90">Hotspots: {data.hotspots.join(', ')}</p>
      )}
      <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 gap-2">
        {cells.slice(0, 24).map((cell) => {
          const intensity = Math.min(100, Math.max(cell.cpu_percent, cell.memory_percent))
          return (
            <div
              key={cell.host_id}
              className="rounded-lg border border-white/[0.06] p-3 text-xs"
              style={{ background: `rgba(239, 68, 68, ${intensity / 200})` }}
            >
              <p className="font-medium truncate">{cell.hostname}</p>
              <p className="text-[var(--text-muted)]">CPU {cell.cpu_percent.toFixed(0)}% · {cell.vm_count} VMs</p>
              <p className="text-[var(--text-muted)] capitalize">{cell.classification}</p>
            </div>
          )
        })}
      </div>
      {cells.length === 0 && <p className="text-sm text-[var(--text-muted)]">No heatmap data yet.</p>}
    </div>
  )
}
