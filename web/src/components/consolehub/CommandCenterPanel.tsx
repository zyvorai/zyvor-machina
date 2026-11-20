// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import { ShieldAlert, X } from 'lucide-react'
import { breakGlassConsoleSession, type ConsoleHubSessionResponse } from '../../api/platform'
import { getVmDoctor, type VmDoctorReport } from '../../api/ai'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import type { ConsoleHubSessionRow } from './ConsoleHubSessionHistory'
import type { VmTimelineEntry } from '../../api/platformVmTimeline'
import type { GuestAccessHints } from '../../utils/guestAccessHints'
import type { VmPortForwardRule } from '../../api/platform'
import MachineTimeline from './MachineTimeline'
import ConsoleHubSessionHistory from './ConsoleHubSessionHistory'
import ConsoleGuestFilePanel from './ConsoleGuestFilePanel'
import VmPortForwardPanel from '../vm/VmPortForwardPanel'
import MachinaDoctorPanel from '../platform/MachinaDoctorPanel'
import ConsoleCopilotLens from './ConsoleCopilotLens'
import { useFocusTrap } from '../../hooks/useFocusTrap'

export type CommandCenterTab = 'Overview' | 'Health' | 'Events' | 'AI'

type Props = {
  open: boolean
  onClose: () => void
  vmId: string
  vmName: string
  healthScore?: number | null
  vmState?: string | null
  guestIp?: string | null
  guestAccess?: GuestAccessHints | null
  hypervisorAddress?: string
  sshUser?: string
  sessions?: ConsoleHubSessionRow[]
  timeline?: VmTimelineEntry[]
  activeTab: CommandCenterTab
  onTabChange: (tab: CommandCenterTab) => void
  onAction?: (action: string) => void
  onPlanRefresh?: () => void
  onOpenVmDetail?: (tab?: string) => void
  activeProtocol?: string
  canBreakGlass?: boolean
  shareLink?: string | null
  onShareView?: () => void
  onOpenReplay?: (sessionId: string) => void | Promise<void>
  portForwardRules?: VmPortForwardRule[]
  readOnly?: boolean
  onExposeSsh?: () => void
  /** Called with the real session response once a break-glass session is
   * successfully created, so the owning page can lift it into `session`
   * state — without this, `recording_enabled` from the response never
   * reaches useConsoleAccessPolicy/useConsoleSessionRecorder and the
   * break-glass session is never actually recorded client-side. */
  onSessionStart?: (session: ConsoleHubSessionResponse) => void
}

const TABS: CommandCenterTab[] = ['Overview', 'Health', 'Events', 'AI']

export default function CommandCenterPanel({
  open,
  onClose,
  vmId,
  vmName,
  healthScore,
  vmState,
  guestIp,
  guestAccess,
  hypervisorAddress,
  sshUser,
  sessions = [],
  timeline = [],
  activeTab,
  onTabChange,
  onAction,
  onPlanRefresh,
  onOpenVmDetail,
  activeProtocol = 'novnc',
  canBreakGlass = false,
  shareLink = null,
  onShareView,
  onOpenReplay,
  portForwardRules = [],
  readOnly = false,
  onExposeSsh,
  onSessionStart,
}: Props) {
  const toast = useToastContext()
  const [doctor, setDoctor] = useState<VmDoctorReport | null>(null)
  const [doctorLoading, setDoctorLoading] = useState(false)
  const [breakGlassReason, setBreakGlassReason] = useState('')
  const [breakGlassBusy, setBreakGlassBusy] = useState(false)

  const ip = guestIp?.trim() ?? ''
  const panelRef = useRef<HTMLElement>(null)
  useFocusTrap(panelRef, open, onClose)

  useEffect(() => {
    if (!open || activeTab !== 'Health') return
    setDoctorLoading(true)
    void getVmDoctor(vmId)
      .then(setDoctor)
      .catch(() => setDoctor(null))
      .finally(() => setDoctorLoading(false))
  }, [open, activeTab, vmId])

  if (!open) return null

  return (
    <>
      <button type="button" className="fixed inset-0 z-[75] bg-black/40 backdrop-blur-sm" aria-label="Close Ops Shelf" onClick={onClose} />
      <aside ref={panelRef} className="fixed top-0 right-0 z-[80] h-full w-full max-w-md bg-slate-950/95 border-l border-white/10 shadow-2xl flex flex-col overflow-hidden animate-in slide-in-from-right duration-200" role="dialog" aria-modal="true" aria-label="Ops Shelf" data-testid="ops-shelf">
        <header className="flex items-center justify-between px-4 py-3 border-b border-white/10 shrink-0">
          <div>
            <h2 className="font-semibold text-[var(--text-primary)]">Ops Shelf</h2>
            <p className="text-xs text-[var(--text-muted)]">VM Intelligence · {vmName}</p>
          </div>
          <button type="button" onClick={onClose} aria-label="Close" className="p-1.5 rounded hover:bg-white/10 text-[var(--text-muted)]"><X className="w-5 h-5" aria-hidden="true" /></button>
        </header>
        <div className="flex gap-1 px-3 py-2 border-b border-white/5 shrink-0 overflow-x-auto">
          {TABS.map((t) => (
            <button
              key={t}
              type="button"
              onClick={() => onTabChange(t)}
              className={
                t === activeTab
                  ? 'px-2 py-1 rounded text-xs whitespace-nowrap bg-emerald-900/40 text-emerald-100'
                  : 'px-2 py-1 rounded text-xs whitespace-nowrap text-[var(--text-muted)] hover:text-[var(--text-secondary)]'
              }
            >
              {t}
            </button>
          ))}
        </div>
        <div className="flex-1 overflow-y-auto p-4 space-y-4 text-sm">
          {activeTab === 'Overview' && (
            <>
              <div className="grid grid-cols-2 gap-2 text-xs">
                <div className="rounded-lg bg-slate-900/60 p-2 border border-slate-800">
                  <p className="text-[var(--text-muted)]">Health</p>
                  <p className="text-lg font-semibold text-[var(--text-primary)]">{healthScore ?? '—'}</p>
                </div>
                <div className="rounded-lg bg-slate-900/60 p-2 border border-slate-800">
                  <p className="text-[var(--text-muted)]">State</p>
                  <p className="text-[var(--text-primary)] capitalize">{vmState ?? 'unknown'}</p>
                </div>
                <div className="rounded-lg bg-slate-900/60 p-2 border border-slate-800 col-span-2">
                  <p className="text-[var(--text-muted)]">Network</p>
                  <p className="font-mono text-emerald-600/90">{guestIp ?? 'No IP'}</p>
                </div>
              </div>
              {guestAccess?.guest_ip_private && ip ? (
                <div>
                  <p className="text-xs font-medium text-[var(--text-muted)] mb-2">Hypervisor NAT</p>
                  <VmPortForwardPanel
                    platformVmId={vmId}
                    vmName={vmName}
                    guestIp={ip}
                    sshUser={sshUser}
                    hypervisorAddress={hypervisorAddress}
                    compact
                    onNotify={() => onPlanRefresh?.()}
                  />
                </div>
              ) : null}
              <ConsoleHubSessionHistory sessions={sessions} onOpenReplay={onOpenReplay} />
              {!readOnly ? (
                <ConsoleGuestFilePanel
                  vmId={vmId}
                  vmName={vmName}
                  guestIp={guestIp}
                  guestAccess={guestAccess}
                  hypervisorAddress={hypervisorAddress}
                  sshUser={sshUser}
                  portForwardRules={portForwardRules}
                  readOnly={readOnly}
                  onExposeSsh={onExposeSsh}
                />
              ) : null}
              {onShareView ? (
                <div className="rounded-lg border border-[var(--accent)]/40 bg-[var(--accent-soft)] p-3 space-y-2" data-testid="ops-shelf-collaborate">
                  <p className="text-xs font-medium text-[var(--text-primary)]">Share read-only view</p>
                  <p className="text-[11px] text-[var(--link)]/70">Invite a teammate to watch this console in Cinema — no power or keyboard control.</p>
                  <button type="button" className="btn-secondary text-xs w-full" onClick={onShareView}>
                    Copy spectator link
                  </button>
                  {shareLink ? (
                    <p className="text-[10px] font-mono text-[var(--link)]/80 break-all">{shareLink}</p>
                  ) : null}
                </div>
              ) : null}
              {canBreakGlass ? (
                <div className="rounded-lg border border-amber-500/30 bg-amber-950/20 p-3 space-y-2" data-testid="ops-shelf-break-glass">
                  <p className="text-xs font-medium text-amber-100 flex items-center gap-1.5">
                    <ShieldAlert className="w-3.5 h-3.5" /> Break-glass console
                  </p>
                  <p className="text-[11px] text-amber-200/70">Starts a recorded, audited session when JIT approval is blocked.</p>
                  <input
                    type="text"
                    aria-label="Break-glass reason"
                    className="input w-full text-xs"
                    placeholder="Reason (required for audit)"
                    value={breakGlassReason}
                    onChange={(e) => setBreakGlassReason(e.target.value)}
                  />
                  <button
                    type="button"
                    className="btn-secondary text-xs w-full"
                    disabled={breakGlassBusy || breakGlassReason.trim().length < 4}
                    onClick={() => {
                      setBreakGlassBusy(true)
                      void breakGlassConsoleSession(vmId, { protocol: activeProtocol, reason: breakGlassReason.trim() })
                        .then((res) => {
                          toast.success('Break-glass session started — recording enabled')
                          setBreakGlassReason('')
                          onSessionStart?.(res)
                          onPlanRefresh?.()
                        })
                        .catch((e: unknown) => toast.error(formatUserError(e)))
                        .finally(() => setBreakGlassBusy(false))
                    }}
                  >
                    Start break-glass session
                  </button>
                </div>
              ) : null}
              <div>
                <p className="text-xs font-medium text-[var(--text-muted)] mb-2">Quick actions</p>
                <div className="flex flex-wrap gap-2">
                  {['Snapshot', 'Restart', 'Inspect Disk', 'eBPF Trace', 'Migrate'].map((a) => (
                    <button
                      key={a}
                      type="button"
                      className="btn-secondary text-xs py-1 px-2"
                      onClick={() => onAction?.(a)}
                    >
                      {a}
                    </button>
                  ))}
                </div>
              </div>
            </>
          )}

          {activeTab === 'Health' && (
            <MachinaDoctorPanel
              vmId={vmId}
              report={doctor}
              loading={doctorLoading}
              onRefresh={() => {
                setDoctorLoading(true)
                void getVmDoctor(vmId)
                  .then(setDoctor)
                  .catch(() => setDoctor(null))
                  .finally(() => setDoctorLoading(false))
              }}
              onTab={(t) => onOpenVmDetail?.(t)}
            />
          )}

          {activeTab === 'Events' && (
            <MachineTimeline sessions={sessions} timeline={timeline} />
          )}

          {activeTab === 'AI' && (
            <ConsoleCopilotLens
              vmId={vmId}
              vmName={vmName}
              activeLens="ai"
              guestIp={guestIp}
              vmState={vmState}
            />
          )}

          <div className="pt-2 border-t border-white/5">
            <Link
              to={`/platform/vms/${vmId}`}
              className="text-xs text-[var(--link)] hover:underline"
              onClick={onClose}
            >
              Open full VM detail →
            </Link>
          </div>
        </div>
      </aside>
    </>
  )
}
