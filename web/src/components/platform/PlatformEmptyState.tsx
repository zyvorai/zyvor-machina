// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { LucideIcon } from 'lucide-react'
import { MacGlassPanel } from './mac/PlatformMacUi'
import PlatformTahoeEmptyState from './tahoe/PlatformTahoeEmptyState'

export default function PlatformEmptyState({
  title,
  subtitle,
  children,
  action,
  icon,
}: {
  title: string
  subtitle?: string
  children?: React.ReactNode
  action?: React.ReactNode
  icon?: LucideIcon
}) {
  if (icon) {
    return (
      <PlatformTahoeEmptyState
        icon={icon}
        title={title}
        description={subtitle ?? ''}
      >
        {action}
        {children}
      </PlatformTahoeEmptyState>
    )
  }

  return (
    <MacGlassPanel title={title} subtitle={subtitle} action={action}>
      {children && <div className="text-sm text-[var(--text-muted)] leading-relaxed -mt-2">{children}</div>}
    </MacGlassPanel>
  )
}
