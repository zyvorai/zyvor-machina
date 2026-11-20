// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'

/** Desktop shell state that more than one component needs: the Finder inspector and cinema mode. */
type PlatformMacDesktopContextValue = {
  inspectorVisible: boolean
  cinemaChromeHidden: boolean
  setCinemaChromeHidden: (v: boolean) => void
  toggleInspector: () => void
  setInspectorVisible: (v: boolean) => void
}

const PlatformMacDesktopContext = createContext<PlatformMacDesktopContextValue | null>(null)

/** Keys left behind by the removed sidebar / icon rail. */
const STALE_KEYS = ['machina-platform-sidebar-collapsed', 'machina-platform-sidebar-visible']

function clearStaleSidebarKeys() {
  try {
    for (const k of STALE_KEYS) localStorage.removeItem(k)
    for (let i = localStorage.length - 1; i >= 0; i--) {
      const k = localStorage.key(i)
      if (k && k.startsWith('machina-sidenav-')) localStorage.removeItem(k)
    }
  } catch {
    /* private mode */
  }
}

export function PlatformMacDesktopProvider({ children }: { children: ReactNode }) {
  const [inspectorVisible, setInspectorVisible] = useState(true)
  const [cinemaChromeHidden, setCinemaChromeHidden] = useState(false)

  useEffect(() => { clearStaleSidebarKeys() }, [])

  const toggleInspector = useCallback(() => setInspectorVisible((v) => !v), [])

  const value = useMemo(
    () => ({ inspectorVisible, cinemaChromeHidden, setCinemaChromeHidden, toggleInspector, setInspectorVisible }),
    [inspectorVisible, cinemaChromeHidden, toggleInspector],
  )

  return <PlatformMacDesktopContext.Provider value={value}>{children}</PlatformMacDesktopContext.Provider>
}

export function usePlatformMacDesktop() {
  const ctx = useContext(PlatformMacDesktopContext)
  if (!ctx) throw new Error('usePlatformMacDesktop must be used within PlatformMacDesktopProvider')
  return ctx
}
