// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { createContext, useContext, ReactNode } from 'react'
import { useToast, type Toast, type ToastAction } from '../hooks/useToast'
import { ToastContainer } from '../components/Toast'

interface ToastContextType {
  success: (message: string, duration?: number, action?: ToastAction) => string
  error: (message: string, duration?: number, action?: ToastAction) => string
  warning: (message: string, duration?: number, action?: ToastAction) => string
  info: (message: string, duration?: number, action?: ToastAction) => string
}

const ToastContext = createContext<ToastContextType | undefined>(undefined)

type ToastState = {
  toasts: Toast[]
  removeToast: (id: string) => void
  clearAll: () => void
}

const ToastStateContext = createContext<ToastState | undefined>(undefined)

export function ToastProvider({ children }: { children: ReactNode }) {
  const { toasts, removeToast, clearAll, success, error, warning, info } = useToast()

  return (
    <ToastStateContext.Provider value={{ toasts, removeToast, clearAll }}>
      <ToastContext.Provider value={{ success, error, warning, info }}>
        {children}
      </ToastContext.Provider>
    </ToastStateContext.Provider>
  )
}

/** Mount inside `<BrowserRouter>` so toast action links can use react-router `<Link>`. */
export function ToastRenderer() {
  const state = useContext(ToastStateContext)
  if (!state) return null
  return (
    <ToastContainer
      toasts={state.toasts}
      onClose={state.removeToast}
      onClearAll={state.clearAll}
    />
  )
}

export function useToastContext() {
  const context = useContext(ToastContext)
  if (!context) throw new Error('useToastContext must be used within ToastProvider')
  return context
}
