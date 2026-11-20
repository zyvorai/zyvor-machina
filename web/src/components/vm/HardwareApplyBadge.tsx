// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { HardwareApplyBadgeMeta } from '../../utils/hardwareApplyBadges'
import { badgeToneClasses } from '../../utils/hardwareApplyBadges'

type Props = {
  badge: HardwareApplyBadgeMeta
  compact?: boolean
}

export default function HardwareApplyBadge({ badge, compact = false }: Props) {
  return (
    <span
      className={`${compact ? 'text-[9px]' : 'text-[10px]'} uppercase tracking-wide px-1.5 py-0.5 rounded border shrink-0 ${badgeToneClasses(badge.tone)}`}
      title={badge.title}
      data-testid={`hardware-badge-${badge.id}`}
    >
      {badge.label}
    </span>
  )
}
