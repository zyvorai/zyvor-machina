// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { AlertTriangle, Info } from 'lucide-react'
import type { AttentionItem } from '../../../utils/hostAttention'
import { statusPillClasses } from '../../../utils/semanticColors'

/** Only the hosts that need a person, each with its reason and the one-click fix when there is one. */
export default function AttentionRail({
  items,
  busy,
  onSelect,
  onRecheck,
}: {
  items: AttentionItem[]
  busy: boolean
  onSelect: (hostId: string) => void
  onRecheck: (hostId: string) => void
}) {
  if (items.length === 0) return null
  return (
    <section aria-label="Needs attention" data-testid="attention-rail" className="rounded-2xl border border-[var(--nl-status-warn-border)] bg-[var(--nl-status-warn-bg)] p-4">
      <h2 className="m-0 mb-2 text-sm font-semibold text-[var(--text-primary)] flex items-center gap-2">
        <AlertTriangle className="w-4 h-4 text-[var(--nl-status-warn-text)]" /> Needs attention
        <span className="text-xs font-normal text-[var(--text-muted)]">{items.length} item{items.length === 1 ? '' : 's'}</span>
      </h2>
      <ul className="m-0 p-0 list-none flex flex-col gap-1.5">
        {items.map((a) => (
          <li key={`${a.hostId}-${a.kind}`} className="flex flex-wrap items-center gap-2 text-sm" data-testid={`attention-${a.kind}-${a.hostname}`}>
            <span className={`text-[10px] uppercase px-2 py-0.5 rounded border ${statusPillClasses(a.severity === 'info' ? 'info' : a.severity)}`}>
              {a.severity === 'info' ? <Info className="w-3 h-3 inline" /> : a.kind}
            </span>
            <button type="button" className="font-medium text-[var(--accent)] hover:underline bg-transparent border-0 p-0 cursor-pointer" onClick={() => onSelect(a.hostId)}>{a.hostname}</button>
            <span className="text-[var(--text-secondary)] flex-1 min-w-[200px]">{a.reason}</span>
            {a.fix === 'recheck' ? (
              <button type="button" className="btn-secondary text-xs" disabled={busy} onClick={() => onRecheck(a.hostId)} data-testid={`recheck-${a.hostname}`}>Re-check</button>
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  )
}
