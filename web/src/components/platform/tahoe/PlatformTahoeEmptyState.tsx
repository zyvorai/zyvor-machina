// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { LucideIcon } from 'lucide-react'
import type { ReactNode } from 'react'

interface PlatformTahoeEmptyStateProps {
  icon: LucideIcon
  title: string
  description: string
  primaryAction?: { label: string; onClick: () => void }
  secondaryAction?: { label: string; onClick: () => void }
  hint?: ReactNode
  children?: ReactNode
}

export default function PlatformTahoeEmptyState({
  icon: Icon,
  title,
  description,
  primaryAction,
  secondaryAction,
  hint,
  children,
}: PlatformTahoeEmptyStateProps) {
  return (
    <div className="apple-empty">
      <div className="apple-empty-icon">
        <Icon className="h-7 w-7" strokeWidth={1.5} />
      </div>
      <h2 className="apple-empty-title">{title}</h2>
      <p className="apple-empty-copy">{description}</p>
      {(primaryAction || secondaryAction || children) && (
        <div className="apple-cta-row justify-center mt-8">
          {primaryAction ? (
            <button type="button" onClick={primaryAction.onClick} className="btn-primary">
              {primaryAction.label}
            </button>
          ) : null}
          {secondaryAction ? (
            <button type="button" onClick={secondaryAction.onClick} className="btn-secondary">
              {secondaryAction.label}
            </button>
          ) : null}
          {children}
        </div>
      )}
      {hint ? <div className="mt-6 text-[13px] text-[var(--text-faint)]">{hint}</div> : null}
    </div>
  )
}
