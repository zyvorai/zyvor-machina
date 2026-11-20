// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { getFleetActivity, type FleetActivityOverview } from '../../../api/platform'
import { formatUserError } from '../../../utils/apiError'

export default function MachineFinderTimelineLens() {
  const [data, setData] = useState<FleetActivityOverview | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    void getFleetActivity()
      .then(setData)
      .catch((e: unknown) => setError(formatUserError(e)))
  }, [])

  if (error) return <p className="text-sm text-red-600 p-4">{error}</p>
  if (!data) return <p className="text-sm text-[var(--text-muted)] p-4">Loading fleet activity…</p>

  return (
    <div className="card p-4 space-y-3" data-testid="machine-finder-timeline">
      <div className="flex items-center justify-between">
        <h2 className="font-medium text-[var(--text-primary)]">Fleet timeline</h2>
        <Link to="/platform/activity" className="btn-secondary text-xs">Open Activity Monitor</Link>
      </div>
      <p className="text-sm text-[var(--text-muted)]">{data.summary}</p>
      <ul className="space-y-2 text-sm">
        {data.top_vms.slice(0, 12).map((vm) => (
          <li key={vm.vm_id} className="flex gap-3 text-[var(--text-secondary)] border-b border-white/[0.04] pb-2">
            <Link to={`/platform/vms/${vm.vm_id}`} className="font-medium text-[var(--link)]/90 hover:underline shrink-0">{vm.vm_name}</Link>
            <span className="text-xs text-[var(--text-muted)]">CPU {vm.cpu_percent?.toFixed?.(0) ?? vm.cpu_percent}% · mem {vm.memory_used_mib} MiB</span>
          </li>
        ))}
        {data.top_vms.length === 0 && <li className="text-[var(--text-muted)]">No recent VM activity.</li>}
      </ul>
    </div>
  )
}
