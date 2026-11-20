// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { LucideIcon } from 'lucide-react'
import type { ReactNode } from 'react'
import PlatformEmptyState from './PlatformEmptyState'
import { MacGlassPanel } from './mac/PlatformMacUi'

export default function GlassDataTable({
  title,
  subtitle,
  columns,
  children,
  empty,
  isEmpty,
  action,
}: {
  title?: string
  subtitle?: string
  columns: ReactNode
  children: ReactNode
  empty?: {
    icon?: LucideIcon
    title: string
    subtitle?: string
    action?: ReactNode
  }
  isEmpty?: boolean
  action?: ReactNode
}) {
  if (empty && isEmpty) {
    return (
      <PlatformEmptyState
        icon={empty.icon}
        title={empty.title}
        subtitle={empty.subtitle}
        action={empty.action}
      />
    )
  }

  return (
    <MacGlassPanel title={title} subtitle={subtitle} action={action}>
      <div className="overflow-x-auto -mx-1">
        <table className="w-full text-sm min-w-[420px]" aria-label={title}>
          <thead className="sticky top-0 z-10 bg-[var(--apple-surface-elevated)] backdrop-blur-sm">
            <tr className="text-[var(--text-muted)] border-b border-white/[0.06]">{columns}</tr>
          </thead>
          <tbody>{children}</tbody>
        </table>
      </div>
    </MacGlassPanel>
  )
}
