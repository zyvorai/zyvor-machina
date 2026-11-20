// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { X } from 'lucide-react'

export default function PopoutTitleBar({ title, subtitle = 'Machina Platform' }: { title: string; subtitle?: string }) {
  return (
    <header className="mac-popout-titlebar flex items-center gap-3 px-3 py-2 border-b border-white/[0.08] bg-black/40 shrink-0 select-none">
      <div className="flex items-center gap-1.5 group">
        <button
          type="button"
          onClick={() => window.close()}
          className="mac-popout-traffic mac-popout-traffic-close group"
          title="Close window"
          aria-label="Close window"
        >
          <X className="h-2.5 w-2.5 opacity-0 group-hover:opacity-100 text-black/70" />
        </button>
        <span className="mac-popout-traffic mac-popout-traffic-minimize" aria-hidden />
        <span className="mac-popout-traffic mac-popout-traffic-maximize" aria-hidden />
      </div>
      <div className="flex-1 min-w-0 text-center">
        <span className="text-xs font-medium text-[var(--text-primary)] truncate block">{title}</span>
        <span className="text-[10px] text-[var(--text-muted)]">{subtitle}</span>
      </div>
      <div className="w-[52px] shrink-0" aria-hidden />
    </header>
  )
}
