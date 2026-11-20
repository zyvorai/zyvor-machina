// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

export type ConsoleExperienceMode = 'cinema' | 'studio' | 'mission'

const MODE_PREF_PREFIX = 'machina-console-mode:'

export function parseConsoleMode(search: string): ConsoleExperienceMode {
  const m = new URLSearchParams(search).get('mode')
  if (m === 'studio' || m === 'mission') return m
  return 'cinema'
}

/** When URL has no `mode` param, restore the operator's last choice for this VM. */
export function resolveConsoleMode(search: string, vmId?: string | null): ConsoleExperienceMode {
  const params = new URLSearchParams(search)
  if (params.has('mode')) return parseConsoleMode(search)
  if (vmId) {
    const saved = loadConsoleModePreference(vmId)
    if (saved) return saved
  }
  return 'cinema'
}

export function loadConsoleModePreference(vmId: string): ConsoleExperienceMode | null {
  try {
    const raw = localStorage.getItem(`${MODE_PREF_PREFIX}${vmId}`)
    if (raw === 'cinema' || raw === 'studio' || raw === 'mission') return raw
  } catch {
    /* ignore */
  }
  return null
}

export function saveConsoleModePreference(vmId: string, mode: ConsoleExperienceMode): void {
  try {
    localStorage.setItem(`${MODE_PREF_PREFIX}${vmId}`, mode)
  } catch {
    /* quota */
  }
}

/** Minimal plan shape needed for default-lens / protocol selection. */
interface PlanSnapshot {
  native: { console_type: string; available: boolean }
  webrtc_spice_available: boolean
  protocols: string[]
  recommended?: string
}

/**
 * Cockpit pattern: choose the default console lens from VM capabilities.
 * Graphical display (VNC → SPICE) always wins over serial.
 * Serial is only the default when the VM has no display device at all.
 */
export function getDefaultLens(plan: PlanSnapshot): 'display' | 'serial' {
  if (plan.native.available || plan.native.console_type === 'vnc') return 'display'
  if (plan.webrtc_spice_available || plan.native.console_type === 'spice') return 'display'
  // A shutoff VM has no live VNC/SPICE port yet (autoport is assigned at start), so the
  // capability checks above are all false even for a graphical VM. The backend still
  // recommends a display protocol when the domain XML has a graphics device — honor it so
  // a graphical VM doesn't default to serial just because it isn't running.
  if (plan.recommended && isDisplayProtocol(plan.recommended)) return 'display'
  if ((plan.protocols ?? []).includes('serial')) return 'serial'
  return 'display'
}

/**
 * Cockpit pattern: choose the initial active protocol from VM capabilities.
 * VNC → 'novnc', SPICE → 'spice', fallback serial → 'serial', else 'novnc'.
 */
export function getDefaultProtocol(plan: PlanSnapshot): string {
  if (plan.native.available || plan.native.console_type === 'vnc') return 'novnc'
  if (plan.webrtc_spice_available || plan.native.console_type === 'spice') return 'spice'
  // Shutoff graphical VM: no live port yet, but trust the backend's display recommendation
  // (e.g. novnc / spice / rdp) so we don't drop to serial. See getDefaultLens.
  if (plan.recommended && isDisplayProtocol(plan.recommended)) return plan.recommended
  if ((plan.protocols ?? []).includes('serial')) return 'serial'
  return 'novnc'
}

export function isDisplayProtocol(protocol: string): boolean {
  return (
    protocol === 'novnc'
    || protocol === 'spice'
    || protocol === 'webrtc_spice'
    || protocol.startsWith('rdp')
  )
}

export function cinemaHubPath(vmId: string, extra?: Record<string, string>): string {
  const params = new URLSearchParams(extra ?? {})
  if (!params.has('mode')) params.set('mode', 'cinema')
  const q = params.toString()
  return `/platform/vms/${vmId}/consolehub${q ? `?${q}` : ''}`
}

export function studioHubPath(vmId: string, extra?: Record<string, string>): string {
  const q = new URLSearchParams({ mode: 'studio', ...extra })
  return `/platform/vms/${vmId}/consolehub?${q}`
}

export function cinemaPopoutPath(vmId: string, extra?: Record<string, string>): string {
  return cinemaHubPath(vmId, { popout: '1', ...extra })
}

export function spectatorCinemaPath(vmId: string, sessionId: string, spectatorToken: string): string {
  const q = new URLSearchParams({ mode: 'cinema', session: sessionId, spectator: spectatorToken })
  return `/platform/vms/${vmId}/consolehub?${q}`
}
