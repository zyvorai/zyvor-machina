// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ReactNode } from 'react'
import { useState } from 'react'
import { ArrowLeft, ChevronLeft } from 'lucide-react'
import { statusBadgeClasses } from '../../utils/semanticColors'
import { vmSemanticKind } from '../../utils/vmVisual'
import type { ConsoleHubPlan, VmPortForwardRule } from '../../api/platform'
import AccessNotePill from './AccessNotePill'
import CinemaControlStrip from './CinemaControlStrip'
import FloatingConsoleHud from './FloatingConsoleHud'
import ConsoleMinimap from './ConsoleMinimap'
import ConsoleCommandPalette, {
  buildDefaultConsoleActions,
  useConsoleCommandPaletteShortcut,
} from './ConsoleCommandPalette'
import ConsoleWatermark from './ConsoleWatermark'
import { useConsoleViewport } from './ConsoleViewportContext'

type Props = {
  vmId: string
  vmName: string
  vmState?: string | null
  guestIp?: string | null
  nodeName?: string | null
  plan: ConsoleHubPlan | null
  loading?: boolean
  hypervisorAddress?: string
  portForwardRules?: VmPortForwardRule[]
  onPlanRefresh?: () => void
  onNotify?: (message: string) => void
  children: ReactNode
  onBack: () => void
  onOpenStudio: () => void
  onOpenOpsShelf: () => void
  onOpenAi: () => void
  onSwitchLens: (lens: string) => void
  onCtrlAltDel: () => void
  onSendKey: (preset: 'esc' | 'ctrl_alt_del' | 'alt_tab') => void
  onPower: (action: 'shutdown' | 'reboot' | 'stop' | 'start') => void
  onScreenshot: () => void
  onSnapshot: () => void
  onExposeSsh?: () => void
  libvirt?: boolean
  onOpenHardware?: () => void
  onOpenNetwork?: () => void
  displayProtocols?: string[]
  activeProtocol?: string
  onProtocolChange?: (p: string) => void
  watermarkLabel?: string | null
  recordingActive?: boolean
  readOnly?: boolean
  onShareView?: () => void
  shareBusy?: boolean
  spiceAudioEnabled?: boolean
  onToggleSpiceAudio?: () => void
}

function stateTone(state?: string | null): 'ok' | 'warn' | 'error' | 'neutral' {
  const s = (state ?? '').toLowerCase()
  if (s.includes('run') || s === 'active') return 'ok'
  if (s.includes('migrat') || s.includes('pause')) return 'warn'
  if (s.includes('fail') || s.includes('error')) return 'error'
  return 'neutral'
}

export default function CinemaShell({
  vmId,
  vmName,
  vmState,
  guestIp,
  nodeName,
  plan,
  loading,
  hypervisorAddress,
  portForwardRules = [],
  onPlanRefresh,
  onNotify,
  children,
  onBack,
  onOpenStudio,
  onOpenOpsShelf,
  onOpenAi,
  onSwitchLens,
  onCtrlAltDel,
  onSendKey,
  onPower,
  onScreenshot,
  onSnapshot,
  onExposeSsh,
  libvirt = true,
  onOpenHardware,
  onOpenNetwork,
  displayProtocols = [],
  activeProtocol,
  onProtocolChange,
  watermarkLabel,
  recordingActive,
  readOnly = false,
  onShareView,
  shareBusy = false,
  spiceAudioEnabled = false,
  onToggleSpiceAudio,
}: Props) {
  const [paletteOpen, setPaletteOpen] = useState(false)
  const vp = useConsoleViewport()
  const tone = stateTone(vmState)
  const offline = vmSemanticKind(vmState ?? undefined) === 'stopped'

  useConsoleCommandPaletteShortcut(() => setPaletteOpen(true), !loading)

  const actions = buildDefaultConsoleActions({
    onSwitchLens,
    onCtrlAltDel,
    onSetMode: (mode) => vp.setMode(mode),
    onExposeSsh,
    onSnapshot,
    onOpenAi,
    onOpenOpsShelf,
    onOpenStudio,
    vmDetailHref: `/platform/vms/${vmId}`,
  })

  return (
    <div className="fixed inset-0 z-[60] bg-[#030305] flex flex-col min-h-0" data-testid="cinema-shell">
      <header className="shrink-0 flex items-center gap-3 px-3 py-2 border-b border-white/[0.06] bg-black/40 backdrop-blur-md text-xs">
        <button type="button" className="inline-flex items-center gap-1 text-[var(--text-secondary)] hover:text-white" onClick={onBack}>
          <ArrowLeft className="w-4 h-4" /> Back
        </button>
        <span className="text-[var(--text-muted)]">Machina Cinema</span>
        <span className={`w-2 h-2 rounded-full ${tone === 'ok' ? 'bg-emerald-400' : tone === 'warn' ? 'bg-amber-400' : 'bg-slate-500'}`} />
        {/* This header is fixed-dark chrome (bg-[#030305]) in both themes, so it needs a fixed
            light ink, not the theme-switching --text-primary (which is near-black in light theme
            and was ~1.2:1 here). */}
        <span className="font-semibold text-white truncate">{vmName}</span>
        {vmState ? <span className={`px-2 py-0.5 rounded-full capitalize ${statusBadgeClasses(tone)}`}>{vmState}</span> : null}
        {guestIp ? <span className="font-mono text-emerald-300/90 hidden sm:inline">{guestIp}</span> : null}
        {nodeName ? <span className="text-[var(--text-muted)] hidden md:inline">Node {nodeName}</span> : null}
        {recordingActive ? (
          <span className="px-2 py-0.5 rounded-full bg-red-950/80 border border-red-500/40 text-red-200 text-[10px] uppercase tracking-wide" data-testid="cinema-recording-badge">
            Rec
          </span>
        ) : null}
        {readOnly ? (
          <span className="px-2 py-0.5 rounded-full bg-amber-950/80 border border-amber-500/40 text-amber-100 text-[10px] uppercase tracking-wide" data-testid="cinema-readonly-badge">
            View only
          </span>
        ) : null}
        {spiceAudioEnabled ? (
          <span className="px-2 py-0.5 rounded-full bg-[var(--apple-surface)] border border-[var(--accent)]/40 text-[var(--text-primary)] text-[10px] uppercase tracking-wide" data-testid="cinema-spice-audio-badge">
            SPICE audio
          </span>
        ) : null}
        <span className="ml-auto text-[var(--text-muted)] hidden sm:inline">{readOnly ? 'Spectator session' : 'Secure session'}</span>
      </header>

      <div className="relative flex-1 min-h-0 flex flex-col" data-cinema-viewport>
        <AccessNotePill
          hints={plan?.guest_access}
          vmId={vmId}
          vmName={vmName}
          sshUser={plan?.ssh_user ?? undefined}
          guestIp={plan?.guest_ip ?? undefined}
          osHint={plan?.os_hint}
          hypervisorHost={hypervisorAddress ?? plan?.hypervisor_address ?? undefined}
          portForwardRules={portForwardRules}
          onPlanRefresh={onPlanRefresh}
          onNotify={onNotify}
          onOpenShell={() => onSwitchLens('shell')}
          onExplain={onOpenAi}
        />

        {displayProtocols.length > 1 && onProtocolChange ? (
          <div className="absolute top-14 right-3 z-30 flex gap-1">
            {displayProtocols.filter((p) => p !== 'native_ssh' && p !== 'serial').map((p) => (
              <button
                key={p}
                type="button"
                onClick={() => onProtocolChange(p)}
                className={
                  p === activeProtocol
                    ? 'px-2 py-0.5 rounded-full text-[10px] bg-emerald-900/50 text-emerald-100 border border-[var(--apple-hairline)]'
                    : 'px-2 py-0.5 rounded-full text-[10px] text-[var(--text-muted)] bg-black/50 border border-white/10'
                }
              >
                {p.replace(/_/g, ' ')}
              </button>
            ))}
          </div>
        ) : null}

        <div className="flex-1 min-h-0 w-full relative flex flex-col bg-black">
          {loading ? (
            <div className="flex items-center justify-center flex-1 text-[var(--text-muted)] text-sm">Connecting…</div>
          ) : (
            children
          )}
          {watermarkLabel ? <ConsoleWatermark label={watermarkLabel} sublabel={recordingActive ? 'Audit trail' : readOnly ? 'Spectator' : undefined} /> : null}
          {offline && !loading ? (
            <div
              className="absolute inset-0 z-20 flex flex-col items-center justify-center gap-3 bg-black/85 px-6 text-center pointer-events-none"
              data-testid="cinema-offline-overlay"
            >
              <p className="text-[var(--text-primary)] text-sm font-medium">VM is powered off</p>
              <p className="text-[var(--text-muted)] text-xs max-w-md">
                Graphical console needs a running guest. Use the Start control in the bar below, or open Studio for serial recovery.
              </p>
            </div>
          ) : null}
          <FloatingConsoleHud visible={!loading} />
          <CinemaControlStrip
            visible={!loading}
            vmId={vmId}
            vmState={vmState}
            readOnly={readOnly}
            libvirt={libvirt}
            onCtrlAltDel={readOnly ? undefined : onCtrlAltDel}
            onSendKey={readOnly ? undefined : onSendKey}
            onPower={readOnly ? undefined : onPower}
            onScreenshot={onScreenshot}
            onOpenAi={onOpenAi}
            onOpenOpsShelf={onOpenOpsShelf}
            onOpenStudio={onOpenStudio}
            onOpenHardware={onOpenHardware}
            onOpenNetwork={onOpenNetwork}
            onSnapshot={readOnly ? undefined : onSnapshot}
            onSwitchLens={onSwitchLens}
            displayProtocols={displayProtocols}
            activeProtocol={activeProtocol}
            onProtocolChange={onProtocolChange}
            onRecord={() => onNotify?.(recordingActive ? 'Session is being recorded for audit' : 'Audit session recording isn’t enabled on this controller — ask an administrator to turn it on.')}
            onShareView={onShareView}
            shareBusy={shareBusy}
            spiceAudioEnabled={spiceAudioEnabled}
            onToggleSpiceAudio={onToggleSpiceAudio}
          />
          <ConsoleMinimap />
        </div>

        <button
          type="button"
          className="absolute right-0 top-1/2 -translate-y-1/2 z-30 px-1 py-3 rounded-l-lg bg-black/60 border border-white/10 border-r-0 text-[var(--text-secondary)] hover:bg-black/80 text-sm"
          onClick={onOpenOpsShelf}
          aria-label="Open Ops Shelf"
          data-testid="ops-shelf-handle"
        >
          <ChevronLeft className="w-4 h-4" />
        </button>
      </div>

      <ConsoleCommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} actions={actions} />
    </div>
  )
}
