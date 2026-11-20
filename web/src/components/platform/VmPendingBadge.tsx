// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { VmPendingConfig } from '../../api/platform'

interface VmPendingBadgeProps {
  pending: VmPendingConfig | null
  category: string
}

export default function VmPendingBadge({ pending, category }: VmPendingBadgeProps) {
  if (!pending?.needs_shutdown) return null
  const hit = pending.pending_changes.some((c) => c.category === category)
  if (!hit) return null
  return (
    <span className="text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded border border-amber-500/40 text-amber-600 bg-amber-500/10">
      Pending
    </span>
  )
}
