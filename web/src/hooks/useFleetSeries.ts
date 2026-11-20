// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, useState } from 'react'

export type FleetSample = { running: number; hostsOnline: number; memoryPct: number | null; offline: number }

const MAX = 40

/** Keeps a short rolling history of fleet figures, one sample per `tick` (a changing number). */
export function useFleetSeries(sample: FleetSample | null, tick: number) {
  const [series, setSeries] = useState<FleetSample[]>([])
  const last = useRef<number>(-1)
  useEffect(() => {
    if (!sample || tick === last.current) return
    last.current = tick
    setSeries((prev) => [...prev, sample].slice(-MAX))
  }, [sample, tick])
  return series
}
