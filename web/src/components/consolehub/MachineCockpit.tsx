// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ReactNode } from 'react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { ConsoleViewportProvider, useConsoleViewport } from './ConsoleViewportContext'
import { ConsoleClipboardProvider } from './ConsoleClipboardContext'
import MachineCommandStrip from './MachineCommandStrip'
import ViewLensBar, { lensToProtocol, type ConsoleLens } from './ViewLensBar'
import MachineCanvas from './MachineCanvas'
import FloatingConsoleHud from './FloatingConsoleHud'
import CommandDock from './CommandDock'
import ConsoleMinimap from './ConsoleMinimap'
import ZeroPanicRecoveryBar from './ZeroPanicRecoveryBar'
import ConsoleCopilotLens from './ConsoleCopilotLens'
import CommandCenterPanel, { type CommandCenterTab } from './CommandCenterPanel'
import MachineTimeline from './MachineTimeline'
import CinemaShell from './CinemaShell'
import StudioLayout from './StudioLayout'
import type { ConsoleHubPlan, ConsoleHubSessionResponse } from '../../api/platform'
import { createConsoleCollaborateLink, createVmSnapshot, endConsoleHubSession, fetchConsoleSessionReplay, vmPower } from '../../api/platform'
import type { ConsoleHubSessionRow } from './ConsoleHubSessionHistory'
import type { VmTimelineEntry } from '../../api/platformVmTimeline'
import { recipeForError, type ConsoleRecipe } from '../../data/consoleRecipes'
import ConsoleHubSession from './ConsoleHubSession'
import GuestAccessBanner from './GuestAccessBanner'
import ShellAccessBanner from './ShellAccessBanner'
import ConsoleLoginRecoveryCard from './ConsoleLoginRecoveryCard'
import VmPortForwardPanel from '../vm/VmPortForwardPanel'
import type { VmPortForwardRule } from '../../api/platform'
import { exposeGuestPortOnVm } from '../../utils/vmPortForwardServices'
import { formatUserError } from '../../utils/apiError'
import { sendGuestKey } from '../../api/vm'
import { useToastContext } from '../../contexts/ToastContext'
import type { ConsoleExperienceMode } from '../../utils/consoleExperienceMode'
import { vmSemanticKind } from '../../utils/vmVisual'
import { isDisplayProtocol } from '../../utils/consoleExperienceMode'
import { downloadCanvasScreenshot, saveVmPosterScreenshot } from '../../utils/vmPosterScreenshot'
import { getDefaultLens, spectatorCinemaPath } from '../../utils/consoleExperienceMode'
import { useConsoleAccessPolicy } from '../../hooks/useConsoleAccessPolicy'
import { useConsoleSessionRecorder } from '../../hooks/useConsoleSessionRecorder'
import { useVmHardware } from '../../hooks/useVmHardware'
import VmHardwareDrawer from '../vm/VmHardwareDrawer'
import VmKubevirtHardwareDrawer from '../vm/VmKubevirtHardwareDrawer'
import { useKubevirtHardware } from '../../hooks/useKubevirtHardware'
import VmNetworkDrawer from '../vm/VmNetworkDrawer'

type MachineCockpitProps = {
  vmId: string
  vmName: string
  plan: ConsoleHubPlan | null
  session: ConsoleHubSessionResponse | null
  wsUrl: string | null
  serialWsUrl?: string | null
  platformSpiceWsPath?: string | null
  activeProtocol: string
  onProtocolChange: (protocol: string) => void
  vmState?: string | null
  nodeName?: string | null
  healthScore?: number | null
  kubeVirtNamespace?: string | null
  error?: string | null
  loading?: boolean
  history?: ConsoleHubSessionRow[]
  machineTimeline?: VmTimelineEntry[]
  isPopout?: boolean
  onReconnect?: () => void
  onPopout?: () => void
  connectKey?: number
  prepend?: ReactNode
  hypervisorAddress?: string
  portForwardRules?: VmPortForwardRule[]
  inventorySource?: string | null
  hostId?: string | null
  onPlanRefresh?: () => void
  experienceMode?: ConsoleExperienceMode
  onExperienceModeChange?: (mode: ConsoleExperienceMode) => void
  /** Lifts a freshly-created session (e.g. break-glass) into the owning
   * page's `session` state so recording_enabled actually flows into
   * useConsoleAccessPolicy / useConsoleSessionRecorder. */
  onSessionStart?: (session: ConsoleHubSessionResponse) => void
}

function CockpitInner({
  vmId,
  vmName,
  plan,
  session,
  wsUrl,
  serialWsUrl,
  platformSpiceWsPath = null,
  activeProtocol,
  onProtocolChange,
  vmState,
  nodeName,
  healthScore,
  kubeVirtNamespace,
  error,
  loading,
  history = [],
  machineTimeline = [],
  isPopout,
  onPopout,
  onReconnect,
  connectKey = 0,
  prepend,
  hypervisorAddress,
  portForwardRules = [],
  inventorySource,
  hostId,
  onPlanRefresh,
  experienceMode = 'cinema',
  onExperienceModeChange,
  onSessionStart,
}: MachineCockpitProps) {
  const toast = useToastContext()
  const navigate = useNavigate()
  const vp = useConsoleViewport()
  const { setProtocol, setMode, setConnected } = vp
  const access = useConsoleAccessPolicy(plan, session)
  const lensInitialized = useRef(false)
  const [lens, setLens] = useState<ConsoleLens>('display')
  const [commandCenter, setCommandCenter] = useState(false)
  const [ccTab, setCcTab] = useState<CommandCenterTab>('Overview')
  const [activeRecipe, setActiveRecipe] = useState<ConsoleRecipe | null>(null)
  const [exposeBusy, setExposeBusy] = useState(false)
  const [vncCanvas, setVncCanvas] = useState<HTMLCanvasElement | null>(null)
  const [shareBusy, setShareBusy] = useState(false)
  const [shareLink, setShareLink] = useState<string | null>(null)
  const [spiceAudio, setSpiceAudio] = useState(true)
  const [hardwareOpen, setHardwareOpen] = useState(false)
  const [networkOpen, setNetworkOpen] = useState(false)

  const isLibvirt = inventorySource !== 'kubevirt'
  const hardware = useVmHardware({
    vmId,
    enabled: isLibvirt,
    inventorySource,
    portForwardRules,
    protocols: plan?.protocols ?? [],
    osHint: plan?.os_hint,
  })
  const kubevirtHardware = useKubevirtHardware({
    vmId,
    enabled: !isLibvirt,
  })

  useConsoleSessionRecorder({
    canvas: vncCanvas,
    sessionId: session?.session_id,
    recordingActive: access.recordingActive,
    readOnly: access.readOnly,
  })

  // Ends the console-hub session server-side (and revokes its ws token) when
  // the session changes or this cockpit unmounts (navigate-away/tab-close via
  // React's unmount cleanup) — regardless of whether it was recorded.
  // Previously only useConsoleSessionRecorder called endConsoleHubSession, and
  // only when recordingActive, so a non-recorded session (the common case)
  // was left with ended_at = NULL server-side when the user simply navigated
  // away. This runs independently of recording state.
  useEffect(() => {
    const sid = session?.session_id
    if (!sid) return
    return () => {
      void endConsoleHubSession(sid).catch(() => undefined)
    }
  }, [session?.session_id])

  const displayProtocols = plan
    ? [...plan.protocols, ...(plan.guest_ip && !plan.protocols.includes('native_ssh') ? ['native_ssh'] : [])]
    : []

  const cinemaActive = experienceMode === 'cinema' && lens === 'display' && isDisplayProtocol(activeProtocol)
  const studioActive = experienceMode === 'studio'
  const spiceDisplay = activeProtocol === 'spice' || activeProtocol === 'webrtc_spice'
  const enableSpiceAudio = spiceDisplay && spiceAudio && !access.readOnly

  useEffect(() => {
    setProtocol(activeProtocol)
    if (activeProtocol === 'novnc' || activeProtocol === 'rdp') {
      setMode('fit')
    }
    if (loading) setConnected(false)
  }, [activeProtocol, loading, setProtocol, setMode, setConnected])

  // Reset the lens guard when navigating to a different VM so the new plan picks its own default.
  useEffect(() => {
    lensInitialized.current = false
  }, [vmId])

  // Cockpit pattern: set the default lens ONCE when the plan first arrives.
  // Decision is made from VM capabilities (native.available, console_type, webrtc_spice_available),
  // not from the backend `recommended` string — mirrors getDefaultConsole() in cockpit/pkg/machines.
  useEffect(() => {
    if (!plan || lensInitialized.current) return
    lensInitialized.current = true
    const defaultLens = getDefaultLens(plan)
    setLens(defaultLens)
    if (defaultLens === 'serial' && experienceMode === 'cinema') {
      onExperienceModeChange?.('studio')
    }
  }, [plan, experienceMode, onExperienceModeChange])

  // If the plan no longer advertises the current protocol (e.g. serial removed
  // because the domain has no PTY), fall back so Studio isn't stuck on a dead console.
  useEffect(() => {
    if (!plan || displayProtocols.length === 0) return
    if (displayProtocols.includes(activeProtocol)) return
    const fallback = plan.recommended && displayProtocols.includes(plan.recommended)
      ? plan.recommended
      : (displayProtocols.find((p) => p === 'novnc' || p === 'spice' || p === 'webrtc_spice') ?? displayProtocols[0])
    if (fallback) {
      onProtocolChange(fallback)
      setLens(fallback === 'serial' ? 'serial' : fallback === 'native_ssh' ? 'shell' : 'display')
    }
  }, [plan, displayProtocols, activeProtocol, onProtocolChange])

  const switchLens = useCallback(
    (next: ConsoleLens | 'native_ssh') => {
      const resolved = next === 'native_ssh' ? 'shell' : next
      if (resolved === 'shell' || resolved === 'serial') {
        onExperienceModeChange?.('studio')
      }
      setLens(resolved)
      if (resolved === 'ai' || resolved === 'events' || resolved === 'recovery') return
      if (resolved === 'network' || resolved === 'perf') {
        setCommandCenter(true)
        setCcTab(resolved === 'network' ? 'Overview' : 'Health')
        return
      }
      const proto = lensToProtocol(resolved, displayProtocols, plan?.recommended ?? 'novnc')
      onProtocolChange(proto)
    },
    [displayProtocols, onExperienceModeChange, onProtocolChange, plan?.recommended],
  )

  const sendCtrlAltDel = async () => {
    if (!access.canSendKeys) {
      toast.info('Read-only session — cannot send keys')
      return
    }
    // Prefer noVNC-native key injection (works without qemu-guest-agent).
    if (vp.sendCtrlAltDel) {
      vp.sendCtrlAltDel()
      toast.success('Sent Ctrl+Alt+Del')
      return
    }
    try {
      await sendGuestKey(vmName, { preset: 'ctrl_alt_del' })
      toast.success('Sent Ctrl+Alt+Del')
    } catch (e: unknown) {
      toast.error(`Could not send key: ${formatUserError(e)}`)
    }
  }

  const sendKeyPreset = async (preset: 'esc' | 'ctrl_alt_del' | 'alt_tab') => {
    if (!access.canSendKeys) {
      toast.info('Read-only session — cannot send keys')
      return
    }
    // Prefer noVNC-native injection (works without qemu-guest-agent) — same as
    // sendCtrlAltDel. Previously these always used the guest-agent HTTP API, so
    // Esc/Alt+Tab failed on a VNC guest without the agent while Ctrl+Alt+Del worked.
    if (vp.sendKeyPreset(preset)) {
      toast.success(`Sent ${preset}`)
      return
    }
    try {
      await sendGuestKey(vmName, { preset })
      toast.success(`Sent ${preset}`)
    } catch (e: unknown) {
      toast.error(`Could not send key: ${formatUserError(e)}`)
    }
  }

  const handlePower = async (action: 'shutdown' | 'reboot' | 'stop' | 'start') => {
    if (!access.canPower) {
      toast.info('Read-only session — power actions disabled')
      return
    }
    const offline = vmSemanticKind(vmState ?? undefined) === 'stopped'
    // 'start' now arrives directly from the control strip; keep the legacy
    // reboot+offline→start remap for any other caller.
    const effectiveAction =
      action === 'reboot' && offline ? ('start' as const) : action
    try {
      if (effectiveAction === 'start') await vmPower(vmId, 'start')
      else if (effectiveAction === 'reboot') await vmPower(vmId, 'reboot')
      else if (effectiveAction === 'shutdown') await vmPower(vmId, 'shutdown')
      else await vmPower(vmId, 'stop')
      toast.success(`${effectiveAction === 'start' ? 'start' : action} queued`)
      onPlanRefresh?.()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const handleSnapshot = async () => {
    if (!access.canSnapshot) {
      toast.info('Read-only session — snapshots disabled')
      return
    }
    try {
      await createVmSnapshot(vmId, {
        name: `snap-${new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-')}`,
        disk_only: true,
      })
      toast.success('Snapshot queued')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const handleScreenshot = () => {
    if (!vncCanvas) {
      toast.error('Display not ready')
      return
    }
    downloadCanvasScreenshot(vncCanvas, `${vmName}-cinema.png`)
    saveVmPosterScreenshot(vmId, vncCanvas.toDataURL('image/png'))
    toast.success('Screenshot saved')
  }

  const handleOpenReplay = useCallback(async (sessionId: string) => {
    try {
      const blob = await fetchConsoleSessionReplay(sessionId)
      const url = URL.createObjectURL(blob)
      window.open(url, '_blank', 'noopener,noreferrer')
      setTimeout(() => URL.revokeObjectURL(url), 60_000)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }, [toast])

  const handleShareView = useCallback(async () => {
    if (!access.canPower || access.readOnly) return
    if (session?.spectator_token && session.session_id) {
      const link = spectatorCinemaPath(vmId, session.session_id, session.spectator_token)
      setShareLink(link)
      await navigator.clipboard.writeText(`${window.location.origin}${link}`)
      toast.success('Spectator link copied — read-only Cinema view')
      return
    }
    setShareBusy(true)
    try {
      const res = await createConsoleCollaborateLink(vmId, {
        protocol: activeProtocol,
        reason: 'Shared Cinema view',
      })
      const token = res.spectator_token
      if (!token) throw new Error('No spectator token returned')
      const link = spectatorCinemaPath(vmId, res.session_id, token)
      setShareLink(link)
      await navigator.clipboard.writeText(`${window.location.origin}${link}`)
      toast.success('Collaborator link copied — viewers get read-only access')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setShareBusy(false)
    }
  }, [access.canPower, access.readOnly, session, vmId, activeProtocol, toast])

  const exposeSsh = useCallback(() => {
    if (!plan?.guest_access?.guest_ip_private) return
    setExposeBusy(true)
    void (async () => {
      try {
        await exposeGuestPortOnVm(vmId, vmName, 22, portForwardRules)
        toast.success('SSH exposed on hypervisor')
        onPlanRefresh?.()
      } catch (e: unknown) {
        toast.error(formatUserError(e))
      } finally {
        setExposeBusy(false)
      }
    })()
  }, [plan?.guest_access?.guest_ip_private, portForwardRules, vmId, vmName, onPlanRefresh, toast])

  const handleCommandCenterAction = useCallback(
    async (action: string) => {
      switch (action) {
        case 'Snapshot':
          await handleSnapshot()
          break
        case 'Restart':
          await handlePower('reboot')
          break
        case 'Inspect Disk':
          navigate(`/platform/vms/${vmId}?tab=disks`)
          setCommandCenter(false)
          break
        case 'eBPF Trace':
          navigate(`/platform/vms/${vmId}?tab=security`)
          setCommandCenter(false)
          break
        case 'Migrate':
          navigate(`/platform/vms/${vmId}?tab=settings`)
          setCommandCenter(false)
          break
        default:
          toast.info('Open VM detail for full workflow')
      }
    },
    [navigate, toast, vmId],
  )

  const recipe = activeRecipe ?? recipeForError(error ?? null)

  const sessionBlock = (
    <ConsoleHubSession
      key={`${activeProtocol}-${wsUrl ?? 'none'}-${connectKey}`}
      protocol={activeProtocol}
      vmName={vmName}
      wsUrl={wsUrl}
      serialWsUrl={serialWsUrl}
      session={session}
      guestIp={plan?.guest_ip ?? undefined}
      sshUser={plan?.ssh_user ?? undefined}
      sshConnectHost={plan?.ssh_connect_host ?? undefined}
      sshConnectPort={plan?.ssh_connect_port ?? undefined}
      kubeVirtNamespace={kubeVirtNamespace ?? undefined}
      fillViewport
      cockpitMode
      onReconnect={onReconnect}
      connectKey={connectKey}
      onCanvasReady={setVncCanvas}
      enableSpiceAudio={enableSpiceAudio}
      platformSpiceWsPath={platformSpiceWsPath}
    />
  )

  const accessBanners =
    activeProtocol === 'serial' || activeProtocol === 'native_ssh' ? (
      <>
        <GuestAccessBanner
          hints={plan?.guest_access}
          lens={activeProtocol === 'serial' ? 'serial' : 'shell'}
          sshUser={plan?.ssh_user ?? undefined}
          guestIp={plan?.guest_ip ?? undefined}
          vmId={vmId}
          vmName={vmName}
          hypervisorHost={hypervisorAddress ?? plan?.hypervisor_address ?? undefined}
          portForwardRules={portForwardRules}
          onPlanRefresh={onPlanRefresh}
          onNotify={(m) => toast.success(m)}
        />
        {activeProtocol === 'serial' && plan?.guest_access ? (
          <ConsoleLoginRecoveryCard
            hints={plan.guest_access}
            vmId={vmId}
            exposing={exposeBusy}
            onSwitchToShell={() => switchLens('shell')}
            onExposeSsh={
              plan.guest_access.guest_ip_private && !plan.guest_access.ssh_nat_host_port ? exposeSsh : undefined
            }
          />
        ) : null}
        {activeProtocol === 'native_ssh' ? (
          <ShellAccessBanner
            hints={plan?.guest_access}
            sshUser={plan?.ssh_user ?? undefined}
            guestIp={plan?.guest_ip ?? undefined}
            hypervisorHost={hypervisorAddress ?? plan?.hypervisor_address ?? undefined}
            portForwardRules={portForwardRules}
            onNotify={(m) => toast.success(m)}
          />
        ) : null}
      </>
    ) : null

  const canvasContent = (() => {
    if (lens === 'ai') {
      return (
        <ConsoleCopilotLens
          vmId={vmId}
          vmName={vmName}
          activeLens={lens}
          guestIp={plan?.guest_ip}
          vmState={vmState}
          onSwitchLens={(l) => switchLens(l as ConsoleLens)}
        />
      )
    }
    if (lens === 'events') {
      return (
        <div className="p-4 overflow-y-auto flex-1">
          <MachineTimeline sessions={history} timeline={machineTimeline} />
        </div>
      )
    }
    if (lens === 'recovery') {
      return (
        <div className="p-4 overflow-y-auto flex-1">
          <ZeroPanicRecoveryBar
            error={error ?? 'Select a recovery action'}
            vmState={vmState}
            recipe={recipe}
            onReconnect={onReconnect}
            onOpenSerial={() => switchLens('serial')}
            onOpenSsh={() => switchLens('shell')}
            onOpenEvents={() => switchLens('events')}
            onAiDiagnose={() => switchLens('ai')}
            onRunRecipe={setActiveRecipe}
          />
        </div>
      )
    }
    if (lens === 'network') {
      return (
        <div className="p-4 overflow-y-auto flex-1 space-y-4">
          <p className="text-sm text-[var(--text-secondary)]">
            Guest IP: <span className="font-mono text-emerald-300">{plan?.guest_ip ?? '—'}</span>
          </p>
          {plan?.guest_access?.guest_ip_private && vmId && vmName && plan?.guest_ip ? (
            <VmPortForwardPanel
              platformVmId={vmId}
              vmName={vmName}
              guestIp={plan.guest_ip}
              sshUser={plan.ssh_user ?? undefined}
              hypervisorAddress={hypervisorAddress ?? plan.hypervisor_address ?? undefined}
              compact
              onNotify={(m) => toast.success(m)}
            />
          ) : (
            <p className="text-xs text-[var(--text-muted)]">NAT port forwarding is available when the guest has a private libvirt IP.</p>
          )}
        </div>
      )
    }
    return (
      <div className="flex flex-col flex-1 min-h-0 w-full gap-2">
        {!cinemaActive ? accessBanners : null}
        {sessionBlock}
      </div>
    )
  })()

  const opsShelf = (
    <CommandCenterPanel
      open={commandCenter && lens !== 'events'}
      onClose={() => setCommandCenter(false)}
      vmId={vmId}
      vmName={vmName}
      healthScore={healthScore}
      vmState={vmState}
      guestIp={plan?.guest_ip}
      guestAccess={plan?.guest_access}
      hypervisorAddress={hypervisorAddress ?? plan?.hypervisor_address ?? undefined}
      sshUser={plan?.ssh_user ?? undefined}
      sessions={history}
      timeline={machineTimeline}
      activeTab={ccTab}
      onTabChange={setCcTab}
      onAction={(action) => void handleCommandCenterAction(action)}
      onPlanRefresh={onPlanRefresh}
      activeProtocol={activeProtocol}
      canBreakGlass={access.canPower && !access.spectatorMode}
      shareLink={shareLink}
      onShareView={() => void handleShareView()}
      onOpenReplay={(sessionId) => void handleOpenReplay(sessionId)}
      portForwardRules={portForwardRules}
      readOnly={access.readOnly}
      onExposeSsh={plan?.guest_access?.guest_ip_private ? exposeSsh : undefined}
      onSessionStart={onSessionStart}
      onOpenVmDetail={(tab) => {
        setCommandCenter(false)
        navigate(tab ? `/platform/vms/${vmId}?tab=${tab}` : `/platform/vms/${vmId}`)
      }}
    />
  )

  if (cinemaActive) {
    return (
      <>
        <CinemaShell
          vmId={vmId}
          vmName={vmName}
          vmState={vmState}
          guestIp={plan?.guest_ip}
          nodeName={nodeName}
          plan={plan}
          loading={loading}
          hypervisorAddress={hypervisorAddress}
          portForwardRules={portForwardRules}
          onPlanRefresh={onPlanRefresh}
          onNotify={(m) => toast.success(m)}
          onBack={() => navigate(`/platform/vms/${vmId}`)}
          onOpenStudio={() => onExperienceModeChange?.('studio')}
          onOpenOpsShelf={() => { setCommandCenter(true); setCcTab('Overview') }}
          onOpenAi={() => { setCommandCenter(true); setCcTab('AI') }}
          onSwitchLens={(l) => switchLens(l as ConsoleLens)}
          onCtrlAltDel={() => void sendCtrlAltDel()}
          onSendKey={(p) => void sendKeyPreset(p as 'esc' | 'ctrl_alt_del' | 'alt_tab')}
          onPower={(a) => void handlePower(a)}
          onScreenshot={handleScreenshot}
          onSnapshot={() => void handleSnapshot()}
          onExposeSsh={plan?.guest_access?.guest_ip_private ? exposeSsh : undefined}
          libvirt={isLibvirt}
          onOpenHardware={() => setHardwareOpen(true)}
          onOpenNetwork={() => setNetworkOpen(true)}
          displayProtocols={displayProtocols}
          activeProtocol={activeProtocol}
          onProtocolChange={onProtocolChange}
          watermarkLabel={access.watermarkLabel}
          recordingActive={access.recordingActive}
          readOnly={access.readOnly}
          onShareView={access.canPower && !access.spectatorMode ? () => void handleShareView() : undefined}
          shareBusy={shareBusy}
          spiceAudioEnabled={enableSpiceAudio}
          onToggleSpiceAudio={spiceDisplay && !access.readOnly ? () => setSpiceAudio((v) => !v) : undefined}
        >
          {sessionBlock}
        </CinemaShell>
        {isLibvirt ? (
          <VmHardwareDrawer
            open={hardwareOpen}
            onClose={() => setHardwareOpen(false)}
            vmId={vmId}
            vmName={vmName}
            hostId={hostId}
            vmState={vmState ?? undefined}
            hardware={hardware}
            portForwardRules={portForwardRules}
            protocols={plan?.protocols ?? []}
            readOnly={access.readOnly}
            onPlanRefresh={onPlanRefresh}
          />
        ) : (
          <VmKubevirtHardwareDrawer
            open={hardwareOpen}
            onClose={() => setHardwareOpen(false)}
            vmId={vmId}
            vmName={vmName}
            hardware={kubevirtHardware}
          />
        )}
        <VmNetworkDrawer
          open={networkOpen}
          onClose={() => setNetworkOpen(false)}
          vmId={vmId}
          vmName={vmName}
          guestIp={plan?.guest_ip}
          sshUser={plan?.ssh_user ?? undefined}
          hypervisorAddress={hypervisorAddress ?? plan?.hypervisor_address ?? undefined}
          onPlanRefresh={onPlanRefresh}
          onNotify={(m) => toast.success(m)}
        />
        {opsShelf}
      </>
    )
  }

  if (studioActive) {
    // Display, Serial, and Shell are each a full-size lens tab (switched via
    // ViewLensBar) — never rendered side by side. A prior side-by-side Split
    // mode (fixed-ratio grid, on by default, then made opt-in) still read as
    // a cramped shared screen rather than genuinely independent consoles, so
    // it's gone: one console fills the panel, switching lenses is one click.
    return (
      <div className="flex flex-col flex-1 min-h-0 w-full">
        {prepend}
        {!error || wsUrl ? null : (
          <ZeroPanicRecoveryBar
            error={error}
            vmState={vmState}
            recipe={recipe}
            onReconnect={onReconnect}
            onOpenSerial={() => switchLens('serial')}
            onOpenSsh={() => switchLens('shell')}
            onOpenEvents={() => { setLens('events'); setCommandCenter(true); setCcTab('Events') }}
            onAiDiagnose={() => switchLens('ai')}
            onRunRecipe={setActiveRecipe}
          />
        )}
        <StudioLayout
          vmName={vmName}
          vmState={vmState}
          guestIp={plan?.guest_ip}
          nodeName={nodeName}
          healthScore={healthScore}
          osHint={plan?.os_hint}
          lens={lens}
          onLensChange={switchLens}
          displayProtocols={displayProtocols}
          activeProtocol={activeProtocol}
          onProtocolChange={onProtocolChange}
          recommended={plan?.recommended}
          onCommandCenter={() => { setCommandCenter(true); setCcTab('Overview') }}
          onAi={() => { setCommandCenter(true); setCcTab('AI') }}
          onOpenCinema={() => onExperienceModeChange?.('cinema')}
          onPopout={onPopout}
          primary={
            <MachineCanvas vmState={vmState} healthScore={healthScore} theatre className="flex-1">
              {loading ? (
                <div className="flex items-center justify-center flex-1 text-[var(--text-muted)] text-sm">Loading…</div>
              ) : (
                canvasContent
              )}
            </MachineCanvas>
          }
          timeline={<MachineTimeline sessions={history} timeline={machineTimeline} />}
        />
        {opsShelf}
      </div>
    )
  }

  return (
    <div className={`flex flex-col flex-1 min-h-0 w-full ${isPopout ? 'fixed inset-0 z-[55] bg-[var(--page-bg)] p-2 md:p-4' : ''}`}>
      {prepend}
      {!error || wsUrl ? null : (
        <ZeroPanicRecoveryBar
          error={error}
          vmState={vmState}
          recipe={recipe}
          onReconnect={onReconnect}
          onOpenSerial={() => switchLens('serial')}
          onOpenSsh={() => switchLens('shell')}
          onOpenEvents={() => { setLens('events'); setCommandCenter(true); setCcTab('Events') }}
          onAiDiagnose={() => switchLens('ai')}
          onRunRecipe={setActiveRecipe}
        />
      )}
      <MachineCommandStrip
        vmName={vmName}
        vmState={vmState}
        guestIp={plan?.guest_ip}
        osHint={plan?.os_hint}
        nodeName={nodeName}
        healthScore={healthScore}
        onEnterTheatre={() => onExperienceModeChange?.('cinema')}
        onCommandCenter={() => { setCommandCenter(true); setCcTab('Overview') }}
        onAi={() => { setCommandCenter(true); setCcTab('AI') }}
      />
      <ViewLensBar
        active={lens}
        onChange={switchLens}
        displayProtocols={displayProtocols}
        activeProtocol={activeProtocol}
        onProtocolChange={onProtocolChange}
        recommended={plan?.recommended}
        availableProtocols={displayProtocols}
      />
      <MachineCanvas vmState={vmState} healthScore={healthScore} className="flex-1">
        {loading ? (
          <div className="flex items-center justify-center flex-1 text-[var(--text-muted)] text-sm">Loading machine canvas…</div>
        ) : (
          <>
            <div className="flex-1 min-h-0 w-full flex flex-col relative z-0">{canvasContent}</div>
            {lens === 'display' ? (
              <>
                <FloatingConsoleHud visible={!loading} />
                <CommandDock
                  visible={!loading}
                  onCtrlAltDel={() => void sendCtrlAltDel()}
                  onExplain={() => switchLens('ai')}
                  onSwitchLens={(l) => switchLens(l as ConsoleLens)}
                  availableProtocols={displayProtocols}
                />
                <ConsoleMinimap />
              </>
            ) : null}
          </>
        )}
      </MachineCanvas>
      {opsShelf}
    </div>
  )
}

export default function MachineCockpit(props: MachineCockpitProps) {
  return (
    <ConsoleViewportProvider>
      <ConsoleClipboardProvider>
        <CockpitInner {...props} />
      </ConsoleClipboardProvider>
    </ConsoleViewportProvider>
  )
}
