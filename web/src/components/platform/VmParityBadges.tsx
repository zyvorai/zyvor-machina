// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { PlatformVm } from '../../api/platform'
import { statusPillClasses } from '../../utils/semanticColors'

type Props = {
  vm: PlatformVm
  pendingShutdown?: boolean
  spice?: boolean
}

export default function VmParityBadges({ vm, pendingShutdown, spice }: Props) {
  const tags = vm.tags ?? []
  const badges: Array<{ key: string; label: string; tone: string }> = []

  if (tags.includes('define-only')) {
    badges.push({ key: 'install', label: 'Install pending', tone: statusPillClasses('warn') })
  }
  if (tags.includes('virt-install') || tags.includes('pxe-install') || tags.includes('url-install') || tags.includes('download-install')) {
    badges.push({ key: 'vi', label: 'virt-install', tone: statusPillClasses('info') })
  }
  if (pendingShutdown) {
    badges.push({ key: 'shutdown', label: 'Needs shutdown', tone: statusPillClasses('warn') })
  }
  if (spice) {
    badges.push({ key: 'spice', label: 'SPICE', tone: statusPillClasses('info') })
  }

  if (badges.length === 0) return null

  return (
    <div className="flex flex-wrap gap-1 mt-0.5">
      {badges.map((b) => (
        <span key={b.key} className={`px-1.5 py-0.5 rounded text-[10px] uppercase tracking-wide ${b.tone}`}>
          {b.label}
        </span>
      ))}
    </div>
  )
}
