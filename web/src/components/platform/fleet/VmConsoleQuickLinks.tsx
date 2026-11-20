// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { Monitor, Wifi } from 'lucide-react'
import { getConsoleHubPlan, type ConsoleHubPlan } from '../../../api/platform'
import {
  CONSOLE_PROTOCOL_LABELS,
  consoleHubPath,
  sortedDisplayProtocols,
} from './vmConsoleLinks'

type Props = {
  vmId: string
  running?: boolean
  compact?: boolean
  className?: string
}

export default function VmConsoleQuickLinks({ vmId, running = true, compact = false, className = '' }: Props) {
  const [plan, setPlan] = useState<ConsoleHubPlan | null>(null)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (!running || !vmId) {
      setPlan(null)
      return
    }
    let cancelled = false
    setLoading(true)
    void getConsoleHubPlan(vmId)
      .then((p) => {
        if (!cancelled) setPlan(p)
      })
      .catch(() => {
        if (!cancelled) setPlan(null)
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [vmId, running])

  if (!running) {
    return <p className={`text-xs text-[var(--text-muted)] ${className}`}>Start the VM to open a graphical console.</p>
  }

  if (loading && !plan) {
    return <p className={`text-xs text-[var(--text-muted)] ${className}`}>Loading console options…</p>
  }

  const protocols = sortedDisplayProtocols(plan?.protocols ?? ['novnc'], plan?.recommended ?? 'novnc')
  if (protocols.length === 0) {
    return <p className={`text-xs text-[var(--text-muted)] ${className}`}>No console protocols reported for this VM.</p>
  }

  return (
    <div className={`flex flex-wrap gap-1.5 ${className}`} data-testid="vm-console-quick-links">
      {protocols.map((p) => {
        const label = CONSOLE_PROTOCOL_LABELS[p] ?? p
        const active = p === (plan?.recommended ?? 'novnc')
        const Icon = p === 'webrtc_spice' ? Wifi : Monitor
        return (
          <Link
            key={p}
            to={consoleHubPath(vmId, p)}
            className={
              compact
                ? `btn-secondary text-[10px] py-1 px-2 inline-flex items-center gap-1 ${active ? 'border-emerald-500/40 text-emerald-800' : ''}`
                : `btn-secondary text-xs py-1.5 px-2.5 inline-flex items-center gap-1 flex-1 justify-center min-w-[4.5rem] ${active ? 'border-emerald-500/40 text-emerald-800' : ''}`
            }
            title={active ? `${label} (recommended)` : label}
          >
            <Icon className="w-3 h-3" />
            {label}
          </Link>
        )
      })}
    </div>
  )
}
