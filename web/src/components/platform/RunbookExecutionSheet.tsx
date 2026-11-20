// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Copy } from 'lucide-react'
import { MacSheet } from './mac/PlatformMacUi'
import { useToastContext } from '../../contexts/ToastContext'

export type RunbookExecutionResult = {
  title: string
  summary: string
  steps: string[]
  commands: string[]
}

type Props = {
  open: boolean
  onClose: () => void
  result: RunbookExecutionResult | null
}

export default function RunbookExecutionSheet({ open, onClose, result }: Props) {
  const toast = useToastContext()
  if (!result) return null

  const copyCommands = async () => {
    if (result.commands.length === 0) return
    try {
      await navigator.clipboard.writeText(result.commands.join('\n'))
      toast.success('Commands copied')
    } catch {
      toast.error('Could not copy commands')
    }
  }

  return (
    <MacSheet open={open} onClose={onClose} title={result.title} subtitle={result.summary}>
      <div className="space-y-4 text-sm">
        {result.steps.length > 0 && (
          <div>
            <p className="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)] mb-2">Steps</p>
            <ol className="list-decimal list-inside space-y-2 text-[var(--text-secondary)]">
              {result.steps.map((step, i) => (
                <li key={`${i}-${step.slice(0, 24)}`}>{step}</li>
              ))}
            </ol>
          </div>
        )}
        {result.commands.length > 0 && (
          <div>
            <div className="flex items-center justify-between gap-2 mb-2">
              <p className="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)]">Commands</p>
              <button type="button" className="btn-secondary text-xs inline-flex items-center gap-1" onClick={() => void copyCommands()}>
                <Copy className="w-3 h-3" /> Copy all
              </button>
            </div>
            <ul className="space-y-2 font-mono text-xs text-[var(--text-muted)]">
              {result.commands.map((cmd, i) => (
                <li key={`${i}-${cmd.slice(0, 16)}`} className="rounded-lg border border-white/[0.06] bg-[var(--apple-surface)] p-2 break-all">
                  {cmd}
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </MacSheet>
  )
}

export function parseRunbookStepsJson(stepsJson: unknown): string[] {
  if (Array.isArray(stepsJson)) {
    return stepsJson.map((s) => (typeof s === 'string' ? s : JSON.stringify(s)))
  }
  if (stepsJson && typeof stepsJson === 'object' && 'steps' in stepsJson) {
    const steps = (stepsJson as { steps?: unknown }).steps
    if (Array.isArray(steps)) return steps.map((s) => (typeof s === 'string' ? s : JSON.stringify(s)))
  }
  return []
}
