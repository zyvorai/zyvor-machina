// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ReactNode } from 'react'
import { Check, Loader2 } from 'lucide-react'
import { statusBadgeClasses, statusBorderClass, statusToneClass } from '../utils/semanticColors'

type Variant = 'slate' | 'amber' | 'violet'

const doneStepClasses = 'bg-[color-mix(in_srgb,var(--machina-status-ok)_90%,transparent)] border-transparent text-white'
const doneLineClasses = 'bg-[color-mix(in_srgb,var(--machina-status-ok)_70%,transparent)]'

const variantRing: Record<Variant, string> = {
  slate: 'ring-[var(--accent)]/40 text-[var(--accent)] border-[var(--accent)]/40',
  amber: 'ring-amber-500/50 text-amber-700 border-amber-500/40',
  violet: 'ring-violet-500/50 text-[var(--link)] border-[var(--accent)]/40',
}

const variantDone: Record<Variant, string> = {
  slate: doneStepClasses,
  amber: doneStepClasses,
  violet: doneStepClasses,
}

const variantLineDone: Record<Variant, string> = {
  slate: doneLineClasses,
  amber: doneLineClasses,
  violet: doneLineClasses,
}

interface BuildStepTimelineProps {
  steps: readonly string[]
  /** Step currently in progress when not allComplete and not failed */
  activeIndex: number
  allComplete?: boolean
  failed?: boolean
  variant?: Variant
  className?: string
}

/**
 * Horizontal milestone strip (numbered steps, check when done, spinner on current).
 */
export function BuildStepTimeline({
  steps,
  activeIndex,
  allComplete = false,
  failed = false,
  variant = 'slate',
  className = '',
}: BuildStepTimelineProps) {
  const v = variant
  const nodes: ReactNode[] = []
  for (let i = 0; i < steps.length; i++) {
    const label = steps[i]
    const done = allComplete || (!failed && i < activeIndex) || (failed && i < activeIndex)
    const current = !allComplete && !failed && i === activeIndex
    const errHere = failed && i === activeIndex
    const lineDone = allComplete || i < activeIndex || (failed && i < activeIndex)

    nodes.push(
      <div key={`step-${i}`} className="flex min-w-0 max-w-[28%] flex-1 flex-col items-center px-0.5 sm:max-w-none" role="listitem">
        <div
          className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-full border-2 text-[11px] font-semibold transition-colors ${
            done
              ? variantDone[v]
              : errHere
                ? `${statusBadgeClasses('error')} border-2 ${statusBorderClass('error')}`
                : current
                  ? `border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)] ${variantRing[v]} ring-2`
                  : 'border-[var(--apple-hairline)] bg-[var(--apple-surface)] text-[var(--text-muted)]'
          }`}
          aria-current={current ? 'step' : undefined}
        >
          {done ? (
            <Check className="h-4 w-4" strokeWidth={2.5} aria-hidden />
          ) : current ? (
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
          ) : errHere ? (
            <span aria-hidden>!</span>
          ) : (
            <span>{i + 1}</span>
          )}
        </div>
        <span
          className={`mt-1.5 text-center text-[10px] font-medium leading-snug sm:text-[11px] ${
            done || current ? 'text-[var(--text-primary)]' : errHere ? statusToneClass('error') : 'text-[var(--text-muted)]'
          }`}
        >
          {label}
        </span>
      </div>,
    )
    if (i < steps.length - 1) {
      nodes.push(
        <div
          key={`line-${i}`}
          className={`mt-4 hidden h-0.5 min-w-[4px] flex-1 sm:block ${lineDone ? variantLineDone[v] : 'bg-[var(--surface-hover)]/80'}`}
          aria-hidden
        />,
      )
    }
  }

  return (
    <div
      className={`rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-surface)] px-2 py-3 sm:px-3 ${className}`}
      role="list"
      aria-label="Build progress"
    >
      <div className="flex w-full items-start justify-between gap-0">{nodes}</div>
    </div>
  )
}
