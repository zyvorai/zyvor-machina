// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react'

type ClipboardBridge = {
  pasteToGuest: (text: string) => void
}

type ClipboardCtx = {
  guestText: string
  canSync: boolean
  registerBridge: (bridge: ClipboardBridge | null) => void
  onGuestClipboard: (text: string) => void
  pasteLocalToGuest: (text?: string) => Promise<boolean>
  copyGuestToLocal: () => Promise<boolean>
}

const Ctx = createContext<ClipboardCtx | null>(null)

export function ConsoleClipboardProvider({ children }: { children: ReactNode }) {
  const bridgeRef = useRef<ClipboardBridge | null>(null)
  const [guestText, setGuestText] = useState('')
  const [canSync, setCanSync] = useState(false)

  const registerBridge = useCallback((bridge: ClipboardBridge | null) => {
    bridgeRef.current = bridge
    setCanSync(Boolean(bridge))
    if (!bridge) setGuestText('')
  }, [])

  const onGuestClipboard = useCallback((text: string) => {
    setGuestText(text)
  }, [])

  const pasteLocalToGuest = useCallback(async (text?: string) => {
    const bridge = bridgeRef.current
    if (!bridge) return false
    try {
      const payload = text ?? (await navigator.clipboard.readText())
      if (!payload.trim()) return false
      bridge.pasteToGuest(payload)
      return true
    } catch {
      return false
    }
  }, [])

  const copyGuestToLocal = useCallback(async () => {
    if (!guestText.trim()) return false
    try {
      await navigator.clipboard.writeText(guestText)
      return true
    } catch {
      return false
    }
  }, [guestText])

  const value = useMemo(
    () => ({
      guestText,
      canSync,
      registerBridge,
      onGuestClipboard,
      pasteLocalToGuest,
      copyGuestToLocal,
    }),
    [guestText, canSync, registerBridge, onGuestClipboard, pasteLocalToGuest, copyGuestToLocal],
  )

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>
}

export function useConsoleClipboardOptional() {
  return useContext(Ctx)
}
