// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useId } from 'react'
import { useCountUp } from '../../../hooks/useCountUp'

function ringColors(score: number): [string, string] {
  if (score >= 80) return ['#30d158', '#64d2ff']
  if (score >= 60) return ['#ffd60a', '#30d158']
  if (score >= 40) return ['#ff9f0a', '#ffd60a']
  return ['#ff453a', '#ff9f0a']
}

/** Circular health score with a status-coloured gradient stroke. */
export default function VmHealthRing({ score, loading }: { score: number | null; loading?: boolean }) {
  const gid = useId()
  const shown = useCountUp(score)
  const r = 34
  const c = 2 * Math.PI * r
  const pct = Math.max(0, Math.min(100, score ?? 0))
  const [from, to] = ringColors(pct)
  return (
    <div className="relative w-[5.5rem] h-[5.5rem] shrink-0" role="img" aria-label={score != null ? `Health ${score} of 100` : 'Health unknown'}>
      <svg viewBox="0 0 80 80" className="w-full h-full -rotate-90">
        <defs>
          <linearGradient id={gid} x1="0" y1="0" x2="1" y2="1">
            <stop offset="0%" stopColor={from} />
            <stop offset="100%" stopColor={to} />
          </linearGradient>
        </defs>
        <circle cx="40" cy="40" r={r} fill="none" stroke="currentColor" strokeOpacity="0.1" strokeWidth="7" />
        {score != null ? (
          <circle
            cx="40" cy="40" r={r} fill="none" stroke={`url(#${gid})`} strokeWidth="7" strokeLinecap="round"
            strokeDasharray={c} strokeDashoffset={c - (c * pct) / 100}
            style={{ transition: 'stroke-dashoffset 0.8s ease' }}
          />
        ) : null}
      </svg>
      <div className="absolute inset-0 grid place-items-center text-center">
        <div>
          <p className="text-xl font-semibold leading-none tracking-tight text-[var(--text-primary)] tabular-nums">
            {loading ? '…' : score != null ? shown ?? score : '—'}
          </p>
          <p className="text-[10px] text-[var(--text-muted)] mt-0.5">Health</p>
        </div>
      </div>
    </div>
  )
}
