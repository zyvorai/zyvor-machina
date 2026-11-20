// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useRef } from 'react'

/**
 * Remembers which element had focus at the moment an overlay (dialog, drawer, sheet) opened,
 * so it can be handed focus back when the overlay closes.
 *
 * The capture happens during render, on the false -> true transition of `active`, and NOT inside
 * an effect. React applies a child's `autoFocus` at commit, before any effect runs, so an
 * effect-time `document.activeElement` is already the dialog's own input or button, which is
 * about to unmount; restoring focus to it drops keyboard users onto <body>.
 */
export function useCaptureTrigger(active: boolean) {
  const trigger = useRef<HTMLElement | null>(null)
  const wasActive = useRef(false)
  if (active && !wasActive.current) {
    trigger.current = typeof document !== 'undefined' && document.activeElement instanceof HTMLElement ? document.activeElement : null
  }
  wasActive.current = active
  return trigger
}

/** Focus `el` if it is still in the document (a delete-confirm may have removed its row). */
export function restoreFocus(el: HTMLElement | null | undefined) {
  if (el && el.isConnected) el.focus?.()
}
