// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { CLOSE_MISSION_CONTROL_EVENT } from '../../../utils/platformJarvisShell'

export const OPEN_MISSION_CONTROL_EVENT = 'machina-open-mission-control'

function initialMissionControlOpen(): boolean {
  if (typeof window === 'undefined') return false
  try {
    if (new URLSearchParams(window.location.search).get('mission') === '1') return true
    return sessionStorage.getItem('machina-open-mission') === '1'
  } catch {
    return false
  }
}

type MissionControlContextValue = {
  open: boolean
  openMissionControl: () => void
  closeMissionControl: () => void
  toggleMissionControl: () => void
}

const MissionControlContext = createContext<MissionControlContextValue | null>(null)

export function MissionControlProvider({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(initialMissionControlOpen)

  const openMissionControl = useCallback(() => {
    try { sessionStorage.setItem('machina-open-mission', '1') } catch { /* ignore */ }
    setOpen(true)
  }, [])
  const closeMissionControl = useCallback(() => {
    try { sessionStorage.removeItem('machina-open-mission') } catch { /* ignore */ }
    setOpen(false)
  }, [])
  const toggleMissionControl = useCallback(() => {
    setOpen((v) => {
      const next = !v
      try {
        if (next) sessionStorage.setItem('machina-open-mission', '1')
        else sessionStorage.removeItem('machina-open-mission')
      } catch {
        /* ignore */
      }
      return next
    })
  }, [])

  useEffect(() => {
    const onClose = () => closeMissionControl()
    window.addEventListener(CLOSE_MISSION_CONTROL_EVENT, onClose)
    return () => window.removeEventListener(CLOSE_MISSION_CONTROL_EVENT, onClose)
  }, [closeMissionControl])

  const value = useMemo(
    () => ({ open, openMissionControl, closeMissionControl, toggleMissionControl }),
    [open, openMissionControl, closeMissionControl, toggleMissionControl],
  )

  return <MissionControlContext.Provider value={value}>{children}</MissionControlContext.Provider>
}

export function useMissionControl() {
  const ctx = useContext(MissionControlContext)
  if (!ctx) throw new Error('useMissionControl must be used within MissionControlProvider')
  return ctx
}

export function dispatchOpenMissionControl() {
  window.dispatchEvent(new CustomEvent(OPEN_MISSION_CONTROL_EVENT))
}
