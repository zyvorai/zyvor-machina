// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, type RefObject } from 'react'
import { restoreFocus, useCaptureTrigger } from './useCaptureTrigger'

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'

function getFocusableElements(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    (el) => el.offsetParent !== null || el === document.activeElement,
  )
}

// Stack of currently-active traps (last = topmost). Escape only dismisses the
// topmost surface, so pressing Escape on a nested drawer/modal doesn't also
// close the parent underneath it.
const trapStack: object[] = []

/**
 * Trap Tab focus inside `containerRef` while `active`; restores focus on deactivate.
 * Pass `onEscape` to also close the dialog when the user presses Escape.
 */
export function useFocusTrap(
  containerRef: RefObject<HTMLElement | null>,
  active: boolean,
  onEscape?: () => void,
) {
  const escapeRef = useRef(onEscape)
  escapeRef.current = onEscape
  // Stable per-instance identity used to find this trap's position in the stack.
  const tokenRef = useRef<object>({})
  // Captured during render: a child's autoFocus has already moved focus by the time the effect runs.
  const triggerRef = useCaptureTrigger(active)

  useEffect(() => {
    if (!active || !containerRef.current) return

    const root = containerRef.current
    const token = tokenRef.current
    trapStack.push(token)

    const timer = window.setTimeout(() => {
      // Respect an autoFocus'd control (e.g. a type-to-confirm input): only move focus in when
      // nothing inside the container already has it.
      if (root.contains(document.activeElement)) return
      const nodes = getFocusableElements(root)
      nodes[0]?.focus()
    }, 0)

    const onTab = (e: KeyboardEvent) => {
      if (e.key !== 'Tab') return
      const nodes = getFocusableElements(root)
      if (nodes.length === 0) return

      const first = nodes[0]
      const last = nodes[nodes.length - 1]

      if (e.shiftKey) {
        if (document.activeElement === first) {
          e.preventDefault()
          last.focus()
        }
      } else if (document.activeElement === last) {
        e.preventDefault()
        first.focus()
      }
    }

    // Escape is bound on the document so it works even before focus lands inside
    // the panel; Tab handling stays scoped to the panel. Only the topmost active
    // trap responds, so Escape on a nested surface leaves the parent open.
    const onEscapeKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || !escapeRef.current) return
      if (trapStack[trapStack.length - 1] !== token) return
      e.preventDefault()
      escapeRef.current()
    }

    root.addEventListener('keydown', onTab)
    document.addEventListener('keydown', onEscapeKey)
    return () => {
      window.clearTimeout(timer)
      const i = trapStack.indexOf(token)
      if (i !== -1) trapStack.splice(i, 1)
      root.removeEventListener('keydown', onTab)
      document.removeEventListener('keydown', onEscapeKey)
      restoreFocus(triggerRef.current)
    }
  }, [active, containerRef, triggerRef])
}
