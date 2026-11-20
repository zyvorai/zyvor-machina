// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { getPlatformInfo, type PlatformInfo } from '../api/system'
import { syncControllerProxyFromPlatformInfo } from '../api/platform'
import { getAuthProviders, type AuthProviders } from '../api/auth'
import { useEventStream, type MachinaEvent } from '../hooks/useEventStream'

/**
 * Snapshot of "what is this daemon doing right now?" used by the global
 * `<Hero/>` to show capability badges (TLS, OIDC, KubeVirt cluster exec) and
 * by every page to react to broadcast events without polling.
 */
interface PlatformInfoContextValue {
  info: PlatformInfo | null
  providers: AuthProviders | null
  loading: boolean
  /** Bumps every time the daemon emits an event on `/api/v1/events/stream`. */
  refreshKey: number
  /** Live state of the SSE channel (false during reconnect). */
  liveConnected: boolean
  lastEvent: MachinaEvent | null
}

const DEFAULT: PlatformInfoContextValue = {
  info: null,
  providers: null,
  loading: true,
  refreshKey: 0,
  liveConnected: false,
  lastEvent: null,
}

const PlatformInfoContext = createContext<PlatformInfoContextValue>(DEFAULT)

/** Slow-changing subset only — {@link usePlatformInfoSlow} doesn't re-render on every SSE tick. */
type PlatformInfoSlow = Pick<PlatformInfoContextValue, 'info' | 'providers' | 'loading'>
const SLOW_DEFAULT: PlatformInfoSlow = { info: DEFAULT.info, providers: DEFAULT.providers, loading: DEFAULT.loading }
const PlatformInfoSlowContext = createContext<PlatformInfoSlow>(SLOW_DEFAULT)

export function PlatformInfoProvider({ children }: { children: ReactNode }) {
  const [info, setInfo] = useState<PlatformInfo | null>(null)
  const [providers, setProviders] = useState<AuthProviders | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    Promise.allSettled([getPlatformInfo(), getAuthProviders()]).then(([pi, ap]) => {
      if (cancelled) return
      const platformInfo = pi.status === 'fulfilled' ? pi.value : null
      if (platformInfo) syncControllerProxyFromPlatformInfo(platformInfo)
      setInfo(platformInfo)
      setProviders(ap.status === 'fulfilled' ? ap.value : null)
      setLoading(false)
    })
    return () => { cancelled = true }
  }, [])

  const { connected: liveConnected, lastEvent, refreshKey } = useEventStream()

  const slowValue = useMemo<PlatformInfoSlow>(
    () => ({ info, providers, loading }),
    [info, providers, loading],
  )

  const value = useMemo<PlatformInfoContextValue>(
    () => ({ info, providers, loading, refreshKey, liveConnected, lastEvent }),
    [info, providers, loading, refreshKey, liveConnected, lastEvent],
  )

  return (
    <PlatformInfoSlowContext.Provider value={slowValue}>
      <PlatformInfoContext.Provider value={value}>{children}</PlatformInfoContext.Provider>
    </PlatformInfoSlowContext.Provider>
  )
}

export function usePlatformInfo(): PlatformInfoContextValue {
  return useContext(PlatformInfoContext)
}

/**
 * Like {@link usePlatformInfo} but only re-renders on `info`/`providers`/`loading`
 * changes — use this in components that don't need live SSE-driven fields
 * (`refreshKey`/`liveConnected`/`lastEvent`), especially ones with in-flight
 * enter/exit animations that a high-frequency re-render can interrupt.
 */
export function usePlatformInfoSlow(): PlatformInfoSlow {
  return useContext(PlatformInfoSlowContext)
}
