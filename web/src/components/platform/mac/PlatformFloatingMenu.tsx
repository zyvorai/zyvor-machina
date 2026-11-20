// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, type CSSProperties, type ReactNode, type RefObject } from 'react'
import { createPortal } from 'react-dom'
import { usePlatformFloatingMenu, type PlatformFloatingAlign } from '../../../hooks/usePlatformFloatingMenu'

interface PlatformFloatingMenuProps {
  open: boolean
  onClose: () => void
  triggerRef: RefObject<HTMLElement | null>
  panelRef?: RefObject<HTMLDivElement | null>
  align?: PlatformFloatingAlign
  sideOffset?: number
  matchTriggerWidth?: boolean
  className?: string
  role?: string
  ariaLabel?: string
  style?: CSSProperties
  'data-testid'?: string
  children: ReactNode
}

export default function PlatformFloatingMenu({
  open,
  onClose,
  triggerRef,
  panelRef,
  align = 'start',
  sideOffset = 4,
  matchTriggerWidth = false,
  className = '',
  role = 'menu',
  ariaLabel,
  style: styleOverride,
  'data-testid': testId,
  children,
}: PlatformFloatingMenuProps) {
  const internalMenuRef = useRef<HTMLDivElement>(null)
  const menuRef = panelRef ?? internalMenuRef
  const { menuStyle } = usePlatformFloatingMenu({
    open,
    triggerRef,
    menuRef,
    align,
    sideOffset,
    matchTriggerWidth,
  })

  useEffect(() => {
    if (!open) return
    const onDoc = (e: MouseEvent) => {
      if (
        triggerRef.current?.contains(e.target as Node) ||
        menuRef.current?.contains(e.target as Node)
      ) {
        return
      }
      onClose()
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose()
        triggerRef.current?.focus()
      }
    }
    const t = window.setTimeout(() => {
      document.addEventListener('mousedown', onDoc)
    }, 0)
    document.addEventListener('keydown', onKey)
    return () => {
      window.clearTimeout(t)
      document.removeEventListener('mousedown', onDoc)
      document.removeEventListener('keydown', onKey)
    }
  }, [open, onClose, triggerRef, menuRef])

  if (!open) return null

  const panelClass = ['mac-menu-panel', className].filter(Boolean).join(' ')

  return createPortal(
    <div
      ref={menuRef}
      role={role}
      aria-label={ariaLabel}
      data-testid={testId}
      className={panelClass}
      style={{ ...menuStyle, ...styleOverride }}
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => e.stopPropagation()}
    >
      {children}
    </div>,
    document.body,
  )
}
