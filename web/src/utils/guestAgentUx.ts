// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { VmGuestHealthReport } from '../api/platform'

export const GUEST_TOAST_CHANNEL_ATTACH = 'Guest agent channel attach queued'

/** QGA fully active: channel + responding agent. */
export function qgaHealthy(report: VmGuestHealthReport | null | undefined): boolean {
  return Boolean(report && report.install_state === 'running' && report.agent_ping)
}

export function guestToolsStripVisible(
  guestHealth: VmGuestHealthReport | null | undefined,
  guestToolsStatus?: string | null,
): boolean {
  if (qgaHealthy(guestHealth)) return false
  if (guestHealth) return true
  const s = guestToolsStatus?.toLowerCase()
  return s !== 'healthy' && s !== 'installed'
}

export function guestToolsStatusLabel(status?: string | null): string {
  const s = status?.toLowerCase()
  if (s === 'healthy' || s === 'installed') return 'Active'
  if (s === 'channel_only') return 'Channel only'
  return '—'
}
