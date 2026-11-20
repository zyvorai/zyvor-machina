// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import SSHConsole from '../SSHConsole'
import { MacGlassPanel } from './mac/PlatformMacUi'

type Props = {
  hostname: string
  address?: string | null
  online?: boolean
}

export default function PlatformHostTerminalPanel({ hostname, address, online = true }: Props) {
  const [sshUser, setSshUser] = useState('root')
  const target = (address?.trim() || hostname.trim())

  if (!target) {
    return (
      <p className="text-sm text-[var(--text-muted)]" data-testid="platform-host-terminal">
        No SSH target address is recorded for this host.
      </p>
    )
  }

  if (!online) {
    return (
      <MacGlassPanel title="Terminal" subtitle="Host must be online for browser SSH">
        <p className="text-sm text-amber-600/90" data-testid="platform-host-terminal">
          {hostname} is offline — terminal is unavailable until the agent reconnects.
        </p>
      </MacGlassPanel>
    )
  }

  return (
    <MacGlassPanel
      title="Terminal"
      subtitle={`Browser SSH to ${hostname} via machina daemon`}
    >
      <div className="space-y-3" data-testid="platform-host-terminal">
        <p className="text-xs text-[var(--text-muted)]">
          Opens a shell on <span className="font-mono text-[var(--text-secondary)]">{target}</span>. Requires SSH keys configured on the machina daemon host.
        </p>
        <label className="block text-xs text-[var(--text-muted)]">
          SSH user
          <input
            className="input w-full max-w-xs text-sm mt-1"
            value={sshUser}
            onChange={(e) => setSshUser(e.target.value)}
            placeholder="root"
            autoComplete="username"
          />
        </label>
        <SSHConsole host={target} sshUser={sshUser.trim() || 'root'} />
      </div>
    </MacGlassPanel>
  )
}
