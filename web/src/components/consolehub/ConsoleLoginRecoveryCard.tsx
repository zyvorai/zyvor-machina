// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link } from 'react-router'
import type { GuestAccessHints } from '../../utils/guestAccessHints'

type Props = {
  hints: GuestAccessHints
  vmId?: string
  onSwitchToShell?: () => void
  onExposeSsh?: () => void
  exposing?: boolean
}

export default function ConsoleLoginRecoveryCard({
  hints,
  vmId,
  onSwitchToShell,
  onExposeSsh,
  exposing = false,
}: Props) {
  if (hints.auth_mode !== 'ssh_key') return null

  return (
    <div className="shrink-0 rounded-lg border border-[var(--apple-hairline)] bg-[var(--accent-soft)] px-3 py-2 text-xs text-[var(--text-primary)]/95 space-y-2" data-testid="console-login-recovery">
      <p className="font-medium">This VM is SSH-key only</p>
      <p className="text-[var(--link)]/80">Serial may show a login prompt, but no password was set. Use Shell in ConsoleHub or expose SSH for your laptop.</p>
      <div className="flex flex-wrap gap-2">
        {onSwitchToShell ? (
          <button type="button" className="btn-secondary text-xs" onClick={onSwitchToShell}>
            Switch to Shell
          </button>
        ) : null}
        {hints.guest_ip_private && !hints.ssh_nat_host_port && onExposeSsh ? (
          <button type="button" className="btn-primary text-xs" disabled={exposing} onClick={onExposeSsh}>
            {exposing ? 'Exposing…' : 'Expose SSH'}
          </button>
        ) : null}
        {vmId ? (
          <Link to={`/platform/vms/${vmId}?tab=network`} className="btn-secondary text-xs inline-flex items-center">
            Network tab
          </Link>
        ) : null}
      </div>
    </div>
  )
}
