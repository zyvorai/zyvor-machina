// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ReactNode } from 'react'
import { Link } from 'react-router'

/** Accent used for selected state (ring + border + tint). */
type ChoiceTone = 'blue' | 'amber' | 'sky' | 'cyan' | 'purple' | 'emerald' | 'slate' | 'violet'

/** Selected shell: hairline surface + accent border — never accent-on-accent wash. */
const selectedClass: Record<ChoiceTone, string> = {
  blue: 'border-[var(--accent)] bg-[var(--apple-surface)] ring-1 ring-[color-mix(in_srgb,var(--accent)_35%,transparent)]',
  amber: 'border-[var(--amber)] bg-[var(--apple-surface)] ring-1 ring-[color-mix(in_srgb,var(--amber)_35%,transparent)]',
  sky: 'border-[var(--accent)] bg-[var(--apple-surface)] ring-1 ring-[color-mix(in_srgb,var(--accent)_35%,transparent)]',
  cyan: 'border-[var(--accent)] bg-[var(--apple-surface)] ring-1 ring-[color-mix(in_srgb,var(--accent)_35%,transparent)]',
  purple: 'border-[var(--accent)] bg-[var(--apple-surface)] ring-1 ring-[color-mix(in_srgb,var(--accent)_35%,transparent)]',
  emerald: 'border-[var(--verdant)] bg-[var(--apple-surface)] ring-1 ring-[color-mix(in_srgb,var(--verdant)_35%,transparent)]',
  slate: 'border-[var(--text-primary)] bg-[var(--apple-surface)] ring-1 ring-[var(--apple-hairline)]',
  violet: 'border-[var(--accent)] bg-[var(--apple-surface)] ring-1 ring-[color-mix(in_srgb,var(--accent)_35%,transparent)]',
}

/** Selected icon tile: solid accent + on-accent text (readable; no blue-on-blue). */
const iconSelectedClass: Record<ChoiceTone, string> = {
  blue: 'bg-[var(--accent)] text-[var(--text-on-accent)]',
  amber: 'bg-[var(--amber)] text-[var(--text-on-accent)]',
  sky: 'bg-[var(--accent)] text-[var(--text-on-accent)]',
  cyan: 'bg-[var(--accent)] text-[var(--text-on-accent)]',
  purple: 'bg-[var(--accent)] text-[var(--text-on-accent)]',
  emerald: 'bg-[var(--verdant)] text-[var(--text-on-accent)]',
  slate: 'bg-[var(--text-primary)] text-[var(--apple-surface)]',
  violet: 'bg-[var(--accent)] text-[var(--text-on-accent)]',
}

const iconIdleClass: Record<ChoiceTone, string> = {
  blue: 'bg-[var(--surface-hover)] text-[var(--text-secondary)]',
  amber: 'bg-[var(--apple-fill-secondary)] text-[var(--amber)]',
  sky: 'bg-[var(--surface-hover)] text-[var(--text-secondary)]',
  cyan: 'bg-[var(--apple-fill-secondary)] text-[var(--text-secondary)]',
  purple: 'bg-[var(--surface-hover)] text-[var(--text-secondary)]',
  emerald: 'bg-[var(--surface-hover)] text-[var(--text-secondary)]',
  slate: 'bg-[var(--surface-hover)] text-[var(--text-secondary)]',
  violet: 'bg-[var(--surface-hover)] text-[var(--text-secondary)]',
}

const baseUnselected = 'border-[var(--apple-hairline)] bg-[var(--apple-surface)] hover:border-[var(--border-strong)] hover:bg-[var(--surface-hover)]'

const baseButton =
  'rounded-xl border text-left transition flex flex-col gap-2 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--surface-0)] disabled:opacity-45 disabled:pointer-events-none'

export function ChoiceCardGrid({ children, className = '' }: { children: ReactNode; className?: string }) {
  return <div className={`grid grid-cols-1 sm:grid-cols-2 gap-3 ${className}`.trim()}>{children}</div>
}

/** Denser grid for many options (e.g. settings / VM detail tabs). */
export function ChoiceCardDenseGrid({ children, className = '' }: { children: ReactNode; className?: string }) {
  return (
    <div className={`grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 gap-2 ${className}`.trim()}>{children}</div>
  )
}

/** Same shell as an idle choice card, for navigation (e.g. NodeInfo quick links). */
export function ChoiceLinkCard({
  to,
  icon,
  title,
  description,
  className = '',
}: {
  to: string
  icon: ReactNode
  title: ReactNode
  description?: ReactNode
  className?: string
}) {
  return (
    <Link
      to={to}
      className={`${baseButton} p-3 gap-2 ${baseUnselected} hover:border-[var(--accent)]/45 hover:bg-[var(--apple-fill-tertiary)]/55 ${className}`.trim()}
    >
      <span className="flex items-center gap-2 text-sm font-medium text-[var(--text-primary)]">
        <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-[var(--surface-hover)] text-[var(--text-secondary)]">{icon}</span>
        {title}
      </span>
      {description ? <span className="text-xs text-[var(--text-muted)] leading-snug">{description}</span> : null}
    </Link>
  )
}

export function ChoiceCard({
  selected,
  onClick,
  icon,
  title,
  description,
  tone,
  disabled,
  compact,
  largeIcon,
  className = '',
}: {
  selected: boolean
  onClick: () => void
  icon: ReactNode
  title: ReactNode
  description?: ReactNode
  tone: ChoiceTone
  disabled?: boolean
  compact?: boolean
  /** Taller icon tile (e.g. primary flow pickers on Create VM). */
  largeIcon?: boolean
  className?: string
}) {
  const pad = compact ? 'p-2.5 gap-1.5' : 'p-4 gap-2'
  const minh = compact ? '' : 'min-h-[108px]'
  const iconBox = largeIcon ? 'h-10 w-10 shrink-0' : compact ? 'h-8 w-8 shrink-0' : 'h-9 w-9 shrink-0'
  const titleCls = compact ? 'text-sm font-medium text-[var(--text-primary)]' : largeIcon ? 'text-[var(--text-primary)] font-semibold' : 'text-[var(--text-primary)] font-medium'
  const descCls = compact
    ? 'text-[11px] text-[var(--text-muted)] leading-snug line-clamp-2'
    : largeIcon
      ? 'text-sm text-[var(--text-muted)] leading-snug'
      : 'text-xs text-[var(--text-muted)] leading-relaxed'

  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className={`${baseButton} ${pad} ${minh} ${selected ? selectedClass[tone] : baseUnselected} ${className}`.trim()}
    >
      <span className={`flex items-center gap-2 ${compact ? '' : ''}`}>
        <span
          className={`flex ${iconBox} items-center justify-center rounded-lg ${
            selected ? iconSelectedClass[tone] : iconIdleClass[tone]
          }`}
        >
          {icon}
        </span>
        <span className={titleCls}>{title}</span>
      </span>
      {description ? <span className={descCls}>{description}</span> : null}
    </button>
  )
}
