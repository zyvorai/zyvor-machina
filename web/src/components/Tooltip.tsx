// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useRef, useState, type ReactNode } from 'react'

const SHOW_DELAY_MS = 500

type Props = {
  label: string
  children: ReactNode
  position?: 'top' | 'bottom'
}

/** Small macOS-style hover tooltip — shows after a short delay, positioned
 * relative to its trigger. Wrap a single icon-only button; keep the button's
 * own `title=`/`aria-label` in place as the accessibility fallback. */
export default function Tooltip({ label, children, position = 'top' }: Props) {
  const [visible, setVisible] = useState(false)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const show = () => {
    if (timerRef.current) clearTimeout(timerRef.current)
    timerRef.current = setTimeout(() => setVisible(true), SHOW_DELAY_MS)
  }
  const hide = () => {
    if (timerRef.current) clearTimeout(timerRef.current)
    timerRef.current = null
    setVisible(false)
  }

  return (
    <span className="mac-tooltip-anchor" onMouseEnter={show} onMouseLeave={hide} onFocus={show} onBlur={hide}>
      {children}
      {visible && (
        <span className={`mac-tooltip mac-tooltip-${position}`} role="tooltip">
          {label}
        </span>
      )}
    </span>
  )
}
