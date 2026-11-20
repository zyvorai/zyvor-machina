// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { Dna } from 'lucide-react'
import { getFleetDna, type FleetDnaOverview } from '../../api/platform'
import { statusBadgeClasses, statusChipClasses } from '../../utils/semanticColors'

function gradeTone(grade: string): 'ok' | 'warn' | 'error' | 'info' {
  const g = grade.trim().toUpperCase().charAt(0)
  if (g === 'A' || g === 'B') return 'ok'
  if (g === 'C') return 'warn'
  if (g === 'D' || g === 'F') return 'error'
  return 'info'
}

function pillarTone(score: number): 'ok' | 'warn' | 'error' {
  if (score >= 80) return 'ok'
  if (score >= 60) return 'warn'
  return 'error'
}

type Props = {
  compact?: boolean
  className?: string
}

export default function InfrastructureDnaStrip({ compact = false, className = '' }: Props) {
  const [dna, setDna] = useState<FleetDnaOverview | null>(null)

  useEffect(() => {
    void getFleetDna()
      .then(setDna)
      .catch(() => setDna(null))
  }, [])

  if (!dna) return null

  const tone = gradeTone(dna.grade)

  return (
    <div
      className={`flex flex-wrap items-center gap-3 rounded-xl border border-white/[0.06] bg-[var(--apple-surface)] px-4 py-3 ${className}`}
      data-testid="infrastructure-dna-strip"
    >
      <div className="flex items-center gap-3 min-w-0">
        <div
          className={`flex h-12 w-12 shrink-0 items-center justify-center rounded-full border-2 text-sm font-bold ${statusBadgeClasses(tone)} ${compact ? 'h-10 w-10 text-xs' : ''}`}
          aria-label={`Infrastructure DNA score ${dna.score}`}
        >
          {dna.score}
        </div>
        <div className="min-w-0">
          <p className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)] flex items-center gap-1.5">
            <Dna className="w-3.5 h-3.5" /> Infrastructure DNA
          </p>
          <p className="text-sm text-[var(--text-primary)] truncate">
            Grade <span className={statusBadgeClasses(tone)}>{dna.grade}</span>
            {!compact && <span className="text-[var(--text-muted)]"> · {dna.summary}</span>}
          </p>
        </div>
      </div>
      <div className="flex flex-wrap gap-1.5 flex-1">
        {dna.pillars.map((p) => (
          <span
            key={p.id}
            title={p.detail}
            className={statusChipClasses(pillarTone(p.score))}
          >
            {p.label} {p.score}
          </span>
        ))}
      </div>
      <Link to="/platform/maintenance?tab=mission" className="text-xs text-orange-600/90 hover:underline shrink-0">
        Maintenance mission →
      </Link>
    </div>
  )
}
