// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useId, type ReactNode } from 'react'
import { ChevronLeft, ChevronRight } from 'lucide-react'

type PlatformStepWizardProps = {
  open: boolean
  onClose: () => void
  title: string
  subtitle?: string
  steps: string[]
  step: number
  onStepChange: (next: number) => void
  canNext: boolean
  busy?: boolean
  finishLabel?: string
  nextLabel?: string
  maxWidthClass?: string
  /** When true, render inline on a page (no modal backdrop). */
  embedded?: boolean
  children: ReactNode
  onFinish: () => void | Promise<void>
  /** When set, called instead of advancing step on Next (non-final steps). */
  onNext?: () => void | Promise<void>
}

export default function PlatformStepWizard({
  open,
  onClose,
  title,
  subtitle,
  steps,
  step,
  onStepChange,
  canNext,
  busy = false,
  finishLabel = 'Finish',
  nextLabel = 'Next',
  maxWidthClass = 'max-w-4xl',
  embedded = false,
  children,
  onFinish,
  onNext,
}: PlatformStepWizardProps) {
  const titleId = useId()
  // Escape closes the modal wizard (keyboard users couldn't dismiss it before).
  useEffect(() => {
    if (!open || embedded) return
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, embedded, onClose])

  if (!open) return null

  const isLast = step >= steps.length - 1

  const panel = (
      <div
        className={`w-full ${maxWidthClass} ${embedded ? '' : 'max-h-[90dvh]'} flex flex-col rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] shadow-2xl overflow-hidden`}
        role={embedded ? undefined : 'dialog'}
        aria-modal={embedded ? undefined : true}
        aria-labelledby={embedded ? undefined : titleId}
        onClick={embedded ? undefined : (e) => e.stopPropagation()}
      >
        <div className="shrink-0 px-6 py-4 border-b border-[var(--apple-hairline)]">
          <h2 id={titleId} className="text-xl font-semibold">{title}</h2>
          {subtitle && <p className="text-sm text-[var(--text-muted)] mt-0.5">{subtitle}</p>}
          <p className="text-sm text-[var(--text-muted)] mt-1">
            Step {step + 1} of {steps.length} — {steps[step]}
          </p>
          <div className="flex gap-1.5 mt-3">
            {steps.map((label, i) => (
              <div key={label} className="flex-1 flex flex-col gap-1 min-w-0">
                <div className={`h-1 rounded-full ${i <= step ? 'bg-[var(--accent)]' : 'bg-[var(--apple-fill-tertiary)]'}`} title={label} />
                <span
                  className={`text-[10px] truncate hidden sm:block ${i === step ? 'text-[var(--text-primary)]' : 'text-[var(--text-faint)]'}`}
                >
                  {label}
                </span>
              </div>
            ))}
          </div>
        </div>

        <div className="flex-1 min-h-0 overflow-y-auto p-6">{children}</div>

        <div className="shrink-0 px-6 py-4 border-t border-[var(--apple-hairline)] flex items-center justify-between gap-2 bg-[var(--apple-surface-elevated)]">
          <button type="button" className="btn-secondary text-sm" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <div className="flex gap-2">
            {step > 0 && (
              <button
                type="button"
                className="btn-secondary text-sm inline-flex items-center gap-1"
                onClick={() => onStepChange(step - 1)}
                disabled={busy}
              >
                <ChevronLeft className="w-4 h-4" /> Back
              </button>
            )}
            <button
              type="button"
              className="btn-primary text-sm inline-flex items-center gap-1 min-w-[7rem] justify-center"
              disabled={busy || !canNext}
              onClick={() => void (isLast ? onFinish() : onNext ? onNext() : onStepChange(step + 1))}
            >
              {busy ? 'Working…' : isLast ? (
                finishLabel
              ) : (
                <>
                  {nextLabel} <ChevronRight className="w-4 h-4" />
                </>
              )}
            </button>
          </div>
        </div>
      </div>
  )

  if (embedded) {
    return <div className="mx-auto">{panel}</div>
  }

  return (
    <div
      className="fixed inset-0 z-[70] bg-black/60 backdrop-blur-sm flex items-center justify-center p-4"
      onClick={onClose}
    >
      {panel}
    </div>
  )
}
