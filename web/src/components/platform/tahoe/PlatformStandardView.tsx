// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { LucideIcon } from 'lucide-react'
import type { ReactNode } from 'react'
import PlatformPageChrome, { platformStatSubtitle } from '../PlatformPageChrome'
import type { TahoeStat } from './tahoeTypes'

interface PlatformStandardViewProps {
  title: string
  description?: string
  eyebrow?: string
  actions?: ReactNode
  icon: LucideIcon
  stats?: TahoeStat[]
  badge?: ReactNode
  loading?: boolean
  error?: string | null
  onErrorRetry?: () => void
  className?: string
  contentClassName?: string
  children: ReactNode
}

export default function PlatformStandardView({
  title,
  description,
  eyebrow: _eyebrow,
  actions,
  icon: Icon,
  stats,
  badge,
  loading = false,
  error = null,
  onErrorRetry,
  className,
  contentClassName = 'space-y-4',
  children,
}: PlatformStandardViewProps) {
  const subtitle = (
    <span className="flex flex-wrap items-center gap-2 text-sm">
      {stats && stats.length > 0 ? platformStatSubtitle(stats) : null}
      {description ? <span className="text-[var(--text-muted)]">{description}</span> : null}
      {badge}
    </span>
  )

  return (
    <PlatformPageChrome
      className={className}
      loading={loading}
      error={error}
      onErrorRetry={onErrorRetry}
      title={title}
      subtitle={subtitle}
      icon={<Icon className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={actions}
      contentClassName={contentClassName}
    >
      {children}
    </PlatformPageChrome>
  )
}
