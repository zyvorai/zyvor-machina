// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ReactNode } from 'react'
import ErrorBanner from './ErrorBanner'
import PageSkeleton from './PageSkeleton'
import { useNavGroupEyebrow } from '../hooks/useNavGroupEyebrow'

type PageLayoutProps = {
  title?: string
  subtitle?: ReactNode
  /** Quiet product line above the title (apple.com eyebrow). */
  eyebrow?: string
  icon?: ReactNode
  actions?: ReactNode
  children?: ReactNode
  /** Full-page skeleton; use for pages with no meaningful shell during load. */
  loading?: boolean
  /** Spinner in the content area while keeping header and actions visible. */
  contentLoading?: boolean
  error?: string | null
  errorTitle?: string
  errorHints?: string[]
  technicalDetail?: string | null
  errorTone?: 'amber' | 'red'
  onErrorRetry?: () => void
  onErrorDismiss?: () => void
  emptyState?: ReactNode
  /** Rendered before errors and header (e.g. Fleet Cloud sub-nav). */
  prepend?: ReactNode
  /** Tighter vertical spacing (e.g. embedded platform panels). */
  compact?: boolean
  /** When set, skip the default title row (e.g. page uses `<Hero>` in children). */
  hideHeader?: boolean
  className?: string
  contentClassName?: string
}

function ContentSpinner() {
  return (
    <div className="flex items-center justify-center h-40" aria-busy="true" aria-label="Loading">
      <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-[var(--accent)]" />
    </div>
  )
}

/** Default page shell — apple.com Browse/Work rhythm for every route. */
export default function PageLayout({
  title,
  subtitle,
  eyebrow,
  icon,
  actions,
  children,
  loading,
  contentLoading,
  error,
  errorTitle,
  errorHints,
  technicalDetail,
  errorTone,
  onErrorRetry,
  onErrorDismiss,
  emptyState,
  prepend,
  hideHeader,
  compact,
  className,
  contentClassName,
}: PageLayoutProps) {
  const navEyebrow = useNavGroupEyebrow()
  if (loading) {
    return <PageSkeleton />
  }

  return (
    <div
      className={`apple-page ${compact ? 'apple-page--compact' : ''} animate-fade-in ${className ?? ''}`}
    >
      {prepend}
      {error ? (
        <ErrorBanner
          title={errorTitle}
          headline={error}
          hints={errorHints}
          technicalDetail={technicalDetail ?? undefined}
          tone={errorTone}
          onRetry={onErrorRetry}
          onDismiss={onErrorDismiss}
        />
      ) : null}

      {!hideHeader ? (
        <header className="apple-page-header">
          <div className="min-w-0 flex-1 max-w-3xl">
            {eyebrow ?? navEyebrow ? <p className="apple-eyebrow">{eyebrow ?? navEyebrow}</p> : null}
            {icon ? (
              <div className="flex items-center gap-3">
                <span className="shrink-0 text-[var(--text-muted)]" aria-hidden>{icon}</span>
                <h1 className="page-title">{title}</h1>
              </div>
            ) : (
              <h1 className="page-title">{title}</h1>
            )}
            {subtitle ? <div className="page-lede">{subtitle}</div> : null}
          </div>
          {actions ? (
            <div className="apple-page-actions">{actions}</div>
          ) : null}
        </header>
      ) : null}

      {contentLoading ? (
        <ContentSpinner />
      ) : emptyState ? (
        emptyState
      ) : (
        <div className={`apple-page-body ${contentClassName ?? ''}`}>{children}</div>
      )}
    </div>
  )
}
