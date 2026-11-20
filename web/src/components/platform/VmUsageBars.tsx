// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { utilizationBarClass } from '../../utils/semanticColors'

type Props = {
  cpuPercent: number
  memoryUsedMib: number
  memoryTotalMib?: number
  vcpuCount?: number
}

function UsageBar({ label, pct, detail, testId }: { label: string; pct: number; detail: string; testId: string }) {
  return (
    <div className="space-y-1" data-testid={`vm-usage-${testId}`}>
      <div className="flex justify-between text-xs text-[var(--text-muted)]">
        <span>{label}</span>
        <span>{detail}</span>
      </div>
      <div className="h-2 rounded-full bg-[var(--apple-fill-tertiary)] overflow-hidden">
        <div
          role="progressbar"
          aria-label={label}
          aria-valuenow={Math.round(Math.min(100, Math.max(0, pct)))}
          aria-valuemin={0}
          aria-valuemax={100}
          className={`h-full rounded-full transition-all ${utilizationBarClass(pct, { warn: 75, error: 90 })}`}
          style={{ width: `${Math.min(100, Math.max(0, pct))}%` }}
        />
      </div>
    </div>
  )
}

export default function VmUsageBars({ cpuPercent, memoryUsedMib, memoryTotalMib, vcpuCount }: Props) {
  const memTotal = memoryTotalMib && memoryTotalMib > 0 ? memoryTotalMib : undefined
  const memPct = memTotal ? (memoryUsedMib / memTotal) * 100 : 0
  const memDetail = memTotal
    ? `${memoryUsedMib} / ${memTotal} MiB`
    : `${memoryUsedMib} MiB used`

  return (
    <div className="grid gap-4 sm:grid-cols-2" data-testid="vm-usage-bars">
      <UsageBar
        label={vcpuCount ? `CPU (${vcpuCount} vCPU)` : 'CPU'}
        testId="cpu"
        pct={cpuPercent}
        detail={`${cpuPercent.toFixed(1)}%`}
      />
      <UsageBar label="Memory" testId="memory" pct={memPct} detail={memDetail} />
    </div>
  )
}
