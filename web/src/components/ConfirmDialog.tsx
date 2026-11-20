// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useId, useRef, useState } from 'react'
import { useFocusTrap } from '../hooks/useFocusTrap'
import { AlertTriangle } from 'lucide-react'

interface Props {
  open: boolean
  title: string
  message: string
  confirmLabel?: string
  variant?: 'danger' | 'warning'
  onConfirm: () => void
  onCancel: () => void
  /** When set, the confirm button stays disabled until the user types this exact string. */
  typeToMatch?: string
  /** Label above the confirmation input (default explains typing the phrase). */
  typeToMatchLabel?: string
}

export default function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel = 'Confirm',
  variant = 'danger',
  onConfirm,
  onCancel,
  typeToMatch,
  typeToMatchLabel,
}: Props) {
  const [typed, setTyped] = useState('')
  const titleId = useId()
  const messageId = useId()
  const inputId = useId()
  const dialogRef = useRef<HTMLFormElement>(null)

  // Trap Tab, close on Escape (topmost surface only) and give focus back to whatever opened the
  // dialog. Replaces the ad hoc Escape listener, which did neither of the other two.
  useFocusTrap(dialogRef, open, onCancel)

  useEffect(() => {
    if (open) setTyped('')
  }, [open, typeToMatch])

  if (!open) return null

  const confirmClass = variant === 'danger' ? 'btn-destructive' : 'btn-primary'

  const needsMatch = Boolean(typeToMatch && typeToMatch.length > 0)
  const matchOk = !needsMatch || typed === typeToMatch

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm animate-fade-in"
      onClick={onCancel}
      role="presentation"
    >
      <form
        ref={dialogRef}
        role="dialog"
        aria-modal
        aria-labelledby={titleId}
        aria-describedby={messageId}
        className="bg-[var(--apple-surface-elevated)] border border-[var(--apple-hairline)] rounded-2xl shadow-2xl w-full max-w-md mx-4 animate-scale-in"
        onClick={(e) => e.stopPropagation()}
        onSubmit={(e) => { e.preventDefault(); if (matchOk) onConfirm() }}
      >
        <div className="flex items-center gap-2.5 p-5 border-b border-[var(--apple-hairline)]">
          <div className="w-8 h-8 rounded-full bg-[color-mix(in_srgb,var(--machina-status-warn)_18%,transparent)] flex items-center justify-center">
            <AlertTriangle className="w-4 h-4 text-[var(--machina-status-warn)]" />
          </div>
          <h2 id={titleId} className="text-lg font-semibold tracking-tight text-[var(--text-primary)]">{title}</h2>
        </div>
        <div className="p-5 text-[var(--text-secondary)] text-sm leading-relaxed space-y-3">
          <div id={messageId}>{message}</div>
          {needsMatch && (
            <div>
              <label className="block text-xs text-[var(--text-muted)] mb-1.5" htmlFor={inputId}>
                {typeToMatchLabel ?? 'Type the confirmation phrase exactly (case-sensitive):'}
              </label>
              <input
                id={inputId}
                type="text"
                autoFocus
                autoComplete="off"
                autoCorrect="off"
                spellCheck={false}
                value={typed}
                onChange={(e) => setTyped(e.target.value)}
                className="input-field font-mono"
                placeholder={typeToMatch}
              />
            </div>
          )}
        </div>
        <div className="flex justify-end gap-3 px-5 pb-5">
          <button type="button" onClick={onCancel} className="btn-secondary text-sm">Cancel</button>
          <button
            type="submit"
            disabled={!matchOk}
            className={`${confirmClass} text-sm disabled:opacity-40 disabled:cursor-not-allowed`}
          >
            {confirmLabel}
          </button>
        </div>
      </form>
    </div>
  )
}
