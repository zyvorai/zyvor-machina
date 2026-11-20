// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import {
  loadPlatformDesktopTier,
  PLATFORM_DESKTOP_TIER_EVENT,
  PLATFORM_DESKTOP_TIER_KEY,
  savePlatformDesktopTier,
  type PlatformDesktopTier,
} from '../utils/platformDesktopTier'
import { resetPlatformDockPaths } from '../utils/platformDockPins'

export function usePlatformDesktopTier(): [PlatformDesktopTier, (t: PlatformDesktopTier) => void] {
  const [tier, setTier] = useState<PlatformDesktopTier>(() => loadPlatformDesktopTier())
  useEffect(() => {
    const onChange = () => setTier(loadPlatformDesktopTier())
    window.addEventListener(PLATFORM_DESKTOP_TIER_EVENT, onChange)
    // The CustomEvent above only fires in the tab that made the change; the
    // native `storage` event covers other tabs so their nav can't disagree.
    const onStorage = (e: StorageEvent) => {
      if (e.key === PLATFORM_DESKTOP_TIER_KEY) setTier(loadPlatformDesktopTier())
    }
    window.addEventListener('storage', onStorage)
    return () => {
      window.removeEventListener(PLATFORM_DESKTOP_TIER_EVENT, onChange)
      window.removeEventListener('storage', onStorage)
    }
  }, [])
  const save = (next: PlatformDesktopTier) => {
    savePlatformDesktopTier(next)
    resetPlatformDockPaths(next)
    setTier(next)
  }
  return [tier, save]
}
