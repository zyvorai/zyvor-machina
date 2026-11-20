// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//
// Site-matching Zyvor lockup (orange swoosh + lowercase wordmark) for app chrome.
// Matches zyvor.dev gnav Mark — not the unused hex ZyvorAiLogo.

import { Link } from 'react-router'

const ACCENT = '#ff5a15'

const SIZES = {
  sm: { svg: 16, word: 'text-[13px]', gap: 'gap-1.5' },
  md: { svg: 18, word: 'text-[15px]', gap: 'gap-2' },
  lg: { svg: 28, word: 'text-[1.75rem]', gap: 'gap-2.5' },
  xl: { svg: 36, word: 'text-[2.25rem]', gap: 'gap-3' },
} as const

type ZyvorMarkSize = keyof typeof SIZES

type ZyvorMarkProps = {
  /** App route (default `/`). Pass `null` for a non-link mark. */
  to?: string | null
  size?: ZyvorMarkSize
  className?: string
  /** When false, render mark only. */
  showWordmark?: boolean
  /**
   * Wordmark color. Use `onDark` on black login / dark bands so light theme
   * tokens cannot turn “zyvor” into dark-on-black (invisible).
   */
  tone?: 'auto' | 'onDark' | 'onLight'
}

function Swoosh({ sizePx }: { sizePx: number }) {
  return (
    <svg width={sizePx} height={sizePx} viewBox="0 0 18 18" aria-hidden className="shrink-0">
      <path
        d="M2 2h14L6.6 16H16"
        fill="none"
        stroke={ACCENT}
        strokeWidth="2.6"
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  )
}

function wordColor(tone: ZyvorMarkProps['tone']): string {
  if (tone === 'onDark') return 'text-[#f5f5f7]'
  if (tone === 'onLight') return 'text-[#1d1d1f]'
  return 'text-[var(--text-primary,#f5f5f7)]'
}

/** Zyvor brand lockup — orange swoosh + lowercase `zyvor`, matching zyvor.dev. */
export function ZyvorMark({
  to = '/',
  size = 'md',
  className = '',
  showWordmark = true,
  tone = 'auto',
}: ZyvorMarkProps) {
  const s = SIZES[size]
  const inner = (
    <span className={`inline-flex items-center ${s.gap} ${className}`.trim()}>
      <Swoosh sizePx={s.svg} />
      {showWordmark ? (
        <span
          className={`zyvor-mark-word ${s.word} font-semibold tracking-[-0.03em] ${wordColor(tone)} lowercase leading-none`}
          style={{ fontFamily: "var(--font-display, 'Archivo', -apple-system, sans-serif)" }}
        >
          zyvor
        </span>
      ) : null}
    </span>
  )
  if (to == null) {
    return (
      <span className="inline-flex items-center" aria-label="Zyvor">
        {inner}
      </span>
    )
  }
  return (
    <Link
      to={to}
      aria-label="Zyvor home"
      className="inline-flex items-center shrink-0 rounded-md focus:outline-none focus-visible:ring-2 focus-visible:ring-orange-500/50"
    >
      {inner}
    </Link>
  )
}

export default ZyvorMark
