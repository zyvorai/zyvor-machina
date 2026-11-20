// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import type { ActiveMonitor, ConsoleMonitor } from '../../utils/consoleMonitors'
import { inferConsoleMonitors } from '../../utils/consoleMonitors'

export type ViewportMode = 'fit' | 'fill' | 'native' | 'scroll' | 'zoom' | 'stretch'
export type ZoomLevel = 75 | 100 | 125 | 150 | 200
type ConsoleKeyPreset = 'esc' | 'ctrl_alt_del' | 'alt_tab'

type ViewportState = {
  mode: ViewportMode
  zoom: ZoomLevel
  scaledFit: boolean
  guestWidth: number
  guestHeight: number
  scrollLeft: number
  scrollTop: number
  viewportWidth: number
  viewportHeight: number
  connected: boolean
  protocol: string
  resolution: string
  monitors: ConsoleMonitor[]
  activeMonitor: ActiveMonitor
}

type ViewportCtx = ViewportState & {
  setMode: (mode: ViewportMode) => void
  setZoom: (zoom: ZoomLevel) => void
  setScaledFit: (v: boolean) => void
  setGuestSize: (w: number, h: number) => void
  setScroll: (left: number, top: number) => void
  setViewportSize: (w: number, h: number) => void
  setConnected: (v: boolean) => void
  setProtocol: (p: string) => void
  setResolution: (r: string) => void
  setMonitors: (monitors: ConsoleMonitor[]) => void
  setActiveMonitor: (monitor: ActiveMonitor) => void
  /** noVNC-native Ctrl+Alt+Del, registered by VNCViewer when connected. */
  sendCtrlAltDel: (() => void) | null
  registerCtrlAltDel: (fn: (() => void) | null) => void
  /**
   * Send a key preset natively over the connected VNC channel. Returns true if
   * it was handled (a VNC viewer is connected), false otherwise — so callers can
   * fall back to the qemu-guest-agent HTTP path.
   */
  sendKeyPreset: (preset: ConsoleKeyPreset) => boolean
  registerSendKeyPreset: (fn: ((preset: ConsoleKeyPreset) => boolean) | null) => void
}

const defaultState: ViewportState = {
  mode: 'fit',
  zoom: 100,
  scaledFit: true,
  guestWidth: 0,
  guestHeight: 0,
  scrollLeft: 0,
  scrollTop: 0,
  viewportWidth: 0,
  viewportHeight: 0,
  connected: false,
  protocol: 'novnc',
  resolution: '—',
  monitors: [],
  activeMonitor: 'all',
}

const Ctx = createContext<ViewportCtx | null>(null)

export function ConsoleViewportProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<ViewportState>(defaultState)
  const ctrlAltDelRef = useRef<(() => void) | null>(null)
  const registerCtrlAltDel = useCallback((fn: (() => void) | null) => {
    ctrlAltDelRef.current = fn
  }, [])

  const setMode = useCallback((mode: ViewportMode) => {
    setState((s) => {
      const scaledFit = mode === 'fit' || mode === 'fill'
      if (s.mode === mode && s.scaledFit === scaledFit) return s
      return { ...s, mode, scaledFit }
    })
  }, [])

  const setZoom = useCallback((zoom: ZoomLevel) => {
    setState((s) => {
      if (s.zoom === zoom && s.mode === 'zoom' && !s.scaledFit) return s
      return { ...s, zoom, mode: 'zoom', scaledFit: false }
    })
  }, [])

  const setScaledFit = useCallback((scaledFit: boolean) => {
    setState((s) => {
      const mode = scaledFit ? 'fit' : 'native'
      if (s.scaledFit === scaledFit && s.mode === mode) return s
      return { ...s, scaledFit, mode }
    })
  }, [])

  const setGuestSize = useCallback((guestWidth: number, guestHeight: number) => {
    setState((s) => {
      const resolution = guestWidth > 0 ? `${guestWidth}×${guestHeight}` : s.resolution
      if (s.guestWidth === guestWidth && s.guestHeight === guestHeight && s.resolution === resolution) return s
      return { ...s, guestWidth, guestHeight, resolution }
    })
  }, [])

  const setScroll = useCallback((scrollLeft: number, scrollTop: number) => {
    setState((s) => {
      if (s.scrollLeft === scrollLeft && s.scrollTop === scrollTop) return s
      return { ...s, scrollLeft, scrollTop }
    })
  }, [])

  const setViewportSize = useCallback((viewportWidth: number, viewportHeight: number) => {
    setState((s) => {
      if (s.viewportWidth === viewportWidth && s.viewportHeight === viewportHeight) return s
      return { ...s, viewportWidth, viewportHeight }
    })
  }, [])

  const setConnected = useCallback((connected: boolean) => {
    setState((s) => (s.connected === connected ? s : { ...s, connected }))
  }, [])

  const setProtocol = useCallback((protocol: string) => {
    setState((s) => (s.protocol === protocol ? s : { ...s, protocol }))
  }, [])

  const setResolution = useCallback((resolution: string) => {
    setState((s) => (s.resolution === resolution ? s : { ...s, resolution }))
  }, [])

  const setMonitors = useCallback((monitors: ConsoleMonitor[]) => {
    setState((s) => {
      const activeMonitor = monitors.length > 1 ? s.activeMonitor : 'all'
      if (
        s.monitors.length === monitors.length
        && s.monitors.every((mon, i) => mon.id === monitors[i]?.id && mon.x === monitors[i]?.x)
        && s.activeMonitor === activeMonitor
      ) {
        return s
      }
      return { ...s, monitors, activeMonitor }
    })
  }, [])

  const setActiveMonitor = useCallback((activeMonitor: ActiveMonitor) => {
    setState((s) => (s.activeMonitor === activeMonitor ? s : { ...s, activeMonitor }))
  }, [])

  useEffect(() => {
    const onTestGuestSize = (event: Event) => {
      const detail = (event as CustomEvent<{ width?: number; height?: number }>).detail
      const width = detail?.width ?? 0
      const height = detail?.height ?? 0
      if (width <= 0 || height <= 0) return
      setGuestSize(width, height)
      setMonitors(inferConsoleMonitors(width, height))
    }
    window.addEventListener('machina:console-guest-size', onTestGuestSize)
    return () => window.removeEventListener('machina:console-guest-size', onTestGuestSize)
  }, [setGuestSize, setMonitors])

  const sendCtrlAltDel = useCallback(() => ctrlAltDelRef.current?.(), [])

  const keyPresetRef = useRef<((preset: ConsoleKeyPreset) => boolean) | null>(null)
  const registerSendKeyPreset = useCallback((fn: ((preset: ConsoleKeyPreset) => boolean) | null) => {
    keyPresetRef.current = fn
  }, [])
  const sendKeyPreset = useCallback((preset: ConsoleKeyPreset) => keyPresetRef.current?.(preset) ?? false, [])

  const value = useMemo(
    () => ({
      ...state,
      setMode,
      setZoom,
      setScaledFit,
      setGuestSize,
      setScroll,
      setViewportSize,
      setConnected,
      setProtocol,
      setResolution,
      setMonitors,
      setActiveMonitor,
      sendCtrlAltDel,
      registerCtrlAltDel,
      sendKeyPreset,
      registerSendKeyPreset,
    }),
    [state, setMode, setZoom, setScaledFit, setGuestSize, setScroll, setViewportSize, setConnected, setProtocol, setResolution, setMonitors, setActiveMonitor, sendCtrlAltDel, registerCtrlAltDel, sendKeyPreset, registerSendKeyPreset],
  )

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>
}

export function useConsoleViewport() {
  const ctx = useContext(Ctx)
  if (!ctx) throw new Error('useConsoleViewport requires ConsoleViewportProvider')
  return ctx
}

export function useConsoleViewportOptional() {
  return useContext(Ctx)
}
