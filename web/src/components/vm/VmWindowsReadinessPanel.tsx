// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { CheckCircle2, CircleAlert } from 'lucide-react'
import type { VmHardwareWindowsReadiness } from '../../api/platform'

type Props = {
  report: VmHardwareWindowsReadiness
  rdpExposed?: boolean
  rdpHostPort?: number | null
}

export default function VmWindowsReadinessPanel({ report, rdpExposed, rdpHostPort }: Props) {
  if (!report.is_windows) return null

  const items = report.items.map((item) => {
    if (item.label === 'RDP exposure' || item.label === 'Display') {
      if (rdpExposed) {
        return {
          ...item,
          label: 'RDP exposure',
          status: rdpHostPort ? `Exposed :${rdpHostPort}` : 'Exposed',
          ok: true,
        }
      }
      if (item.label === 'Display' && !rdpExposed) {
        return { ...item, label: 'RDP exposure', status: 'Not exposed', ok: false }
      }
    }
    return item
  })

  const ready =
    report.ready &&
    items.filter((i) => i.label !== 'Display').every((i) => i.ok)

  return (
    <div
      className="rounded-lg border border-[var(--accent)]/40 bg-[var(--accent-soft)] p-3 space-y-2"
      data-testid="vm-windows-readiness-panel"
    >
      <div className="flex items-center justify-between gap-2">
        <p className="text-xs font-medium text-[var(--text-primary)]">Windows readiness</p>
        <span
          className={`text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded border ${
            ready
              ? 'border-emerald-500/40 text-emerald-600 bg-emerald-500/10'
              : 'border-amber-500/40 text-amber-600 bg-amber-500/10'
          }`}
          data-testid="vm-windows-readiness-status"
        >
          {ready ? 'Ready' : 'Needs attention'}
        </span>
      </div>
      <ul className="space-y-1.5">
        {items
          .filter((i) => i.label !== 'Display')
          .map((item) => (
            <li key={item.label} className="flex items-start gap-2 text-xs" data-testid={`vm-windows-readiness-${item.label.toLowerCase().replace(/\s+/g, '-')}`}>
              {item.ok ? (
                <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400 shrink-0 mt-0.5" />
              ) : (
                <CircleAlert className="w-3.5 h-3.5 text-amber-400 shrink-0 mt-0.5" />
              )}
              <span className="text-[var(--text-secondary)]">
                <span className="text-[var(--text-muted)]">{item.label}:</span> {item.status}
              </span>
            </li>
          ))}
      </ul>
    </div>
  )
}
