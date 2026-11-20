// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, useState } from 'react'
import { ChevronDown, ChevronUp, Copy, Monitor, Terminal, AlertTriangle, GripVertical } from 'lucide-react'
import { Link } from 'react-router'
import { formatUserError } from '../../utils/apiError'
import {
  aggregateAccessNoteLabels,
  aggregateAccessNoteMessages,
  isWindowsGuest,
  type GuestAccessHints,
} from '../../utils/guestAccessHints'
import { downloadRdpFile } from '../../api/rdp'
import {
  exposeGuestPortOnVm,
  laptopSshCommand,
  natRuleForGuestPort,
  type NatRuleLike,
} from '../../utils/vmPortForwardServices'

type Props = {
  hints: GuestAccessHints | null | undefined
  vmId?: string
  vmName?: string
  sshUser?: string
  guestIp?: string
  hypervisorHost?: string
  /** `os_hint` from the console plan — switches the note between SSH and RDP guidance. */
  osHint?: string
  portForwardRules?: NatRuleLike[]
  onPlanRefresh?: () => void
  onNotify?: (message: string) => void
  onOpenShell?: () => void
  onExplain?: () => void
  className?: string
}

export default function AccessNotePill({
  hints,
  vmId,
  vmName,
  sshUser,
  guestIp,
  hypervisorHost,
  osHint,
  portForwardRules = [],
  onPlanRefresh,
  onNotify,
  onOpenShell,
  onExplain,
  className = '',
}: Props) {
  const [open, setOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const [pos, setPos] = useState<{ x: number; y: number } | null>(() => {
    if (typeof window === 'undefined') return null
    try {
      const raw = window.localStorage.getItem('machina.accessNotePos')
      return raw ? (JSON.parse(raw) as { x: number; y: number }) : null
    } catch {
      return null
    }
  })
  const dragRef = useRef<{ startX: number; startY: number; baseX: number; baseY: number } | null>(null)

  useEffect(() => {
    if (typeof window === 'undefined') return
    try {
      if (pos) window.localStorage.setItem('machina.accessNotePos', JSON.stringify(pos))
      else window.localStorage.removeItem('machina.accessNotePos')
    } catch {
      /* ignore */
    }
  }, [pos])

  const onDragStart = (e: React.PointerEvent) => {
    e.preventDefault()
    e.stopPropagation()
    const base = pos ?? { x: 0, y: 0 }
    dragRef.current = { startX: e.clientX, startY: e.clientY, baseX: base.x, baseY: base.y }
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  }
  const onDragMove = (e: React.PointerEvent) => {
    const d = dragRef.current
    if (!d) return
    setPos({ x: d.baseX + (e.clientX - d.startX), y: d.baseY + (e.clientY - d.startY) })
  }
  const onDragEnd = (e: React.PointerEvent) => {
    dragRef.current = null
    try {
      ;(e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId)
    } catch {
      /* ignore */
    }
  }
  const host = hypervisorHost || (typeof window !== 'undefined' ? window.location.hostname : undefined)
  const windows = isWindowsGuest(osHint)
  // A rule already forwarding a host port to guest 3389, if the operator made one.
  const rdpNatHostPort = natRuleForGuestPort(portForwardRules, 3389)?.host_port ?? null
  const hintOpts = {
    sshUser,
    guestIp,
    hypervisorHost: host,
    osFamily: osHint,
    rdpNatHostPort,
    vmNetworkHref: vmId ? `/platform/vms/${vmId}?tab=network` : undefined,
  }
  const labels = aggregateAccessNoteLabels(hints, hintOpts)
  const messages = aggregateAccessNoteMessages(hints, hintOpts)

  if (labels.length === 0 && messages.length === 0) return null

  const notify = (msg: string) => onNotify?.(msg)
  const sshCmd = laptopSshCommand(sshUser || 'ubuntu', guestIp || '', host, portForwardRules)
  // Windows: show the address a native RDP client dials, not an ssh line.
  const rdpAddress = rdpNatHostPort ? `${host ?? 'HYPERVISOR_IP'}:${rdpNatHostPort}` : ''
  const connectCmd = windows ? rdpAddress : sshCmd

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

  const exposeRdp = async () => {
    if (!vmId || !vmName) return
    setBusy(true)
    try {
      // Label only: buildExposePayload already prefixes the VM name, so passing
      // `${vmName}-rdp` here doubled it in the firewall rule comment.
      await exposeGuestPortOnVm(vmId, vmName, 3389, portForwardRules, 'RDP')
      notify('RDP exposed on hypervisor — connect with Microsoft Remote Desktop or mstsc')
      onPlanRefresh?.()
    } catch (e: unknown) {
      notify(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const downloadRdp = () => {
    if (!vmName || !rdpNatHostPort || !host) {
      notify('Expose RDP first, then download the .rdp file')
      return
    }
    downloadRdpFile(host, rdpNatHostPort, vmName)
    notify(`${vmName}.rdp downloaded — open it with your RDP client`)
  }

  const copySsh = async () => {
    if (!sshCmd) return
    try {
      await navigator.clipboard.writeText(sshCmd)
      notify('SSH command copied')
    } catch {
      notify('Copy failed')
    }
  }

  return (
    <div
      className={`absolute top-14 left-1/2 -translate-x-1/2 z-40 max-w-xl w-[min(92vw,36rem)] ${className}`}
      style={pos ? { transform: `translate(calc(-50% + ${pos.x}px), ${pos.y}px)` } : undefined}
      data-testid="access-note-pill"
    >
      <div className="w-full flex items-center rounded-full border border-amber-500/35 bg-black/70 backdrop-blur-md text-xs text-amber-100/95 shadow-lg">
        <span
          role="button"
          tabIndex={0}
          aria-label="Move access note"
          title="Drag to move · double-click to reset"
          className="pl-2 pr-1 py-1.5 shrink-0 cursor-grab active:cursor-grabbing text-amber-300/60 hover:text-amber-200 touch-none select-none"
          onPointerDown={onDragStart}
          onPointerMove={onDragMove}
          onPointerUp={onDragEnd}
          onDoubleClick={() => setPos(null)}
        >
          <GripVertical className="w-3.5 h-3.5" />
        </span>
        <button
          type="button"
          className="flex-1 min-w-0 flex items-center gap-2 pr-3 py-1.5 rounded-r-full hover:bg-white/[0.06] transition"
          onClick={() => setOpen((v) => !v)}
        >
          <AlertTriangle className="w-3.5 h-3.5 shrink-0 text-amber-400" />
          <span className="truncate text-left flex-1">
            Access note: {labels.join(' · ') || 'Review guest access'}
          </span>
          {open ? <ChevronUp className="w-3.5 h-3.5 shrink-0" /> : <ChevronDown className="w-3.5 h-3.5 shrink-0" />}
        </button>
      </div>
      {open ? (
        <div className="mt-2 rounded-xl border border-amber-500/25 bg-slate-950/95 backdrop-blur-md p-3 text-xs text-amber-100/90 space-y-2 shadow-xl">
          {messages.map((msg) => (
            <p key={msg}>{msg}</p>
          ))}
          {connectCmd ? (
            <div className="space-y-1">
              {windows ? (
                <p className="text-[11px] uppercase tracking-wide text-amber-200/70">
                  Connect with Microsoft Remote Desktop (macOS) or mstsc (Windows)
                </p>
              ) : null}
              <pre className="rounded-lg bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] p-2 font-mono text-sm text-emerald-700 overflow-x-auto select-all">
                {connectCmd}
              </pre>
            </div>
          ) : null}
          <div className="flex flex-wrap gap-2 pt-1">
            {connectCmd ? (
              <button type="button" className="btn-secondary text-xs inline-flex items-center gap-1" onClick={() => void copySsh()}>
                <Copy className="w-3 h-3" /> {windows ? 'Copy address' : 'Copy command'}
              </button>
            ) : null}
            {windows ? (
              <>
                {hints?.guest_ip_private && !rdpNatHostPort && vmId && vmName ? (
                  <button type="button" className="btn-primary text-xs" disabled={busy} onClick={() => void exposeRdp()}>
                    {busy ? 'Exposing…' : 'Expose RDP'}
                  </button>
                ) : null}
                {rdpNatHostPort ? (
                  <button type="button" className="btn-secondary text-xs inline-flex items-center gap-1" onClick={downloadRdp}>
                    <Monitor className="w-3 h-3" /> Download .rdp
                  </button>
                ) : null}
              </>
            ) : (
              <>
                {hints?.guest_ip_private && !hints.ssh_nat_host_port && vmId && vmName ? (
                  <button type="button" className="btn-primary text-xs" disabled={busy} onClick={() => void exposeSsh()}>
                    {busy ? 'Exposing…' : 'Expose SSH'}
                  </button>
                ) : null}
                {onOpenShell ? (
                  <button type="button" className="btn-secondary text-xs inline-flex items-center gap-1" onClick={onOpenShell}>
                    <Terminal className="w-3 h-3" /> Open Shell
                  </button>
                ) : null}
              </>
            )}
            {onExplain ? (
              <button type="button" className="btn-secondary text-xs" onClick={onExplain}>
                Explain
              </button>
            ) : null}
            {vmId ? (
              <Link to={`/platform/vms/${vmId}?tab=network`} className="btn-secondary text-xs">
                Network
              </Link>
            ) : null}
          </div>
        </div>
      ) : null}
    </div>
  )
}
