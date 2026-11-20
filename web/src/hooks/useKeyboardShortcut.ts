// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect } from 'react'

interface ShortcutOptions {
  key: string
  ctrl?: boolean
  meta?: boolean
  alt?: boolean
  handler: (e: KeyboardEvent) => void
  enabled?: boolean
}

function isInputFocused(): boolean {
  const el = document.activeElement
  if (!el) return false
  const tag = el.tagName.toLowerCase()
  if (tag === 'input' || tag === 'textarea' || tag === 'select') return true
  if ((el as HTMLElement).isContentEditable) return true
  return false
}

export function useKeyboardShortcut({ key, ctrl, meta, alt, handler, enabled = true }: ShortcutOptions) {
  useEffect(() => {
    if (!enabled) return

    const onKeyDown = (e: KeyboardEvent) => {
      const wantCtrl = ctrl || false
      const wantMeta = meta || false
      const wantAlt = alt || false

      if (wantCtrl && !e.ctrlKey && !e.metaKey) return
      if (wantMeta && !e.metaKey && !e.ctrlKey) return
      if (wantAlt && !e.altKey) return
      if (e.key.toLowerCase() !== key.toLowerCase()) return

      // Bare-key shortcuts (no Ctrl/Meta) must not hijack typing in form fields —
      // otherwise printable keys like "?" never reach the focused input/textarea.
      // Guard BEFORE preventDefault so the keystroke still lands in the field.
      if (!wantCtrl && !wantMeta && isInputFocused()) return

      e.preventDefault()
      handler(e)
    }

    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [key, ctrl, meta, alt, handler, enabled])
}

export { isInputFocused }
