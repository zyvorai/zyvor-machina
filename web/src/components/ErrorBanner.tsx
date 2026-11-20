// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ReactNode } from 'react'
import { AlertTriangle } from 'lucide-react'
import { statusBadgeClasses, statusToneClass } from '../utils/semanticColors'

type Tone = 'amber' | 'red'

type Props = {
  title?: string
  headline?: string
  /** Shorthand for simple platform pages: maps to title + headline */
  message?: string
  hints?: string[]
  technicalDetail?: string
  tone?: Tone
  onDismiss?: () => void
  onRetry?: () => void
  retryLabel?: string
  actions?: ReactNode
}

const toneStyles: Record<Tone, { border: string; bg: string; title: string; text: string; semantic: 'warn' | 'error' }> = {
  amber: {
    border: 'border-[color-mix(in_srgb,var(--machina-status-warn)_40%,transparent)]',
    bg: 'bg-[color-mix(in_srgb,var(--machina-status-warn)_10%,transparent)]',
    // Text tokens are theme-aware (dark text on light, bright on dark). The old mix-toward-white
    // values were ~2.3:1 on the light theme's tinted panel.
    title: 'text-[var(--nl-status-warn-text,#ffd60a)]',
    text: 'text-[var(--nl-status-warn-text,#ffd60a)]',
    semantic: 'warn',
  },
  red: {
    border: 'border-[color-mix(in_srgb,var(--machina-status-error)_40%,transparent)]',
    bg: 'bg-[color-mix(in_srgb,var(--machina-status-error)_10%,transparent)]',
    title: 'text-[var(--nl-accent-red-text,#ff6961)]',
    text: 'text-[var(--nl-accent-red-text,#ff6961)]',
    semantic: 'error',
  },
}

/** Actionable error panel with optional hints and technical details. */
export default function ErrorBanner({
  title: titleProp,
  headline: headlineProp,
  message,
  hints,
  technicalDetail,
  tone = 'amber',
  onDismiss,
  onRetry,
  retryLabel = 'Retry',
  actions,
}: Props) {
  const title = titleProp ?? (message ? 'Error' : '')
  const headline = headlineProp ?? message ?? ''
  const s = toneStyles[tone]

  return (
    <div
      role="alert"
      className={`rounded-xl border ${s.border} ${s.bg} px-4 py-3 space-y-3`}
    >
      <div className="flex items-start gap-2">
        <AlertTriangle className={`w-5 h-5 shrink-0 mt-0.5 ${statusToneClass(s.semantic)}`} />
        <div className="min-w-0 flex-1 space-y-1">
          <h2 className={`text-sm font-semibold ${s.title}`}>{title}</h2>
          <p className={`text-sm ${s.text} leading-relaxed`}>{headline}</p>
        </div>
        <div className="flex flex-wrap gap-2 shrink-0">
          {onRetry && (
            <button
              type="button"
              onClick={onRetry}
              className={`text-xs px-2 py-1 rounded border ${statusBadgeClasses(s.semantic)}`}
            >
              {retryLabel}
            </button>
          )}
          {onDismiss && (
            <button
              type="button"
              onClick={onDismiss}
              className={`text-xs px-2 py-1 rounded border ${statusBadgeClasses(s.semantic)} opacity-90 hover:opacity-100`}
            >
              Dismiss
            </button>
          )}
        </div>
      </div>

      {hints && hints.length > 0 && (
        <div className="pl-7 space-y-1.5">
          <p className={`text-xs font-medium ${statusToneClass(s.semantic)}`}>What usually fixes it</p>
          <ul className={`text-xs list-disc pl-4 space-y-1 ${s.text}`}>
            {hints.map((h) => (
              <li key={h}>{h}</li>
            ))}
          </ul>
        </div>
      )}

      {actions && <div className="pl-7 flex flex-wrap gap-2">{actions}</div>}

      {technicalDetail && (
        <details className="pl-7 group">
          <summary className={`text-xs cursor-pointer ${statusToneClass(s.semantic)} opacity-80 hover:opacity-100`}>
            Technical details
          </summary>
          <pre className="mt-2 text-[11px] leading-snug text-[var(--text-secondary)] bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg p-3 overflow-x-auto whitespace-pre-wrap break-words max-h-56 overflow-y-auto">
            {technicalDetail}
          </pre>
        </details>
      )}
    </div>
  )
}
