// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Copy } from 'lucide-react'
import { Link } from 'react-router'
import { formatUserError } from '../../utils/apiError'
import { consoleAccessHints, type GuestAccessHints } from '../../utils/guestAccessHints'
import { exposeGuestPortOnVm, laptopSshCommand, type NatRuleLike } from '../../utils/vmPortForwardServices'

type Props = {
  hints: GuestAccessHints | null | undefined
  lens: string
  sshUser?: string
  guestIp?: string
  vmId?: string
  vmName?: string
  hypervisorHost?: string
  portForwardRules?: NatRuleLike[]
  onPlanRefresh?: () => void
  onNotify?: (message: string) => void
}

export default function GuestAccessBanner({
  hints,
  lens,
  sshUser,
  guestIp,
  vmId,
  vmName,
  hypervisorHost,
  portForwardRules = [],
  onPlanRefresh,
  onNotify,
}: Props) {
  const [busy, setBusy] = useState(false)
  const host = hypervisorHost || (typeof window !== 'undefined' ? window.location.hostname : undefined)
  const messages = consoleAccessHints(hints, lens, {
    sshUser,
    guestIp,
    hypervisorHost: host,
    vmNetworkHref: vmId ? `/platform/vms/${vmId}?tab=network` : undefined,
  })

  const notify = (msg: string) => onNotify?.(msg)

  const exposeSsh = async () => {
    if (!vmId || !vmName) return
    setBusy(true)
    try {
      await exposeGuestPortOnVm(vmId, vmName, 22, portForwardRules)
      notify('SSH exposed on hypervisor')
      onPlanRefresh?.()
    } catch (e: unknown) {
      notify(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const copySsh = async () => {
    const cmd = laptopSshCommand(sshUser || 'ubuntu', guestIp || '', host, portForwardRules)
    if (!cmd) return
    try {
      await navigator.clipboard.writeText(cmd)
      notify('SSH command copied')
    } catch {
      notify('Copy failed')
    }
  }

  const showActions =
    hints &&
    (lens === 'serial' || lens === 'shell') &&
    hints.guest_ip_private

  if (messages.length === 0 && !showActions) return null

  return (
    <div className="shrink-0 space-y-2 rounded-lg border border-amber-500/30 bg-amber-950/25 px-3 py-2 text-xs text-amber-100/95" data-testid="guest-access-banner">
      {messages.map((msg) => (
        <p key={msg}>{msg}</p>
      ))}
      {showActions ? (
        <div className="flex flex-wrap gap-2 pt-1">
          {!hints.ssh_nat_host_port && vmId && vmName ? (
            <button type="button" className="btn-primary text-xs" disabled={busy} onClick={() => void exposeSsh()}>
              {busy ? 'Exposing…' : 'Expose SSH'}
            </button>
          ) : null}
          {hints.ssh_nat_host_port ? (
            <button type="button" className="btn-secondary text-xs inline-flex items-center gap-1" onClick={() => void copySsh()}>
              <Copy className="w-3 h-3" /> Copy SSH command
            </button>
          ) : null}
          {vmId ? (
            <Link to={`/platform/vms/${vmId}?tab=network`} className="btn-secondary text-xs inline-flex items-center">
              Network tab
            </Link>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}
