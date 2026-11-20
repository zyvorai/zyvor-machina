// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import HardwareApplyBadge from './HardwareApplyBadge'
import { resolveHardwareBadges } from '../../utils/hardwareApplyBadges'

type Props = {
  label: string
  value: string
  badges?: string[]
  /** @deprecated use badges from backend report */
  badge?: 'pending' | 'restart' | null
  testId?: string
}

export default function VmHardwareSection({ label, value, badges, badge, testId }: Props) {
  const resolved = resolveHardwareBadges(badges)
  const legacyRestart = badge === 'pending' || badge === 'restart'

  return (
    <div className="flex items-start justify-between gap-3 py-2 border-b border-white/[0.06] last:border-0" data-testid={testId}>
      <span className="text-xs text-[var(--text-muted)] shrink-0 w-28">{label}</span>
      <div className="flex flex-wrap items-center justify-end gap-1.5 min-w-0 flex-1">
        <span className="text-xs text-[var(--text-primary)] text-right">{value}</span>
        {resolved.map((b) => (
          <HardwareApplyBadge key={b.id} badge={b} />
        ))}
        {legacyRestart && !resolved.some((b) => b.id === 'restart_required') ? (
          <span className="text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded border border-amber-500/40 text-amber-600 bg-amber-500/10 shrink-0">
            Restart needed
          </span>
        ) : null}
      </div>
    </div>
  )
}
