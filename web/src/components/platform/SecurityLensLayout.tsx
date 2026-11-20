// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ReactNode } from 'react'
import { Link } from 'react-router'
import { ArrowLeft } from 'lucide-react'
import PlatformPageChrome, { PlatformRefreshButton, platformStatSubtitle } from './PlatformPageChrome'
import OperatingSurfaceLayout from './OperatingSurfaceLayout'
import PlatformEmptyState from './PlatformEmptyState'
import { MacGlassPanel } from './mac/PlatformMacUi'
import PlatformFilterPills from './PlatformFilterPills'
import { hubLinkClasses } from '../../utils/semanticColors'

type StatItem = {
  label: string
  value: string
  tone?: 'ok' | 'warn' | 'default'
  icon?: ReactNode
}

type FilterOption = { id: string; label: string }

export default function SecurityLensLayout({
  backHref = '/platform/zeus/security/firewall',
  backLabel = 'Firewall',
  title,
  subtitle,
  icon,
  stats,
  insight,
  filters,
  filterValue,
  onFilterChange,
  panelTitle,
  isEmpty,
  emptyTitle,
  emptySubtitle,
  children,
  onRefresh,
  loading,
  error,
  testId,
}: {
  backHref?: string
  backLabel?: string
  title: string
  subtitle?: string
  icon?: ReactNode
  stats?: StatItem[]
  insight?: ReactNode
  filters?: FilterOption[]
  filterValue?: string
  onFilterChange?: (id: string) => void
  panelTitle?: string
  isEmpty?: boolean
  emptyTitle?: string
  emptySubtitle?: string
  children?: ReactNode
  onRefresh: () => void
  loading?: boolean
  error?: string | null
  testId?: string
}) {
  return (
    <PlatformPageChrome
      loading={loading}
      error={error ?? null}
      onErrorRetry={onRefresh}
      prepend={
        <Link to={backHref} className={`text-sm inline-flex items-center gap-1 min-h-9 ${hubLinkClasses()}`}>
          <ArrowLeft className="w-4 h-4" /> {backLabel}
        </Link>
      }
      title={title}
      subtitle={
        stats && stats.length > 0
          ? (
              <span className="flex flex-col gap-1">
                {subtitle ? <span className="text-[var(--text-muted)]">{subtitle}</span> : null}
                {platformStatSubtitle(stats.map((s) => ({ label: s.label, value: s.value })))}
              </span>
            )
          : subtitle
      }
      icon={icon}
      actions={<PlatformRefreshButton onClick={onRefresh} />}
      contentClassName="space-y-4"
    >
      <OperatingSurfaceLayout testId={testId}>
        {insight}
        {filters && filterValue !== undefined && onFilterChange && (
          <PlatformFilterPills options={filters} value={filterValue} onChange={onFilterChange} />
        )}
        {isEmpty && emptyTitle ? (
          <PlatformEmptyState title={emptyTitle} subtitle={emptySubtitle} />
        ) : panelTitle ? (
          <MacGlassPanel title={panelTitle}>{children}</MacGlassPanel>
        ) : (
          children
        )}
      </OperatingSurfaceLayout>
    </PlatformPageChrome>
  )
}
