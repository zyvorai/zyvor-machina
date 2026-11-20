// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { AlertTriangle, CheckCircle2, Circle, Loader2, Power, X, Info } from 'lucide-react'
import type { VmGuestHealthReport, VmPendingConfig } from '../../api/platform'
import { guestToolsStripVisible } from '../../utils/guestAgentUx'
import type { GuestAccessHints } from '../../utils/guestAccessHints'
import { statusPillClasses, statusSurfaceClasses, statusToneClass } from '../../utils/semanticColors'
import { sshNatHostPort, type NatRuleLike } from '../../utils/vmPortForwardServices'

const DISMISS_KEY = 'machina_guest_tools_strip_dismissed'

type Props = {
  vmId: string
  pending?: VmPendingConfig | null
  pendingLoading?: boolean
  guestHealth?: VmGuestHealthReport | null
  guestToolsStatus?: string | null
  observedState?: string
  guestIp?: string
  guestAccess?: GuestAccessHints | null
  portForwardRules?: NatRuleLike[]
  guestToolsInstalling?: boolean
  onShutdownForPending?: () => void
  onOpenGuestHealth?: () => void
  onInstallGuestTools?: () => void
  onOpenAccess?: () => void
}

type AttentionItem = {
  id: string
  priority: number
  label: string
  detail: string
  tone: 'pending' | 'guest' | 'laptop'
  expanded: React.ReactNode
  chipLabel: string
  dismissible?: boolean
}

function bannerClass(id: AttentionItem['tone']) {
  return `vm-attention-banner vm-attention-banner--${id}`
}

export default function VmAttentionStack({
  vmId,
  pending,
  pendingLoading,
  guestHealth,
  guestToolsStatus,
  observedState,
  guestIp,
  guestAccess,
  portForwardRules = [],
  guestToolsInstalling,
  onShutdownForPending,
  onOpenGuestHealth,
  onInstallGuestTools,
  onOpenAccess,
}: Props) {
  const dismissId = `${DISMISS_KEY}:${vmId}`
  const [guestDismissed, setGuestDismissed] = useState(() => {
    try {
      return localStorage.getItem(dismissId) === '1'
    } catch {
      return false
    }
  })
  const [activeId, setActiveId] = useState<string | null>(null)

  if (pendingLoading) return null

  const running = observedState === 'running'
  const ip = guestIp?.trim() ?? ''
  const sshExposed = Boolean(sshNatHostPort(portForwardRules))
  const privateNat = Boolean(guestAccess?.guest_ip_private)

  const items: AttentionItem[] = []

  if (pending?.needs_shutdown) {
    items.push({
      id: 'pending_config',
      priority: 1,
      tone: 'pending',
      label: 'Changes pending shutdown',
      chipLabel: 'Pending config',
      detail: 'Persistent configuration differs from the running guest. Shut down and start the VM to apply changes.',
      expanded: (
        <div className="space-y-3">
          <p className="text-sm text-[var(--text-primary)]">{pending.pending_changes.length > 0
            ? 'The following changes require a full shutdown:'
            : 'Shut down and start the VM to apply pending changes.'}</p>
          {pending.pending_changes.length > 0 && (
            <ul className="text-xs text-[var(--text-muted)] space-y-1 list-disc list-inside">
              {pending.pending_changes.slice(0, 6).map((c) => (
                <li key={`${c.category}-${c.summary}`}>
                  <span className="text-[var(--text-muted)] uppercase tracking-wide">{c.category}</span>
                  {' — '}
                  {c.summary}
                </li>
              ))}
              {pending.pending_changes.length > 6 && <li>+{pending.pending_changes.length - 6} more</li>}
            </ul>
          )}
          {onShutdownForPending && (
            <button type="button" className="btn-secondary text-sm" onClick={onShutdownForPending}>
              <Power className="w-4 h-4" /> Shut down to apply
            </button>
          )}
        </div>
      ),
    })
  }

  if (running && guestToolsStripVisible(guestHealth, guestToolsStatus) && !guestDismissed) {
    const detail =
      guestHealth?.install_state === 'channel_only'
        ? 'Virtio channel is attached — start guestkit-agent inside the guest (QGA-compatible).'
        : 'Guest agent is not fully active — attach the channel and install guestkit-agent inside the VM.'
    items.push({
      id: 'guest_agent',
      priority: 2,
      tone: 'guest',
      label: 'Guest agent setup',
      chipLabel: 'Guest agent',
      detail,
      dismissible: true,
      expanded: (
        <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <p className={`text-sm ${statusToneClass('warn')}`}>{detail}</p>
          <div className="flex flex-wrap gap-2 shrink-0">
            {onOpenGuestHealth && (
              <button type="button" className="btn-secondary text-xs" onClick={onOpenGuestHealth}>
                Open Guest health
              </button>
            )}
            {onInstallGuestTools && (
              <button type="button" className="btn-secondary text-xs" disabled={guestToolsInstalling} onClick={onInstallGuestTools}>
                {guestToolsInstalling ? <Loader2 className="w-3 h-3 animate-spin inline" /> : null}
                Attach virtio channel
              </button>
            )}
            <button
              type="button"
              className="p-1 text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
              aria-label="Dismiss guest agent reminder"
              onClick={() => {
                setGuestDismissed(true)
                try {
                  localStorage.setItem(dismissId, '1')
                } catch { /* private mode */ }
              }}
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
      ),
    })
  }

  if (running && privateNat && (!ip || !sshExposed)) {
    items.push({
      id: 'laptop_access',
      priority: 3,
      tone: 'laptop',
      label: 'Laptop access',
      chipLabel: !ip ? 'Guest IP' : 'Expose SSH',
      detail: !ip
        ? 'Guest IP not detected yet — install guest tools or wait for DHCP.'
        : 'Expose SSH on the hypervisor so you can connect from your laptop via NAT.',
      expanded: (
        <ul className="space-y-2 text-xs">
          <li className="flex items-center gap-2">
            {running ? <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" /> : <Circle className="w-3.5 h-3.5 text-[var(--text-muted)]" />}
            <span className="text-[var(--text-primary)]">VM running</span>
          </li>
          <li className="flex flex-wrap items-center gap-2">
            {ip ? <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" /> : <Circle className="w-3.5 h-3.5 text-[var(--text-muted)]" />}
            <span className={ip ? 'text-[var(--text-primary)]' : 'text-[var(--text-muted)]'}>{ip ? `Guest IP ${ip}` : 'Waiting for guest IP'}</span>
            {!ip && onInstallGuestTools && (
              <button type="button" className="btn-secondary text-xs" disabled={guestToolsInstalling} onClick={onInstallGuestTools}>
                Install guest tools
              </button>
            )}
          </li>
          <li className="flex flex-wrap items-center gap-2">
            {sshExposed ? <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" /> : <Circle className="w-3.5 h-3.5 text-[var(--text-muted)]" />}
            <span className={sshExposed ? 'text-[var(--text-primary)]' : 'text-[var(--text-muted)]'}>
              {sshExposed ? 'SSH exposed on hypervisor' : 'Expose SSH for laptop access'}
            </span>
            {!sshExposed && onOpenAccess && (
              <button type="button" className="btn-primary text-xs" onClick={onOpenAccess}>
                Open Access tab
              </button>
            )}
          </li>
        </ul>
      ),
    })
  }

  if (items.length === 0) return null

  items.sort((a, b) => a.priority - b.priority)
  const resolvedActiveId = activeId && items.some((i) => i.id === activeId) ? activeId : items[0]!.id
  const resolvedActive = items.find((i) => i.id === resolvedActiveId) ?? items[0]!
  const rest = items.filter((i) => i.id !== resolvedActive.id)

  return (
    <div className="space-y-2 animate-fade-in" data-testid="vm-attention-stack">
      <div
        className={`rounded-xl border p-4 ${statusSurfaceClasses(resolvedActive.tone === 'laptop' ? 'info' : 'warn')} ${bannerClass(resolvedActive.tone)}`}
        data-testid={resolvedActive.id === 'pending_config' ? 'vm-pending-config-banner' : undefined}
      >
        <div className="flex gap-3">
          {resolvedActive.tone === 'laptop'
            ? <Info className="w-5 h-5 text-[var(--machina-status-info)] shrink-0 mt-0.5" />
            : <AlertTriangle className="w-5 h-5 text-amber-400 shrink-0 mt-0.5" />}
          <div className="flex-1 min-w-0 space-y-2">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <p className="text-sm font-medium text-[var(--text-primary)]">{resolvedActive.label}</p>
              {items.length > 1 && (
                <span className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">
                  {items.length} issue{items.length === 1 ? '' : 's'}
                </span>
              )}
            </div>
            {resolvedActive.expanded}
          </div>
        </div>
      </div>
      {rest.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {rest.map((item) => (
            <button
              key={item.id}
              type="button"
              className={`${statusPillClasses('warn')} vm-attention-chip`}
              onClick={() => setActiveId(item.id)}
            >
              {item.chipLabel}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
