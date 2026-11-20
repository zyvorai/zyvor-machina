// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { CSSProperties, ReactNode } from 'react'

/** Rounded card around a table: horizontal scroll with a min width, plus an optional title row. */
export default function TableCard({
  title,
  subtitle,
  actions,
  minWidth = 640,
  children,
  className = '',
}: {
  title?: string
  subtitle?: string
  actions?: ReactNode
  minWidth?: number
  children: ReactNode
  className?: string
}) {
  return (
    <section className={`overflow-hidden rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] ${className}`}>
      {title || actions ? (
        <header className="flex items-center justify-between gap-3 border-b border-[var(--apple-hairline)] px-4 py-3">
          <div className="min-w-0">
            {title ? <h2 className="truncate text-sm font-semibold text-[var(--text-primary)]">{title}</h2> : null}
            {subtitle ? <p className="mt-0.5 truncate text-xs text-[var(--text-muted)]">{subtitle}</p> : null}
          </div>
          {actions ? <div className="flex shrink-0 items-center gap-2">{actions}</div> : null}
        </header>
      ) : null}
      <div className="overflow-x-auto" style={{ ['--tc-min']: `${minWidth}px` } as CSSProperties}>
        <div className="[&_table]:min-w-[var(--tc-min)] [&_th]:whitespace-nowrap [&_td]:tabular-nums">{children}</div>
      </div>
    </section>
  )
}
