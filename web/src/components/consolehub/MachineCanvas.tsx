// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ReactNode } from 'react'

type AuraTone = 'healthy' | 'warning' | 'failing' | 'migrating' | 'stopped' | 'snapshot'

export function machineAuraClass(tone: AuraTone): string {
  switch (tone) {
    case 'healthy':
      return 'ring-1 ring-[color-mix(in_srgb,var(--machina-status-ok)_35%,transparent)]'
    case 'warning':
      return 'ring-1 ring-[color-mix(in_srgb,var(--machina-status-warn)_40%,transparent)]'
    case 'failing':
      return 'ring-1 ring-[color-mix(in_srgb,var(--machina-status-error)_45%,transparent)]'
    case 'migrating':
      return 'ring-1 ring-[color-mix(in_srgb,var(--accent)_40%,transparent)]'
    case 'snapshot':
      return 'ring-1 ring-white/15'
    default:
      return 'ring-1 ring-[var(--apple-hairline)]'
  }
}

export function machineAuraTone(
  vmState?: string | null,
  healthScore?: number | null,
): AuraTone {
  const s = (vmState ?? '').toLowerCase()
  if (s.includes('migrat')) return 'migrating'
  if (s.includes('stop') || s.includes('shut')) return 'stopped'
  if (s.includes('fail') || s.includes('crash') || s.includes('error')) return 'failing'
  if (healthScore != null && healthScore < 60) return 'failing'
  if (healthScore != null && healthScore < 85) return 'warning'
  if (s.includes('run') || s === 'active') return 'healthy'
  return 'stopped'
}

type Props = {
  children: ReactNode
  vmState?: string | null
  healthScore?: number | null
  theatre?: boolean
  className?: string
}

export default function MachineCanvas({ children, vmState, healthScore, theatre, className = '' }: Props) {
  const tone = machineAuraTone(vmState, healthScore)
  return (
    <div
      className={`relative flex-1 min-h-0 rounded-xl overflow-hidden bg-[#0a0a0c] ${machineAuraClass(tone)} ${theatre ? 'min-h-[22rem]' : 'min-h-[50vh]'} ${className}`}
      style={{
        backgroundImage: 'radial-gradient(circle at 50% 50%, rgba(255,255,255,0.02) 1px, transparent 1px)',
        backgroundSize: '24px 24px',
      }}
    >
      <div className="absolute inset-0 flex flex-col min-h-0 overflow-hidden">
        {children}
      </div>
    </div>
  )
}
