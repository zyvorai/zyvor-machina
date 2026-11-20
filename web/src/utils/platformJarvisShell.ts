// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Phase 57 — intent-first Jarvis shell (minimal menus on desktop landing).

import { loadPlatformDesktopTier, type PlatformDesktopTier } from './platformDesktopTier'

const JARVIS_SHELL_KEY = 'machina-jarvis-shell'
const JARVIS_SHELL_EVENT = 'machina-jarvis-shell-changed'
export const OPEN_SPOTLIGHT_EVENT = 'machina-open-spotlight'
export const CLOSE_PLATFORM_MENUS_EVENT = 'machina-close-platform-menus'
export const CLOSE_MISSION_CONTROL_EVENT = 'machina-close-mission-control'
export const DISMISS_PLATFORM_SHELL_EVENT = 'machina-dismiss-platform-shell'
export const SCROLL_GEOGRAPHY_EVENT = 'machina-scroll-geography'

/** Close transient platform chrome (mission control, menus, control center backdrop, dock editor). */
export function dismissPlatformShellOverlays() {
  window.dispatchEvent(new CustomEvent(CLOSE_MISSION_CONTROL_EVENT))
  window.dispatchEvent(new CustomEvent(CLOSE_PLATFORM_MENUS_EVENT))
  window.dispatchEvent(new CustomEvent(DISMISS_PLATFORM_SHELL_EVENT))
}

function defaultJarvisShellForTier(tier: PlatformDesktopTier): boolean {
  return tier === 'normal'
}

export function loadJarvisShell(tier?: PlatformDesktopTier): boolean {
  try {
    const raw = localStorage.getItem(JARVIS_SHELL_KEY)
    if (raw === '0') return false
    if (raw === '1') return true
  } catch {
    /* ignore */
  }
  return defaultJarvisShellForTier(tier ?? loadPlatformDesktopTier())
}

export function saveJarvisShell(enabled: boolean) {
  localStorage.setItem(JARVIS_SHELL_KEY, enabled ? '1' : '0')
  window.dispatchEvent(new CustomEvent(JARVIS_SHELL_EVENT, { detail: enabled }))
}

export function dispatchOpenSpotlight(prefill?: string) {
  window.dispatchEvent(new CustomEvent(CLOSE_MISSION_CONTROL_EVENT))
  window.dispatchEvent(new CustomEvent(CLOSE_PLATFORM_MENUS_EVENT))
  window.dispatchEvent(new CustomEvent(OPEN_SPOTLIGHT_EVENT, { detail: { prefill } }))
}

export function dispatchScrollGeography() {
  window.dispatchEvent(new CustomEvent(SCROLL_GEOGRAPHY_EVENT))
}
