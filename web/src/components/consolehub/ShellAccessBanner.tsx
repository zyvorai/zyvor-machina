// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Copy } from 'lucide-react'
import type { GuestAccessHints } from '../../utils/guestAccessHints'
import { laptopSshCommand, type NatRuleLike } from '../../utils/vmPortForwardServices'

type Props = {
  hints?: GuestAccessHints | null
  sshUser?: string
  guestIp?: string
  hypervisorHost?: string
  portForwardRules?: NatRuleLike[]
  onNotify?: (message: string) => void
}

export default function ShellAccessBanner({
  hints,
  sshUser = 'ubuntu',
  guestIp = '',
  hypervisorHost,
  portForwardRules = [],
  onNotify,
}: Props) {
  if (!hints) return null

  const laptopCmd = guestIp
    ? laptopSshCommand(sshUser, guestIp, hypervisorHost, portForwardRules)
    : ''

  const copyLaptop = async () => {
    if (!laptopCmd) return
    try {
      await navigator.clipboard.writeText(laptopCmd)
      onNotify?.('Laptop SSH command copied — use your cloud-init private key (-i)')
    } catch {
      onNotify?.('Copy failed')
    }
  }

  return (
    <div className="shrink-0 rounded-lg border border-[var(--accent)]/40 bg-[var(--accent-soft)] px-3 py-2 text-xs text-[var(--text-primary)]/90 space-y-1.5" data-testid="shell-access-banner">
      <p>
        In-browser Shell authenticates with <strong>hypervisor SSH keys</strong>, not your cloud-init key.
        {hints.auth_mode === 'ssh_key' ? ' This VM is SSH-key only.' : null}
      </p>
      {hints.guest_ip_private && (
        <p>
          Guest IP is cluster/NAT private — Shell dials the exposed port on the hypervisor when NAT is configured.
        </p>
      )}
      {laptopCmd ? (
        <div className="flex flex-wrap items-center gap-2 pt-0.5">
          <code className="font-mono text-emerald-700/90 break-all">{laptopCmd}</code>
          <button type="button" className="btn-secondary text-xs inline-flex items-center gap-1" onClick={() => void copyLaptop()}>
            <Copy className="w-3 h-3" /> Copy for laptop
          </button>
        </div>
      ) : null}
    </div>
  )
}
