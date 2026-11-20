// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useRef, type ReactNode } from 'react'
import PlatformFloatingMenu from './PlatformFloatingMenu'
import { PlatformMacMenuItem } from './PlatformMenuItem'

export { PlatformMacMenuItem } from './PlatformMenuItem'

export default function PlatformMacMenuDropdown({
  label,
  open,
  onToggle,
  onClose,
  children,
}: {
  label: string
  open: boolean
  onToggle: () => void
  onClose: () => void
  children: ReactNode
}) {
  const triggerRef = useRef<HTMLButtonElement>(null)

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        onMouseDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          e.stopPropagation()
          onToggle()
        }}
        aria-expanded={open}
        aria-haspopup="menu"
        className={`mac-menu-item px-3 py-1.5 rounded-md text-[0.9375rem] font-medium ${open ? 'mac-menu-item-active' : ''}`}
      >
        {label}
      </button>
      <PlatformFloatingMenu
        open={open}
        onClose={onClose}
        triggerRef={triggerRef}
        align="start"
        sideOffset={4}
        ariaLabel={label}
        className="mac-menu-panel min-w-[240px] max-h-[min(70vh,32rem)] overflow-y-auto py-1.5 shadow-2xl"
      >
        {children}
      </PlatformFloatingMenu>
    </>
  )
}
