// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { getPlatformVmMetrics } from '../../api/platform'

type Props = {
  vmId: string
  running: boolean
  memoryMib: number
}

function MiniBar({ pct, tone }: { pct: number | null; tone: string }) {
  if (pct == null) return <span className="text-[var(--text-faint)] text-[10px]">—</span>
  return (
    <div className="flex items-center gap-1.5 min-w-[5.5rem]">
      <div className="flex-1 h-1.5 rounded-full bg-[var(--apple-fill-tertiary)] overflow-hidden">
        <div className={`h-full ${tone}`} style={{ width: `${Math.min(100, pct)}%` }} />
      </div>
      <span className="text-[10px] tabular-nums text-[var(--text-muted)] w-8 text-right">{pct}%</span>
    </div>
  )
}

export default function MachineFinderTableUsageCell({ vmId, running, memoryMib }: Props) {
  const [cpuPct, setCpuPct] = useState<number | null>(null)
  const [memPct, setMemPct] = useState<number | null>(null)

  useEffect(() => {
    if (!running) {
      setCpuPct(null)
      setMemPct(null)
      return
    }
    let cancelled = false
    void (async () => {
      try {
        const m = await getPlatformVmMetrics(vmId)
        if (cancelled) return
        setCpuPct(m.cpu_percent != null ? Math.round(m.cpu_percent) : null)
        if (memoryMib > 0 && m.memory_used_mib) {
          setMemPct(Math.min(100, Math.round((m.memory_used_mib / memoryMib) * 100)))
        }
      } catch {
        if (!cancelled) {
          setCpuPct(null)
          setMemPct(null)
        }
      }
    })()
    return () => { cancelled = true }
  }, [vmId, running, memoryMib])

  if (!running) {
    return <span className="text-[var(--text-faint)] text-xs">—</span>
  }

  return (
    <div className="space-y-1" data-testid={`vm-usage-${vmId}`}>
      <MiniBar pct={cpuPct} tone="bg-[var(--accent)]/70" />
      <MiniBar pct={memPct} tone="bg-[var(--accent)]" />
    </div>
  )
}
