// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Activity } from 'lucide-react'
import { MacGlassPanel } from '../mac/PlatformMacUi'
import Sparkline from '../../kit/Sparkline'
import VmUsageBars from '../VmUsageBars'
import type { VmMetricSample } from '../../../hooks/useVmMetricSeries'

/** Performance tab: live CPU and memory charts built from the polled sample history. */
export default function VmPerfPanel({
  series,
  memoryTotalMib,
  vcpuCount,
}: {
  series: VmMetricSample[]
  memoryTotalMib?: number
  vcpuCount?: number
}) {
  const latest = series.length ? series[series.length - 1] : null
  if (!latest) {
    return (
      <MacGlassPanel title="Performance">
        <p className="text-[var(--text-muted)] text-sm">Metrics appear after the next host inventory sync.</p>
      </MacGlassPanel>
    )
  }
  return (
    <MacGlassPanel title="Performance" subtitle={`Updated ${new Date(latest.updated_at).toLocaleTimeString()} · sampled every 5s`}>
      <div className="grid gap-4 sm:grid-cols-2" data-testid="vm-perf-charts">
        <div className="rounded-xl border border-[var(--apple-hairline)] p-4 text-[var(--link)]">
          <p className="text-xs text-[var(--text-muted)] flex items-center gap-1.5"><Activity className="w-3.5 h-3.5" /> CPU</p>
          <p className="text-3xl font-semibold tracking-tight text-[var(--text-primary)] mt-1">{latest.cpu_percent.toFixed(1)}%</p>
          <div className="mt-3"><Sparkline values={series.map((s) => s.cpu_percent)} width={320} height={64} label="CPU usage history" /></div>
        </div>
        <div className="rounded-xl border border-[var(--apple-hairline)] p-4 text-[var(--link)]">
          <p className="text-xs text-[var(--text-muted)]">Memory</p>
          <p className="text-3xl font-semibold tracking-tight text-[var(--text-primary)] mt-1">{latest.memory_used_mib} <span className="text-base font-normal text-[var(--text-muted)]">MiB</span></p>
          <div className="mt-3"><Sparkline values={series.map((s) => s.memory_used_mib)} width={320} height={64} label="Memory usage history" /></div>
        </div>
      </div>
      <div className="mt-4">
        <VmUsageBars cpuPercent={latest.cpu_percent} memoryUsedMib={latest.memory_used_mib} memoryTotalMib={memoryTotalMib} vcpuCount={vcpuCount} />
      </div>
      <p className="text-xs text-[var(--text-muted)] mt-3">Guest tools will unlock richer CPU, disk latency, and noisy-neighbor insights.</p>
    </MacGlassPanel>
  )
}
