// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type RefObject } from 'react'

export type PlatformFloatingAlign = 'start' | 'end'

interface PlatformFloatingMenuStyle {
  top?: number
  left?: number
  bottom?: number
  right?: number
  width?: number
}

interface UsePlatformFloatingMenuOptions {
  open: boolean
  triggerRef: RefObject<HTMLElement | null>
  menuRef: RefObject<HTMLElement | null>
  align?: PlatformFloatingAlign
  sideOffset?: number
  viewportMargin?: number
  matchTriggerWidth?: boolean
}

const DEFAULT_DOCK_RESERVE_PX = 80

function readBottomReserve(): number {
  if (typeof document === 'undefined') return DEFAULT_DOCK_RESERVE_PX
  const dock = document.querySelector('.mac-dock-inner')
  if (dock) {
    const rect = dock.getBoundingClientRect()
    if (rect.height > 0) return rect.height + 12
  }
  return DEFAULT_DOCK_RESERVE_PX
}

function computePlatformFloatingPosition(
  triggerRect: DOMRect,
  menuRect: DOMRect,
  opts: { align: PlatformFloatingAlign; sideOffset: number; viewportMargin: number },
): PlatformFloatingMenuStyle {
  const { align, sideOffset, viewportMargin } = opts
  const bottomReserve = readBottomReserve()
  const vw = window.innerWidth
  const vh = window.innerHeight

  const spaceBelow = vh - triggerRect.bottom - bottomReserve
  const spaceAbove = triggerRect.top - viewportMargin
  const openBelow = spaceBelow >= menuRect.height + sideOffset || spaceBelow >= spaceAbove

  let top: number | undefined
  let bottom: number | undefined
  if (openBelow) {
    top = Math.min(triggerRect.bottom + sideOffset, vh - bottomReserve - menuRect.height - viewportMargin)
    top = Math.max(viewportMargin, top)
  } else {
    bottom = Math.max(bottomReserve + viewportMargin, vh - triggerRect.top + sideOffset)
  }

  let left: number | undefined
  let right: number | undefined
  if (align === 'end') {
    left = triggerRect.right - menuRect.width
    if (left < viewportMargin) left = viewportMargin
    if (left + menuRect.width > vw - viewportMargin) {
      left = vw - viewportMargin - menuRect.width
    }
  } else {
    left = triggerRect.left
    if (left + menuRect.width > vw - viewportMargin) {
      left = vw - viewportMargin - menuRect.width
    }
    if (left < viewportMargin) left = viewportMargin
  }

  if (left != null && left + menuRect.width > vw - viewportMargin) {
    right = viewportMargin
    left = undefined
  }

  return { top, left, bottom, right }
}

export function usePlatformFloatingMenu({
  open,
  triggerRef,
  menuRef,
  align = 'start',
  sideOffset = 4,
  viewportMargin = 8,
  matchTriggerWidth = false,
}: UsePlatformFloatingMenuOptions) {
  const [style, setStyle] = useState<PlatformFloatingMenuStyle>({})
  const rafRef = useRef<number | null>(null)

  const updatePosition = useCallback(() => {
    const trigger = triggerRef.current
    const menu = menuRef.current
    if (!open || !trigger || !menu) return

    const triggerRect = trigger.getBoundingClientRect()
    const menuRect = menu.getBoundingClientRect()
    const next = computePlatformFloatingPosition(triggerRect, menuRect, {
      align,
      sideOffset,
      viewportMargin,
    })
    if (matchTriggerWidth) {
      setStyle({ ...next, width: triggerRect.width })
      return
    }
    setStyle(next)
  }, [open, triggerRef, menuRef, align, sideOffset, viewportMargin, matchTriggerWidth])

  const scheduleUpdate = useCallback(() => {
    if (rafRef.current != null) cancelAnimationFrame(rafRef.current)
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = null
      updatePosition()
    })
  }, [updatePosition])

  useLayoutEffect(() => {
    if (!open) {
      setStyle({})
      return
    }
    scheduleUpdate()
  }, [open, scheduleUpdate])

  useEffect(() => {
    if (!open) return
    const onScroll = () => scheduleUpdate()
    const onResize = () => scheduleUpdate()
    window.addEventListener('scroll', onScroll, true)
    window.addEventListener('resize', onResize)
    return () => {
      window.removeEventListener('scroll', onScroll, true)
      window.removeEventListener('resize', onResize)
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current)
    }
  }, [open, scheduleUpdate])

  const menuStyle: CSSProperties = {
    position: 'fixed',
    top: style.top,
    left: style.left,
    bottom: style.bottom,
    right: style.right,
    width: style.width,
    zIndex: 500,
  }

  return { menuStyle, updatePosition }
}
