// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { VmDoctorReport } from '../api/ai'
import type { VmGuestHealthReport, VmPendingConfig } from '../api/platform'
import { qgaHealthy } from './guestAgentUx'
import type { GuestAccessHints } from './guestAccessHints'
import type { NatRuleLike } from './vmPortForwardServices'

export type VmDetailBlocker =
  | 'pending_config'
  | 'guest_agent_offline'
  | 'guest_ip_missing'
  | 'ssh_not_exposed'

export function vmDetailBlockers(input: {
  pending?: VmPendingConfig | null
  guestHealth?: VmGuestHealthReport | null
  guestToolsStatus?: string | null
  observedState?: string
  guestIp?: string
  guestAccess?: GuestAccessHints | null
  portForwardRules?: NatRuleLike[]
}): VmDetailBlocker[] {
  const blockers: VmDetailBlocker[] = []
  if (input.pending?.needs_shutdown) blockers.push('pending_config')
  if (input.observedState === 'running' && !qgaHealthy(input.guestHealth)) {
    blockers.push('guest_agent_offline')
  }
  if (
    input.observedState === 'running'
    && input.guestAccess?.guest_ip_private
    && !input.guestIp?.trim()
  ) {
    blockers.push('guest_ip_missing')
  }
  // SSH NAT exposure is Access-tab setup, not a hard VM fault — omit from the
  // yellow "N blockers" badge (still surfaced in Access / connect hub).
  return blockers
}

export function buildVmSpotlightPrefill(input: {
  vmName: string
  observedState?: string
  guestIp?: string
  healthScore?: number | null
  doctor?: VmDoctorReport | null
  blockers?: VmDetailBlocker[]
}): string {
  const parts = [
    `VM ${input.vmName}`,
    input.observedState ? `state ${input.observedState}` : null,
    input.guestIp ? `IP ${input.guestIp}` : null,
    input.healthScore != null ? `health ${input.healthScore}/100` : null,
    input.doctor ? `doctor ${input.doctor.score_numeric}/100` : null,
  ].filter(Boolean)

  const blockerLabels: Record<VmDetailBlocker, string> = {
    pending_config: 'pending domain config — shut down to apply',
    guest_agent_offline: 'guest agent not responding',
    guest_ip_missing: 'guest IP not detected',
    ssh_not_exposed: 'SSH not exposed for laptop NAT access',
  }

  const blockers = (input.blockers ?? []).map((b) => blockerLabels[b])
  const intents = [
    blockers.length ? `Fix: ${blockers.join('; ')}` : null,
    'Open Cinema console',
    'Explain doctor score',
    'Expose SSH on hypervisor',
  ].filter(Boolean)

  return [...parts, ...intents].join(' · ')
}
