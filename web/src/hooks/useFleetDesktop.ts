// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useSyncExternalStore } from 'react'
import {
  getFleetDesktop,
  getFleetLinuxHealth,
  type FleetDesktopOverview,
  type FleetLinuxHealthOverview,
} from '../api/platform'

type FleetDesktopState = {
  desktop: FleetDesktopOverview | null
  linuxHealth: FleetLinuxHealthOverview | null
  loading: boolean
  error: string | null
  updatedAt: number
}

const listeners = new Set<() => void>()
const pollIntervals = new Map<symbol, number>()
const CACHE_MS = 5_000
let snapshot: FleetDesktopState = {
  desktop: null,
  linuxHealth: null,
  loading: false,
  error: null,
  updatedAt: 0,
}
let inFlight: Promise<void> | null = null
let pollTimer: ReturnType<typeof setInterval> | null = null

function emit(next: Partial<FleetDesktopState>) {
  snapshot = { ...snapshot, ...next }
  listeners.forEach((listener) => listener())
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

function getSnapshot() {
  return snapshot
}

async function refreshShared(force = false): Promise<void> {
  if (inFlight) return inFlight
  if (!force && snapshot.updatedAt > 0 && Date.now() - snapshot.updatedAt < CACHE_MS) return

  emit({ loading: true, error: null })
  inFlight = Promise.all([
    getFleetDesktop(),
    getFleetLinuxHealth().catch(() => null),
  ])
    .then(([desktop, linuxHealth]) => {
      emit({ desktop, linuxHealth, loading: false, updatedAt: Date.now() })
    })
    .catch((e: unknown) => {
      emit({
        loading: false,
        error: e instanceof Error ? e.message : 'Fleet desktop load failed',
      })
    })
    .finally(() => {
      inFlight = null
    })
  return inFlight
}

function reschedulePolling() {
  if (pollTimer) clearInterval(pollTimer)
  pollTimer = null
  const active = [...pollIntervals.values()].filter((ms) => ms > 0)
  if (active.length === 0) return
  pollTimer = setInterval(() => void refreshShared(true), Math.min(...active))
}

export function useFleetDesktop(enabled = true, intervalMs = 60_000) {
  const state = useSyncExternalStore(subscribe, getSnapshot, getSnapshot)
  const consumerId = useRef(Symbol('fleet-desktop-consumer'))

  const refresh = useCallback(async () => {
    if (enabled) await refreshShared(true)
  }, [enabled])

  useEffect(() => {
    if (!enabled) return undefined
    const id = consumerId.current
    pollIntervals.set(id, intervalMs)
    reschedulePolling()
    void refreshShared(false)
    return () => {
      pollIntervals.delete(id)
      reschedulePolling()
    }
  }, [enabled, intervalMs])

  return {
    desktop: state.desktop,
    linuxHealth: state.linuxHealth,
    loading: state.loading,
    error: state.error,
    refresh,
  }
}
