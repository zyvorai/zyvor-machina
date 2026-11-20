// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useId } from 'react'
import { useCountUp } from '../../hooks/useCountUp'

/** Circular utilisation gauge: low is calm (green→cyan), high shifts through amber to red. */
export default function RingGauge({ value, label, sub, size = 96 }: { value: number | null; label: string; sub?: string; size?: number }) {
  const gid = useId()
  const shown = useCountUp(value == null ? null : Math.round(value))
  const pct = value == null ? 0 : Math.max(0, Math.min(100, value))
  const [from, to] = pct >= 90 ? ['#ff453a', '#ff9f0a'] : pct >= 75 ? ['#ff9f0a', '#ffd60a'] : ['#30d158', '#64d2ff']
  const r = 34
  const c = 2 * Math.PI * r
  return (
    <div className="flex flex-col items-center text-center" role="img" aria-label={value == null ? `${label} unknown` : `${label} ${Math.round(pct)} percent`}>
      <div className="relative" style={{ width: size, height: size }}>
        <svg viewBox="0 0 80 80" className="h-full w-full -rotate-90">
          <defs>
            <linearGradient id={gid} x1="0" y1="0" x2="1" y2="1">
              <stop offset="0%" stopColor={from} />
              <stop offset="100%" stopColor={to} />
            </linearGradient>
          </defs>
          <circle cx="40" cy="40" r={r} fill="none" stroke="currentColor" strokeOpacity="0.1" strokeWidth="7" />
          {value != null ? (
            <circle cx="40" cy="40" r={r} fill="none" stroke={`url(#${gid})`} strokeWidth="7" strokeLinecap="round" strokeDasharray={c} strokeDashoffset={c - (c * pct) / 100} style={{ transition: 'stroke-dashoffset 0.9s ease' }} />
          ) : null}
        </svg>
        <div className="absolute inset-0 grid place-items-center">
          <span className="text-xl font-semibold tracking-tight tabular-nums text-[var(--text-primary)]">{value == null ? '—' : `${shown ?? Math.round(pct)}%`}</span>
        </div>
      </div>
      <p className="mt-2 text-sm font-medium text-[var(--text-primary)]">{label}</p>
      {sub ? <p className="text-xs text-[var(--text-muted)]">{sub}</p> : null}
    </div>
  )
}
