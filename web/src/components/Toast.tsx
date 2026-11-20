// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link } from 'react-router'
import { motion } from 'framer-motion'
import { X, CheckCircle, AlertCircle, AlertTriangle, Info } from 'lucide-react'
import type { Toast } from '../hooks/useToast'
import { formatUserError } from '../utils/apiError'
import { summarizeK8sClientError } from '../utils/k8sErrors'
import { statusBorderClass, statusToneClass, toastSemanticTone } from '../utils/semanticColors'

const icons = {
  success: <CheckCircle className={`w-5 h-5 ${statusToneClass('ok')}`} />,
  error: <AlertCircle className={`w-5 h-5 ${statusToneClass('error')}`} />,
  warning: <AlertTriangle className={`w-5 h-5 ${statusToneClass('warn')}`} />,
  info: <Info className={`w-5 h-5 ${statusToneClass('info')}`} />,
}

function displayErrorMessage(raw: string): string {
  const sanitized = formatUserError(new Error(raw))
  const lower = sanitized.toLowerCase()
  const looksK8s =
    lower.includes('kubectl') ||
    lower.includes('kubeconfig') ||
    lower.includes('certificate signed by unknown authority') ||
    lower.includes('memcache.go')
  if (looksK8s && sanitized.length > 200) {
    return summarizeK8sClientError(sanitized).headline
  }
  if (sanitized.length > 320) {
    return `${sanitized.slice(0, 317)}…`
  }
  return sanitized
}

function ToastItem({
  toast,
  onClose,
}: {
  toast: Toast
  onClose: (id: string) => void
}) {
  const display =
    toast.type === 'error' ? displayErrorMessage(toast.message) : toast.message
  return (
    <motion.div
      key={toast.id}
      layout
      initial={{ opacity: 1, x: 12, scale: 0.98 }}
      animate={{ opacity: 1, x: 0, scale: 1 }}
      exit={{ opacity: 0, x: 24 }}
      transition={{ type: 'spring', stiffness: 380, damping: 28 }}
      className={`liquid-glass-toast glass-strong pointer-events-auto flex items-start gap-3 px-4 py-3 min-w-[300px] max-w-lg border shrink-0 ${statusBorderClass(toastSemanticTone(toast.type))}`}
    >
      <span className="shrink-0 mt-0.5">{icons[toast.type]}</span>
      <div className="flex-1 min-w-0">
        <span
          className="block text-sm text-[var(--text-primary)] whitespace-pre-wrap break-words max-h-32 overflow-y-auto"
          title={display.length > 220 ? display : undefined}
        >
          {display}
        </span>
        {toast.action && (
          <Link
            to={toast.action.href}
            data-testid="toast-action-link"
            className="inline-block mt-2 text-xs font-medium text-[var(--link)] hover:text-[var(--link)]"
            onClick={() => onClose(toast.id)}
          >
            {toast.action.label} →
          </Link>
        )}
      </div>
      <button
        type="button"
        onClick={() => onClose(toast.id)}
        className="shrink-0 rounded p-0.5 text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-white/10"
        aria-label="Dismiss notification"
      >
        <X className="w-4 h-4" strokeWidth={1.75} />
      </button>
    </motion.div>
  )
}

export function ToastContainer({
  toasts,
  onClose,
  onClearAll,
}: {
  toasts: Toast[]
  onClose: (id: string) => void
  onClearAll: () => void
}) {
  if (toasts.length === 0) return null

  const errorToasts = toasts.filter((t) => t.type === 'error')
  const otherToasts = toasts.filter((t) => t.type !== 'error')

  return (
    <div
      className="fixed top-4 right-4 z-[120] flex max-h-[min(80vh,calc(100vh-2rem))] max-w-[min(100vw-2rem,32rem)] flex-col gap-2 pointer-events-none"
      role="region"
      aria-label="Notifications"
    >
      {toasts.length > 1 && (
        <div className="flex justify-end pointer-events-auto">
          <button
            type="button"
            onClick={onClearAll}
            className="liquid-glass-toast glass-strong rounded-full border border-white/[0.08] px-3 py-1 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
          >
            Dismiss all ({toasts.length})
          </button>
        </div>
      )}
      {/* Assertive region: errors are announced immediately, interrupting current speech */}
      <div
        aria-live="assertive"
        aria-atomic="true"
        className="contents pointer-events-none"
      >
        {errorToasts.length > 0 && (
          <div className="flex flex-col gap-2 overflow-y-auto overscroll-contain pr-1">
            {errorToasts.map((toast) => (
              <ToastItem key={toast.id} toast={toast} onClose={onClose} />
            ))}
          </div>
        )}
      </div>
      {/* Polite region: success/info/warning announced when the user is idle */}
      <div
        aria-live="polite"
        aria-atomic="true"
        className="contents pointer-events-none"
      >
        {otherToasts.length > 0 && (
          <div className="flex flex-col gap-2 overflow-y-auto overscroll-contain pr-1">
            {otherToasts.map((toast) => (
              <ToastItem key={toast.id} toast={toast} onClose={onClose} />
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
