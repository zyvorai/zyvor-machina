// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import Sparkline from '../../kit/Sparkline'
import { useCountUp } from '../../../hooks/useCountUp'
import type { VmMetricSample } from '../../../hooks/useVmMetricSeries'
import { formatVmMemoryGiB } from '../../../utils/vmVisual'

function Figure({ label, children, spark }: { label: string; children: React.ReactNode; spark?: React.ReactNode }) {
  return (
    <div className="nl-pulse-figure min-w-0">
      <div className="nl-pulse-value">{children}</div>
      <div className="nl-pulse-label">{label}</div>
      {spark ? <div className="nl-pulse-spark">{spark}</div> : null}
    </div>
  )
}

/** Live headline numbers for a VM, in the same stat style as Mission Control's pulse band. */
export default function VmOverviewStats({
  series,
  vcpus,
  memoryMib,
  healthScore,
  running,
}: {
  series: VmMetricSample[]
  vcpus?: number | null
  memoryMib?: number | null
  healthScore?: number | null
  running: boolean
}) {
  const latest = series.length ? series[series.length - 1] : null
  const cpu = useCountUp(latest ? Math.round(latest.cpu_percent) : null)
  const memPct = latest && memoryMib ? Math.round((latest.memory_used_mib / memoryMib) * 100) : null
  const mem = useCountUp(memPct)
  return (
    <div className="nl-pulse-grid rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] px-5 py-4" data-testid="vm-overview-stats">
      <Figure
        label="CPU"
        spark={running ? <Sparkline values={series.map((s) => s.cpu_percent)} width={72} height={26} /> : undefined}
      >
        {latest && running && cpu != null ? `${cpu}%` : '—'}
      </Figure>
      <Figure
        label={memoryMib ? `Memory of ${formatVmMemoryGiB(memoryMib)}` : 'Memory'}
        spark={running ? <Sparkline values={series.map((s) => s.memory_used_mib)} width={72} height={26} /> : undefined}
      >
        {memPct != null && running && mem != null ? `${mem}%` : '—'}
      </Figure>
      <Figure label="vCPUs">{vcpus ?? '—'}</Figure>
      <Figure label="Health">{healthScore != null ? healthScore : '—'}</Figure>
    </div>
  )
}
