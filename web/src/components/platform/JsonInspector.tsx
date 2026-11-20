// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState, type ReactNode } from 'react'
import { hubLinkClasses } from '../../utils/semanticColors'
import TerminalFrame from '../TerminalFrame'
import { renderHighlightedJson } from '../../utils/terminalHighlight'

export function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === 'object' && !Array.isArray(value)) return value as Record<string, unknown>
  return null
}

export function asArray(value: unknown): unknown[] {
  return Array.isArray(value) ? value : []
}

export function recordEntries(data: Record<string, unknown>, max = 12): Array<[string, unknown]> {
  return Object.entries(data)
    .filter(([k]) => !k.startsWith('_'))
    .slice(0, max)
}

export function summarizeJsonValue(value: unknown): string {
  if (Array.isArray(value)) return `${value.length} item${value.length === 1 ? '' : 's'}`
  if (value && typeof value === 'object') {
    const count = Object.keys(value as Record<string, unknown>).length
    return `${count} field${count === 1 ? '' : 's'}`
  }
  return String(value)
}

export default function JsonInspector({
  data,
  children,
  emptyMessage = 'No data.',
  className = '',
}: {
  data: unknown
  children?: ReactNode
  emptyMessage?: string
  className?: string
}) {
  const [raw, setRaw] = useState(false)
  if (data == null) return <p className="text-sm text-[var(--text-muted)]">{emptyMessage}</p>

  return (
    <div className={`space-y-3 ${className}`}>
      {!raw && (children ?? (
        <dl className="grid gap-2 sm:grid-cols-2 text-sm">
          {recordEntries(asRecord(data) ?? { value: data }).map(([k, v]) => (
            <div key={k} className="rounded-lg border border-white/[0.06] bg-[var(--apple-surface)] px-3 py-2">
              <dt className="text-xs text-[var(--text-muted)] capitalize">{k.replace(/_/g, ' ')}</dt>
              <dd className="text-[var(--text-primary)] mt-0.5 break-words">{summarizeJsonValue(v)}</dd>
            </div>
          ))}
        </dl>
      ))}
      {raw && (
        <TerminalFrame label="response.json" maxHeight="max-h-96">
          {renderHighlightedJson(JSON.stringify(data, null, 2))}
        </TerminalFrame>
      )}
      <button
        type="button"
        className={`text-xs hover:underline ${hubLinkClasses()}`}
        onClick={() => setRaw((v) => !v)}
      >
        {raw ? 'Hide raw JSON' : 'View raw JSON'}
      </button>
    </div>
  )
}
