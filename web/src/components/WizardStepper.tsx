// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ReactNode } from 'react'

type Props = {
  steps: readonly string[]
  current: number
  onStep: (index: number) => void
  trailing?: ReactNode
}

/** Horizontal step buttons for multi-step forms (Create VM, Import, etc.). */
export default function WizardStepper({ steps, current, onStep, trailing }: Props) {
  return (
    <div className="bg-[var(--apple-surface)] rounded-xl p-4 border border-[var(--apple-hairline)] flex flex-wrap items-center justify-between gap-3">
      <div className="flex flex-wrap gap-2">
        {steps.map((label, i) => (
          <button
            key={label}
            type="button"
            onClick={() => onStep(i)}
            className={`text-xs px-2.5 py-1.5 rounded-lg transition font-medium ${
              i === current
                ? 'bg-[var(--accent)] text-white shadow-md shadow-[var(--accent)]/20'
                : i < current
                  ? 'bg-[var(--surface-hover)]/80 text-[var(--text-primary)] hover:bg-[var(--apple-fill-secondary)]'
                  : 'bg-[var(--apple-fill-tertiary)] text-[var(--text-muted)] hover:bg-[var(--surface-hover)]'
            }`}
          >
            {i + 1}. {label}
          </button>
        ))}
      </div>
      {trailing}
    </div>
  )
}
