// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//
// Thin Zeus TahoeListKit port — toolbar / table wrap / empty. Wrap existing
// Browse tables; do not rebuild list features.

import type { LucideIcon } from 'lucide-react'
import type { ReactNode } from 'react'
import { Search } from 'lucide-react'
import PlatformTahoeEmptyState from './PlatformTahoeEmptyState'

export function TahoeToolbar({
  search,
  onSearchChange,
  placeholder = 'Search…',
  trailing,
}: {
  search?: string
  onSearchChange?: (value: string) => void
  placeholder?: string
  trailing?: ReactNode
}) {
  return (
    <div className="tahoe-toolbar mb-4">
      {onSearchChange != null ? (
        <div className="tahoe-search-wrap flex-1 min-w-[10rem]">
          <Search className="h-4 w-4 shrink-0" aria-hidden />
          <input
            type="search"
            placeholder={placeholder}
            aria-label={placeholder}
            value={search ?? ''}
            onChange={(e) => onSearchChange(e.target.value)}
            className="tahoe-search"
          />
        </div>
      ) : null}
      {trailing}
    </div>
  )
}

export function TahoeTableWrap({ children, className = '' }: { children: ReactNode; className?: string }) {
  return (
    <div className={`tahoe-table-wrap tahoe-glass-card overflow-hidden ${className}`.trim()}>
      <div className="overflow-x-auto">{children}</div>
    </div>
  )
}

export function TahoeListEmpty({
  icon,
  title,
  description,
  primaryAction,
  secondaryAction,
}: {
  icon: LucideIcon
  title: string
  description: string
  primaryAction?: { label: string; onClick: () => void }
  secondaryAction?: { label: string; onClick: () => void }
}) {
  return (
    <PlatformTahoeEmptyState
      icon={icon}
      title={title}
      description={description}
      primaryAction={primaryAction}
      secondaryAction={secondaryAction}
    />
  )
}
