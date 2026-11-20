// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { vmStatusBadgeClasses } from '../utils/vmVisual'

interface VmStatusBadgeProps {
  state: string | undefined | null
  /** solid = filled pill; soft = glass-friendly tint */
  variant?: 'soft' | 'solid'
  className?: string
}

export default function VmStatusBadge({ state, variant = 'soft', className = '' }: VmStatusBadgeProps) {
  const label = (state ?? 'unknown').replace(/_/g, ' ')
  return (
    <span className={`${vmStatusBadgeClasses(state, variant)} ${className}`.trim()} title={label}>
      {label}
    </span>
  )
}
