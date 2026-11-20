// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import {
  ExternalLink,
  Cpu,
  HardDrive,
  Monitor,
  MoreHorizontal,
  Pause,
  Play,
  Power,
  RotateCcw,
  Square,
  Terminal,
  Trash2,
} from 'lucide-react'
import type { VmGuestHealthReport } from '../../api/platform'
import { cinemaHubPath, studioHubPath } from '../../utils/consoleExperienceMode'
import { safeHref } from '../../utils/safeHref'
import SpotlightPageAction from './SpotlightPageAction'

type ActFn = (msg: string, fn: () => Promise<unknown>) => void

type PowerHandlers = {
  onInstall?: () => void
  onStart?: () => void
  onResume?: () => void
  onShutdown?: () => void
  onGracefulShutdown?: () => void
  onPause?: () => void
  onReboot?: () => void
  onGracefulReboot?: () => void
  onForceReboot?: () => void
  onNmi?: () => void
  onForceStop?: () => void
}

type Props = {
  vmId: string
  vmName: string
  inventorySource?: string | null
  observedState: string
  canInstall: boolean
  guestHealth?: VmGuestHealthReport | null
  isPopout?: boolean
  virtViewerUrl?: string | null
  spotlightPrefill: string
  onSsh: () => void
  onDelete: () => void
  onOpenHardware?: () => void
  onPopout?: () => void
  act: ActFn
  power: PowerHandlers
}

export default function VmDetailActionBar({
  vmId,
  vmName,
  inventorySource,
  observedState,
  canInstall,
  guestHealth,
  isPopout,
  virtViewerUrl,
  spotlightPrefill,
  onSsh,
  onDelete,
  onOpenHardware,
  onPopout,
  act,
  power,
}: Props) {
  const [menuOpen, setMenuOpen] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!menuOpen) return
    const close = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setMenuOpen(false)
    }
    document.addEventListener('mousedown', close)
    return () => document.removeEventListener('mousedown', close)
  }, [menuOpen])

  const isKubevirt = inventorySource === 'kubevirt'
  const isMissing = observedState === 'missing'
  const isRunning = observedState === 'running'
  const isPaused = observedState === 'paused'
  const isStopped = observedState === 'stopped' || observedState === 'shut off' || observedState === 'shutoff'
  const agentOk = guestHealth?.install_state === 'running' && guestHealth.agent_ping

  const primaryPower = (() => {
    if (isKubevirt || isMissing) return null
    if (canInstall && power.onInstall) {
      return (
        <button type="button" className="btn-secondary text-sm" data-testid="vm-install-button" onClick={power.onInstall}>
          <HardDrive className="w-4 h-4" /> Install
        </button>
      )
    }
    if (isStopped && power.onStart) {
      return (
        <button type="button" className="btn-secondary text-sm" onClick={power.onStart}>
          <Play className="w-4 h-4" /> Start
        </button>
      )
    }
    if (isPaused && power.onResume) {
      return (
        <button type="button" className="btn-secondary text-sm" onClick={power.onResume}>
          <Play className="w-4 h-4" /> Resume
        </button>
      )
    }
    if (isRunning && (power.onGracefulShutdown || power.onShutdown)) {
      return (
        <button
          type="button"
          className="btn-secondary text-sm"
          title={agentOk ? 'Clean shutdown via guest agent' : 'ACPI shutdown'}
          onClick={agentOk ? power.onGracefulShutdown : power.onShutdown}
        >
          <Power className="w-4 h-4" /> Shutdown
        </button>
      )
    }
    return null
  })()

  const menuItem = (label: string, onClick: () => void, opts?: { destructive?: boolean; testId?: string }) => (
    <button
      key={label}
      type="button"
      data-testid={opts?.testId}
      className={
        opts?.destructive
          ? 'w-full text-left px-3 py-2 text-sm text-red-600 hover:bg-red-500/10'
          : 'w-full text-left px-3 py-2 text-sm text-[var(--text-primary)] hover:bg-white/5'
      }
      onClick={() => {
        setMenuOpen(false)
        onClick()
      }}
    >
      {label}
    </button>
  )

  return (
    <div className="flex flex-wrap items-center gap-2" data-testid="vm-detail-action-bar">
      <Link to={cinemaHubPath(vmId)} className="btn-primary text-sm inline-flex items-center gap-1 vm-cinema-cta">
        <Monitor className="w-4 h-4" /> Open Cinema
      </Link>
      {onOpenHardware ? (
        <button type="button" className="btn-secondary text-sm inline-flex items-center gap-1" onClick={onOpenHardware} data-testid="vm-detail-hardware">
          <Cpu className="w-4 h-4" /> Hardware
        </button>
      ) : null}
      {!isKubevirt && (
        <button type="button" className="btn-secondary text-sm inline-flex items-center gap-1" onClick={onSsh}>
          <Terminal className="w-4 h-4" /> SSH
        </button>
      )}
      {primaryPower}

      {!isKubevirt && !isMissing && (
        <div className="relative" ref={menuRef}>
          <button
            type="button"
            className="btn-secondary text-sm inline-flex items-center gap-1"
            aria-expanded={menuOpen}
            aria-haspopup="menu"
            onClick={() => setMenuOpen((o) => !o)}
          >
            <MoreHorizontal className="w-4 h-4" /> Power &amp; more
          </button>
          {menuOpen ? (
            <div
              className="absolute right-0 top-full z-50 mt-1 min-w-[12rem] rounded-lg border border-white/10 bg-[var(--apple-surface-elevated)] shadow-xl py-1 vm-detail-action-menu"
              role="menu"
            >
              {isRunning && power.onPause ? menuItem('Pause', power.onPause) : null}
              {isRunning && agentOk && power.onGracefulReboot
                ? menuItem('Graceful reboot', power.onGracefulReboot)
                : isRunning && power.onReboot
                  ? menuItem('Reboot', power.onReboot)
                  : null}
              {isRunning && power.onForceReboot
                ? menuItem('Force reboot', power.onForceReboot, { testId: 'vm-force-reboot-button' })
                : null}
              {isRunning && power.onNmi ? menuItem('Inject NMI', power.onNmi, { testId: 'vm-nmi-button' }) : null}
              {(isRunning || isPaused) && power.onForceStop ? menuItem('Force stop', power.onForceStop) : null}
              <div className="my-1 border-t border-white/10" />
              <Link
                to={studioHubPath(vmId)}
                className="block px-3 py-2 text-sm text-[var(--text-primary)] hover:bg-white/5"
                onClick={() => setMenuOpen(false)}
              >
                Studio
              </Link>
              {virtViewerUrl ? (
                <a
                  href={safeHref(virtViewerUrl)}
                  download={`${vmName}.vv`}
                  className="block px-3 py-2 text-sm text-[var(--text-primary)] hover:bg-white/5"
                  data-testid="vm-virt-viewer-download"
                  onClick={() => setMenuOpen(false)}
                >
                  Virt-Viewer file
                </a>
              ) : null}
              {!isPopout && onPopout ? menuItem('Pop out', onPopout) : null}
              <div className="my-1 border-t border-white/10" />
              {menuItem('Delete VM', onDelete, { destructive: true })}
            </div>
          ) : null}
        </div>
      )}

      {!isPopout ? <SpotlightPageAction prefill={spotlightPrefill} label="Ask Zyra" /> : null}
    </div>
  )
}
