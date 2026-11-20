// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { AlertTriangle, Download, X } from 'lucide-react'
import type { VmGuestHealthReport } from '../../api/platform'
import { guestToolsStripVisible } from '../../utils/guestAgentUx'
import { statusSurfaceClasses, statusToneClass } from '../../utils/semanticColors'

const DISMISS_KEY = 'machina_guest_tools_strip_dismissed'

export default function GuestToolsStrip({
  guestHealth,
  guestToolsStatus,
  guestIp,
  guestHostname,
  vmId,
  onInstall,
  onOpenGuestHealth,
  installing,
  compact,
}: {
  guestHealth?: VmGuestHealthReport | null
  guestToolsStatus?: string | null
  guestIp?: string
  guestHostname?: string
  vmId?: string
  onInstall?: () => void
  onOpenGuestHealth?: () => void
  installing?: boolean
  compact?: boolean
}) {
  const dismissId = vmId ? `${DISMISS_KEY}:${vmId}` : DISMISS_KEY
  const [dismissed, setDismissed] = useState(() => {
    try {
      return localStorage.getItem(dismissId) === '1'
    } catch {
      return false
    }
  })

  if (!guestToolsStripVisible(guestHealth, guestToolsStatus) || dismissed) return null

  const dismiss = () => {
    setDismissed(true)
    try {
      localStorage.setItem(dismissId, '1')
    } catch { /* private mode */ }
  }

  const detail =
    guestHealth?.install_state === 'channel_only'
      ? 'Virtio channel is attached — start guestkit-agent inside the guest (QGA-compatible). Attaching the channel does not install the binary.'
      : 'Guest agent is not fully active — attach the virtio channel on the hypervisor, then install/start guestkit-agent inside the VM.'

  if (compact) {
    return (
      <div className={`rounded-lg border px-3 py-2 flex flex-wrap items-center justify-between gap-2 text-xs ${statusSurfaceClasses('warn')}`}>
        <span className={`flex items-center gap-2 ${statusToneClass('warn')}`}>
          <AlertTriangle className="w-3.5 h-3.5 shrink-0" />
          {detail}
        </span>
        <div className="flex items-center gap-2">
          {onOpenGuestHealth && (
            <button type="button" className="btn-secondary text-xs py-1 px-2" onClick={onOpenGuestHealth}>
              Guest health
            </button>
          )}
          {onInstall && (
            <button type="button" className="btn-secondary text-xs py-1 px-2" disabled={installing} onClick={onInstall}>
              {installing ? 'Installing…' : 'Attach virtio channel'}
            </button>
          )}
          <button type="button" className="p-1 text-[var(--text-muted)] hover:text-[var(--text-secondary)]" onClick={dismiss} aria-label="Dismiss">
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>
    )
  }

  return (
    <div className={`rounded-xl border p-4 flex flex-wrap items-center justify-between gap-3 ${statusSurfaceClasses('warn')}`}>
      <div className="flex items-center gap-3 min-w-0">
        <AlertTriangle className={`w-5 h-5 shrink-0 ${statusToneClass('warn')}`} />
        <div className="min-w-0">
          <p className="font-medium text-sm text-[var(--text-primary)]">Guest agent setup</p>
          <p className="text-xs text-[var(--text-muted)] mt-0.5">{detail}</p>
          {(guestIp || guestHostname) && (
            <p className="text-xs text-[var(--text-muted)] mt-1">
              {guestHostname && <span>{guestHostname} · </span>}
              {guestIp && <span>{guestIp}</span>}
            </p>
          )}
        </div>
      </div>
      <div className="flex items-center gap-2 shrink-0">
        {onOpenGuestHealth && (
          <button type="button" className="btn-secondary text-xs" onClick={onOpenGuestHealth}>
            Open Guest health
          </button>
        )}
        {onInstall && (
          <button type="button" className="btn-secondary text-xs flex items-center gap-1" disabled={installing} onClick={onInstall}>
            <Download className="w-3 h-3" /> {installing ? 'Queuing…' : 'Attach virtio channel'}
          </button>
        )}
        <button type="button" className="btn-secondary text-xs" onClick={dismiss}>Dismiss</button>
      </div>
    </div>
  )
}
