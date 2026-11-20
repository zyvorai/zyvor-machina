// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { VmTimelineEntry } from '../../api/platformVmTimeline'
import type { ConsoleHubSessionRow } from './ConsoleHubSessionHistory'

type Props = {
  sessions?: ConsoleHubSessionRow[]
  timeline?: VmTimelineEntry[]
  loading?: boolean
}

function fmt(iso: string): string {
  try {
    return new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  } catch {
    return iso
  }
}

type Row = { at: string; label: string; kind: string }

export default function MachineTimeline({ sessions = [], timeline = [], loading }: Props) {
  const rows: Row[] = [
    ...sessions.map((s) => ({
      at: s.started_at,
      label: s.ended_at ? `Console ${s.protocol} ended` : `Console ${s.protocol} opened`,
      kind: 'console',
    })),
    ...timeline.map((t) => ({
      at: t.created_at,
      label: t.label,
      kind: t.kind,
    })),
  ].sort((a, b) => new Date(b.at).getTime() - new Date(a.at).getTime()).slice(0, 20)

  if (loading) {
    return <p className="text-xs text-[var(--text-muted)]">Loading timeline…</p>
  }

  if (rows.length === 0) {
    return (
      <div className="rounded-lg border border-slate-800/80 p-3 text-xs text-[var(--text-muted)]">
        No machine events yet — console sessions and lifecycle events appear here.
      </div>
    )
  }

  return (
    <div>
      <p className="text-xs font-medium text-[var(--text-muted)] mb-2">Machine timeline</p>
      <ul className="space-y-2 text-xs">
        {rows.map((r, i) => (
          <li key={`${r.at}-${i}`} className="flex gap-3 text-[var(--text-muted)]">
            <span className="text-[var(--text-muted)] font-mono shrink-0 w-14">{fmt(r.at)}</span>
            <span className="text-[var(--text-secondary)]">{r.label}</span>
          </li>
        ))}
      </ul>
    </div>
  )
}
