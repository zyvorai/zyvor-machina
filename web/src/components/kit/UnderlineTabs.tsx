// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, type ReactNode } from 'react'

export type UnderlineTab<T extends string> = { id: T; label: string; icon?: ReactNode; count?: number }

/** Scrollable underline tab bar (same look as the top bar / detail tabs); keeps the active tab in view. */
export default function UnderlineTabs<T extends string>({
  tabs,
  value,
  onChange,
  label = 'Sections',
  trailing,
  className = '',
}: {
  tabs: Array<UnderlineTab<T>>
  value: T
  onChange: (id: T) => void
  label?: string
  /** Right-aligned extras (links, buttons) on the same row. */
  trailing?: ReactNode
  className?: string
}) {
  const listRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>('[aria-selected="true"]')
    el?.scrollIntoView?.({ block: 'nearest', inline: 'center' })
  }, [value])
  return (
    <div className={`flex items-center gap-4 border-b border-[var(--apple-hairline)] ${className}`}>
      <div ref={listRef} role="tablist" aria-label={label} className="flex min-w-0 flex-1 overflow-x-auto [scrollbar-width:none]">
        {tabs.map((t) => {
          const active = value === t.id
          return (
            <button
              key={t.id}
              type="button"
              role="tab"
              aria-selected={active}
              onClick={() => onChange(t.id)}
              className={`relative inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap px-3.5 min-h-11 text-[13px] transition-colors after:absolute after:left-3.5 after:right-3.5 after:-bottom-px after:h-0.5 after:rounded-full ${
                active
                  ? 'text-[var(--text-primary)] font-medium after:bg-[var(--apple-link,#0066cc)]'
                  : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)] after:bg-transparent'
              }`}
            >
              {t.icon}
              {t.label}
              {t.count != null ? <span className="rounded-full bg-[var(--apple-fill-tertiary)] px-1.5 text-[11px] tabular-nums text-[var(--text-muted)]">{t.count}</span> : null}
            </button>
          )
        })}
      </div>
      {trailing ? <div className="flex shrink-0 items-center gap-3 pb-1">{trailing}</div> : null}
    </div>
  )
}
