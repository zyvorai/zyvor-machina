// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { ChevronRight } from 'lucide-react'

export interface FinderPathSegment {
  label: string
  onClick?: () => void
}

export default function FinderPathBar({ segments }: { segments: FinderPathSegment[] }) {
  if (!segments.length) return null

  return (
    <nav aria-label="Path" className="mac-finder-path flex items-center gap-0.5 min-w-0 text-xs text-[var(--text-muted)]">
      {segments.map((seg, i) => {
        const isLast = i === segments.length - 1
        return (
          <span key={`${seg.label}-${i}`} className="flex items-center gap-0.5 min-w-0">
            {i > 0 ? <ChevronRight className="h-3 w-3 shrink-0 opacity-50" aria-hidden /> : null}
            {seg.onClick && !isLast ? (
              <button type="button" onClick={seg.onClick} className="truncate hover:text-[var(--text-primary)] transition-colors">
                {seg.label}
              </button>
            ) : (
              <span className={`truncate ${isLast ? 'text-[var(--text-primary)] font-medium' : ''}`}>{seg.label}</span>
            )}
          </span>
        )
      })}
    </nav>
  )
}
