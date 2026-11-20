// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ReactNode } from 'react'

type Props = {
  icon?: ReactNode
  title: string
  description?: ReactNode
  primaryAction?: ReactNode
  secondaryAction?: ReactNode
  className?: string
}

/** Centered empty state — Story-tier calm, Law 0 white surface. */
export default function EmptyState({
  icon,
  title,
  description,
  primaryAction,
  secondaryAction,
  className = '',
}: Props) {
  return (
    <div className={`apple-empty ${className}`}>
      {icon ? <div className="apple-empty-icon">{icon}</div> : null}
      <h2 className="apple-empty-title">{title}</h2>
      {description ? <p className="apple-empty-copy">{description}</p> : null}
      {(primaryAction || secondaryAction) ? (
        <div className="apple-cta-row justify-center mt-8">
          {primaryAction}
          {secondaryAction}
        </div>
      ) : null}
    </div>
  )
}
