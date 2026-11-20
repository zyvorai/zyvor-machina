// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { getPlatformVmMetrics } from '../api/platform'

export type VmMetricSample = { cpu_percent: number; memory_used_mib: number; updated_at: string }

const MAX = 40
const INTERVAL_MS = 5000

/** Polls a VM's latest CPU/memory sample while it runs and keeps a short rolling history for charts. */
export function useVmMetricSeries(id: string | undefined, running: boolean, initial: VmMetricSample | null) {
  const [series, setSeries] = useState<VmMetricSample[]>(() => (initial ? [initial] : []))

  useEffect(() => {
    if (initial) {
      setSeries((prev) => (prev.length === 0 ? [initial] : prev))
    }
  }, [initial])

  useEffect(() => {
    if (!id || !running) return
    let cancelled = false
    const tick = () => {
      void getPlatformVmMetrics(id)
        .then((m) => {
          if (cancelled || !m) return
          setSeries((prev) => (prev.length && prev[prev.length - 1].updated_at === m.updated_at ? prev : [...prev, m].slice(-MAX)))
        })
        .catch(() => undefined)
    }
    const t = setInterval(tick, INTERVAL_MS)
    return () => {
      cancelled = true
      clearInterval(t)
    }
  }, [id, running])

  return series
}
