// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// apple.com Story / hub primitives — single composition, no card grids.

import type { ReactNode } from 'react'
import { Link } from 'react-router'
import { ChevronRight } from 'lucide-react'

export function AppleStoryHeader({
  eyebrow,
  title,
  lede,
  cta,
  centered = false,
  as: Title = 'h1',
}: {
  eyebrow?: string
  title: string
  lede?: string
  cta?: ReactNode
  /** Page-level hero (not a sub-section): use the centered AirPods-style hero band. */
  centered?: boolean
  /** Heading level. Default h1; use h2 when the page's own header already renders the h1. */
  as?: 'h1' | 'h2'
}) {
  return (
    <header className={`apple-story-stack mb-8 ${centered ? 'apple-hero-band' : 'max-w-3xl'}`}>
      {eyebrow ? (
        <p
          className={
            centered
              ? 'apple-eyebrow'
              : 'text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)] mb-2'
          }
        >
          {eyebrow}
        </p>
      ) : null}
      <Title className="apple-display text-[var(--text-primary)] tracking-tight">{title}</Title>
      {lede ? <p className="apple-lede text-[var(--text-secondary)] mt-3">{lede}</p> : null}
      {cta ? <div className={centered ? 'apple-cta-row' : 'mt-5 flex flex-wrap gap-3'}>{cta}</div> : null}
    </header>
  )
}

type AppleDestinationItem = {
  to: string
  title: string
  subtitle?: string
  icon?: ReactNode
}

/** Linear destination rows for hubs — replaces NavCard grids. */
export function AppleDestinationList({ items }: { items: AppleDestinationItem[] }) {
  return (
    <ul className="divide-y divide-[var(--apple-hairline)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] overflow-hidden">
      {items.map((item) => (
        <li key={item.to}>
          <Link
            to={item.to}
            className="group flex items-center gap-3 px-4 py-3.5 hover:bg-[var(--apple-fill-tertiary)]/60 transition"
          >
            {item.icon ? (
              <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-[var(--apple-fill-tertiary)] text-[var(--text-secondary)] group-hover:text-[var(--text-primary)]">
                {item.icon}
              </span>
            ) : null}
            <span className="min-w-0 flex-1">
              <span className="block text-sm font-medium text-[var(--text-primary)]">{item.title}</span>
              {item.subtitle ? (
                <span className="block text-xs text-[var(--text-muted)] mt-0.5">{item.subtitle}</span>
              ) : null}
            </span>
            <ChevronRight className="w-4 h-4 shrink-0 text-[var(--text-muted)] group-hover:text-[var(--text-secondary)]" aria-hidden />
          </Link>
        </li>
      ))}
    </ul>
  )
}
