// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { cinemaHubPath, studioHubPath } from '../../../utils/consoleExperienceMode'

/** Display order for Cockpit-style console lenses in Machine Finder. */
const CONSOLE_PROTOCOL_ORDER = ['novnc', 'spice', 'webrtc_spice', 'serial', 'native_ssh'] as const

export const CONSOLE_PROTOCOL_LABELS: Record<string, string> = {
  novnc: 'VNC',
  spice: 'SPICE',
  webrtc_spice: 'Performance',
  serial: 'Serial',
  native_ssh: 'SSH',
  rdp: 'RDP',
}

export function consoleHubPath(vmId: string, protocol?: string, popout = false): string {
  const extra: Record<string, string> = {}
  if (protocol) extra.protocol = protocol
  if (popout) extra.popout = '1'
  const hub = protocol === 'serial' ? studioHubPath : cinemaHubPath
  return hub(vmId, Object.keys(extra).length ? extra : undefined)
}

export function sortedDisplayProtocols(protocols: string[], recommended: string): string[] {
  const set = new Set(protocols)
  if (recommended && !set.has(recommended)) set.add(recommended)
  const ordered: string[] = CONSOLE_PROTOCOL_ORDER.filter((p) => set.has(p))
  for (const p of set) {
    if (!ordered.includes(p) && (p === 'rdp' || p === 'native_ssh')) ordered.push(p)
  }
  return ordered
}

export function consoleStatusLabel(protocols: string[], recommended: string): string {
  const display = sortedDisplayProtocols(protocols, recommended)
  if (display.length === 0) return 'Console unavailable'
  return display.map((p) => CONSOLE_PROTOCOL_LABELS[p] ?? p).join(' · ')
}
