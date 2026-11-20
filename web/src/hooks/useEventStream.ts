// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, useState } from 'react'

/** Single payload pushed by the daemon broadcast bus (`/api/v1/events/stream`). */
export interface MachinaEvent {
  kind: string
  target: string
  status: string
  message?: string
  timestamp_ms: number
}

interface UseEventStreamOptions {
  /** Reset connection / parser state when this changes (e.g. on auth). */
  resetKey?: unknown
  /** When `true`, keep the EventSource open. Default: `true`. */
  enabled?: boolean
}

/**
 * Subscribes to the daemon-side broadcast bus. Returns the latest event and a
 * `refreshKey` that bumps every time a new event arrives — passing it as a
 * React `key` (or to `useEffect` deps) is the easiest way to force pages
 * to re-fetch after a successful KubeVirt upload, VM start, etc.
 *
 * Resilient: auto-reconnects with backoff and survives transient daemon
 * restarts (e.g., during `systemctl restart machina-daemon`).
 */
export function useEventStream(opts: UseEventStreamOptions = {}) {
  const { resetKey, enabled = true } = opts
  const [connected, setConnected] = useState(false)
  const [lastEvent, setLastEvent] = useState<MachinaEvent | null>(null)
  const [refreshKey, setRefreshKey] = useState(0)
  const reconnectTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const closedRef = useRef(false)

  useEffect(() => {
    if (!enabled) return
    closedRef.current = false
    let attempt = 0
    let es: EventSource | null = null

    const connect = () => {
      if (closedRef.current) return
      try {
        es = new EventSource('/api/v1/events/stream', { withCredentials: true })
      } catch {
        scheduleReconnect()
        return
      }

      es.onopen = () => {
        attempt = 0
        setConnected(true)
      }
      es.onerror = () => {
        setConnected(false)
        es?.close()
        scheduleReconnect()
      }
      // Daemon sends events with `event: <kind>` and JSON payload as `data:`.
      const onMessage = (ev: MessageEvent) => {
        if (typeof ev.data !== 'string') return
        try {
          const parsed = JSON.parse(ev.data) as MachinaEvent
          if (parsed && typeof parsed.kind === 'string') {
            setLastEvent(parsed)
            setRefreshKey(k => k + 1)
          }
        } catch { /* ignore malformed event */ }
      }
      // Catch-all: any named event delivered by the bus.
      es.onmessage = onMessage
      // Also subscribe to known event names so they aren't silently dropped.
      for (const kind of ['kubevirt.qcow2.apply', 'kubevirt.qcow2.upload', 'kubevirt.qcow2.start']) {
        es.addEventListener(kind, onMessage as EventListener)
      }
    }

    const scheduleReconnect = () => {
      if (closedRef.current) return
      attempt = Math.min(attempt + 1, 6)
      const delay = Math.min(15000, 500 * 2 ** attempt) + Math.floor(Math.random() * 250)
      reconnectTimer.current = setTimeout(connect, delay)
    }

    connect()

    return () => {
      closedRef.current = true
      setConnected(false)
      if (reconnectTimer.current) clearTimeout(reconnectTimer.current)
      es?.close()
    }
  }, [enabled, resetKey])

  return { connected, lastEvent, refreshKey }
}
