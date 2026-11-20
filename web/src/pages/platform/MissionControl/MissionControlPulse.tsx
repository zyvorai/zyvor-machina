// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useMemo, useState } from 'react'
import Sparkline from '../../../components/kit/Sparkline'
import { useCountUp } from '../../../hooks/useCountUp'
import { useFleetSeries, type FleetSample } from '../../../hooks/useFleetSeries'
import type { MissionControlFleetState } from './useMissionControlFleet'

function Figure({
  label,
  value,
  format,
  spark,
  tone = 'neutral',
}: {
  label: string
  value: number | null
  format: (n: number) => string
  spark: number[]
  tone?: 'neutral' | 'warn'
}) {
  const shown = useCountUp(value)
  return (
    <div className="nl-pulse-figure min-w-0">
      <div className={`nl-pulse-value${tone === 'warn' ? ' is-warn' : ''}`}>{shown == null ? '—' : format(shown)}</div>
      <div className="nl-pulse-label">{label}</div>
      <div className="nl-pulse-spark">
        <Sparkline values={spark} label={`${label} over the last samples`} />
      </div>
    </div>
  )
}

/** Netra-style pulse band: live figures with count-up numbers and sparklines. */
export default function MissionControlPulse({ state, warnings }: { state: MissionControlFleetState; warnings: number }) {
  const { loading, error, running, onlineHosts, hosts, storagePct, updatedAt, sampleTick } = state
  const down = Boolean(error)
  const sample: FleetSample | null = useMemo(
    () => (loading || down ? null : { running, hostsOnline: onlineHosts, memoryPct: storagePct, offline: warnings }),
    [loading, down, running, onlineHosts, storagePct, warnings],
  )
  const series = useFleetSeries(sample, sampleTick)

  // "Updated Ns ago" label, re-rendered every few seconds.
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 5000)
    return () => window.clearInterval(id)
  }, [])
  const ago = updatedAt ? Math.max(0, Math.round((now - updatedAt) / 1000)) : null

  const off = loading || down
  return (
    <section className="apple-section apple-section--tight" aria-label="Fleet pulse" data-testid="mission-control-pulse">
      <div className="nl-pulse">
        <div className="nl-pulse-head">
          <h2 className="nl-pulse-title">Fleet pulse</h2>
          <span className={`nl-live-pill${down ? ' is-down' : ''}`}>
            <i aria-hidden />
            {down ? 'Offline' : ago == null ? 'Connecting…' : `Live · updated ${ago < 5 ? 'just now' : `${ago}s ago`}`}
          </span>
        </div>
        <div className="nl-pulse-grid">
          <Figure label="VMs running" value={off ? null : running} format={(n) => String(Math.round(n))} spark={series.map((s) => s.running)} />
          <Figure
            label="Hosts online"
            value={off ? null : onlineHosts}
            format={(n) => `${Math.round(n)}/${hosts.length}`}
            spark={series.map((s) => s.hostsOnline)}
          />
          <Figure
            label="Memory used"
            value={off || storagePct == null ? null : storagePct}
            format={(n) => `${Math.round(n)}%`}
            spark={series.map((s) => s.memoryPct ?? 0)}
          />
          <Figure
            label="Alerts"
            value={off ? null : warnings}
            format={(n) => (Math.round(n) === 0 ? 'None' : String(Math.round(n)))}
            spark={series.map((s) => s.offline)}
            tone={warnings > 0 ? 'warn' : 'neutral'}
          />
        </div>
      </div>
    </section>
  )
}
