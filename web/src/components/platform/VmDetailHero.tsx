// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Activity, Shield, Sparkles, Terminal, Zap } from 'lucide-react'
import type { VmDetailBlocker } from '../../utils/vmDetailSpotlight'

type Props = {
  observedState: string
  guestIp?: string
  healthScore?: number | null
  doctorScore?: number | null
  sshExposed?: boolean
  blockers?: VmDetailBlocker[]
  onOpenAccess?: () => void
  onOpenDoctor?: () => void
}

/**
 * Readiness/health/doctor pill row for VM detail — the name/state/host/IP
 * are already shown once in PlatformVmDetail's PageLayout header; this only
 * adds what that header doesn't have.
 */
export default function VmDetailHero({
  observedState,
  guestIp,
  healthScore,
  doctorScore,
  sshExposed,
  blockers = [],
  onOpenAccess,
  onOpenDoctor,
}: Props) {
  const running = observedState === 'running'
  const blockerCount = blockers.length
  const ready = running && Boolean(guestIp?.trim()) && (sshExposed || !blockers.includes('ssh_not_exposed'))

  return (
    <div className="vm-detail-hero apple-page-actions border-b border-[var(--apple-hairline)] pb-8 mb-2 animate-fade-in" data-testid="vm-detail-hero">
      {ready ? (
        <span className="vm-detail-hero-pill vm-detail-hero-pill--ok">
          <Zap className="w-3.5 h-3.5" /> Ready to connect
        </span>
      ) : blockerCount > 0 ? (
        <button
          type="button"
          className="vm-detail-hero-pill vm-detail-hero-pill--warn"
          onClick={onOpenAccess}
        >
          <Sparkles className="w-3.5 h-3.5" />
          {blockerCount} blocker{blockerCount === 1 ? '' : 's'} — fix in Access
        </button>
      ) : null}
      {healthScore != null && !Number.isNaN(healthScore) && (
        <span className="vm-detail-hero-pill">
          <Activity className="w-3.5 h-3.5 text-[var(--link)]" />
          Health {healthScore}
        </span>
      )}
      {doctorScore != null && (
        <button type="button" className="vm-detail-hero-pill" onClick={onOpenDoctor}>
          <Shield className="w-3.5 h-3.5 text-[var(--accent)]" />
          Doctor {doctorScore}/100
        </button>
      )}
      {sshExposed && (
        <span className="vm-detail-hero-pill vm-detail-hero-pill--ok">
          <Terminal className="w-3.5 h-3.5" /> SSH exposed
        </span>
      )}
    </div>
  )
}
