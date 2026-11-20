// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState, useCallback, useEffect, useRef } from 'react'

export type ToastAction = {
  label: string
  href: string
}

export interface Toast {
  id: string
  message: string
  type: 'success' | 'error' | 'warning' | 'info'
  action?: ToastAction
}

const MAX_VISIBLE_TOASTS = 8

/** Normalize kubectl stderr so duplicate TLS spam dedupes across retries. */
function errorToastDedupeKey(message: string): string {
  const stripped = message
    .replace(/E\d{4}\s+\d{2}:\d{2}:\d{2}\.\d+\s+\d+\s+/g, '')
    .replace(/\b\d{5,7}\b/g, '')
  return stripped.length > 900 ? stripped.slice(0, 900) : stripped
}

export function useToast() {
  const [toasts, setToasts] = useState<Toast[]>([])
  const timersRef = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map())
  const lastErrorToastRef = useRef<{ key: string; at: number; id: string } | null>(null)

  const cancelTimer = useCallback((id: string) => {
    const timer = timersRef.current.get(id)
    if (timer) {
      clearTimeout(timer)
      timersRef.current.delete(id)
    }
  }, [])

  useEffect(() => {
    const timers = timersRef.current
    return () => {
      timers.forEach(clearTimeout)
      timers.clear()
    }
  }, [])

  const removeToast = useCallback((id: string) => {
    cancelTimer(id)
    // Once the deduped error toast is actually gone, a repeat of the same
    // error must be able to show again immediately — otherwise a dismissed
    // (or auto-expired) error toast silently swallows the next occurrence
    // for the rest of the 8s dedupe window, even though nothing is on screen.
    if (lastErrorToastRef.current?.id === id) lastErrorToastRef.current = null
    setToasts((prev) => prev.filter((t) => t.id !== id))
  }, [cancelTimer])

  const clearAll = useCallback(() => {
    timersRef.current.forEach(clearTimeout)
    timersRef.current.clear()
    lastErrorToastRef.current = null
    setToasts([])
  }, [])

  const addToast = useCallback((
    message: string,
    type: Toast['type'],
    duration = 5000,
    action?: ToastAction,
  ) => {
    const id = crypto.randomUUID()
    setToasts((prev) => {
      const next = [...prev, { id, message, type, action }]
      if (next.length <= MAX_VISIBLE_TOASTS) return next
      const dropped = next.slice(0, next.length - MAX_VISIBLE_TOASTS)
      dropped.forEach((t) => {
        cancelTimer(t.id)
        // If the evicted toast was the one tracked for error-dedupe, forget it —
        // otherwise a repeat of the same error is silently suppressed for the
        // rest of the 8s window even though nothing is on screen anymore.
        if (lastErrorToastRef.current?.id === t.id) lastErrorToastRef.current = null
      })
      return next.slice(-MAX_VISIBLE_TOASTS)
    })
    const timer = setTimeout(() => {
      timersRef.current.delete(id)
      if (lastErrorToastRef.current?.id === id) lastErrorToastRef.current = null
      setToasts((prev) => prev.filter((t) => t.id !== id))
    }, duration)
    timersRef.current.set(id, timer)
    return id
  }, [cancelTimer])

  const error = useCallback(
    (msg: string, d?: number) => {
      const key = errorToastDedupeKey(msg)
      const now = Date.now()
      const prev = lastErrorToastRef.current
      if (prev && prev.key === key && now - prev.at < 8000) {
        return prev.id
      }
      const id = addToast(msg, 'error', d)
      lastErrorToastRef.current = { key, at: now, id }
      return id
    },
    [addToast],
  )

  const success = useCallback(
    (msg: string, d?: number, action?: ToastAction) => addToast(msg, 'success', d, action),
    [addToast],
  )
  const warning = useCallback(
    (msg: string, d?: number, action?: ToastAction) => addToast(msg, 'warning', d, action),
    [addToast],
  )
  const info = useCallback(
    (msg: string, d?: number, action?: ToastAction) => addToast(msg, 'info', d, action),
    [addToast],
  )

  return {
    toasts,
    removeToast,
    clearAll,
    success,
    error,
    warning,
    info,
  }
}
