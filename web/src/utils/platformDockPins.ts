// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { DOCK_PATHS_BY_TIER, loadPlatformDesktopTier, type PlatformDesktopTier } from './platformDesktopTier'

const DOCK_KEY = 'machina-platform-dock-pins'
const PLATFORM_DOCK_CHANGED_EVENT = 'machina-platform-dock-changed'

/** Default pinned apps for the Machina platform dock (v9s MacDock pattern). */
function defaultDockPathsForTier(tier: PlatformDesktopTier = loadPlatformDesktopTier()): string[] {
  return DOCK_PATHS_BY_TIER[tier]
}

function savePlatformDockPaths(paths: string[]) {
  try { localStorage.setItem(DOCK_KEY, JSON.stringify(paths)) } catch { /* storage unavailable or full */ }
  window.dispatchEvent(new CustomEvent(PLATFORM_DOCK_CHANGED_EVENT))
}

export function resetPlatformDockPaths(tier: PlatformDesktopTier = loadPlatformDesktopTier()) {
  savePlatformDockPaths(defaultDockPathsForTier(tier))
}
