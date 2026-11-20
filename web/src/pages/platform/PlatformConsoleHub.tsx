// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useLocation, useNavigate, useParams } from 'react-router'
import {
  createConsoleHubSession,
  getConsoleHubPlan,
  getPlatformVm,
  issuePlatformVmWsToken,
  listConsoleHubSessions,
  listVmPortForwards,
  listVmTimeline,
  platformVmVncWsUrl,
  platformVmSerialWsUrl,
  platformVmSpiceWsPath,
  platformVncWsUrl,
  requestConsoleAccess,
  runVmHealthCheck,
  type ConsoleHubPlan,
  type ConsoleHubSessionResponse,
  type VmPortForwardRule,
} from '../../api/platform'
import { formatUserError } from '../../utils/apiError'
import { useToastContext } from '../../contexts/ToastContext'
import MachineCockpit from '../../components/consolehub/MachineCockpit'
import type { ConsoleHubSessionRow } from '../../components/consolehub/ConsoleHubSessionHistory'
import { isCenterPopoutMode, openCenterPopout } from '../../utils/platformCenterPopout'
import {
  getDefaultProtocol,
  parseConsoleMode,
  cinemaPopoutPath,
  resolveConsoleMode,
  saveConsoleModePreference,
  type ConsoleExperienceMode,
} from '../../utils/consoleExperienceMode'
import { usePlatformMacDesktop } from '../../components/platform/mac/PlatformMacDesktopContext'
import { useBreadcrumbName } from '../../contexts/BreadcrumbNameContext'

export default function PlatformConsoleHub() {
  const toast = useToastContext()
  const navigate = useNavigate()
  const location = useLocation()
  const { id } = useParams<{ id: string }>()
  const protocolFromUrl = new URLSearchParams(location.search).get('protocol')
  const isPopout = isCenterPopoutMode(location.search)
  const experienceMode = resolveConsoleMode(location.search, id)
  const { setCinemaChromeHidden } = usePlatformMacDesktop()
  const [plan, setPlan] = useState<ConsoleHubPlan | null>(null)
  const [session, setSession] = useState<ConsoleHubSessionResponse | null>(null)
  const [activeProtocol, setActiveProtocol] = useState('novnc')
  const [wsUrl, setWsUrl] = useState<string | null>(null)
  const [serialWsUrl, setSerialWsUrl] = useState<string | null>(null)
  const [platformSpiceWsPath, setPlatformSpiceWsPath] = useState<string | null>(null)
  const [vmName, setVmName] = useState<string | null>(null)
  useBreadcrumbName(vmName ?? '')
  const [vmState, setVmState] = useState<string | null>(null)
  const [nodeName, setNodeName] = useState<string | null>(null)
  const [healthScore, setHealthScore] = useState<number | null>(null)
  const [kubeVirtNamespace, setKubeVirtNamespace] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [connectKey, setConnectKey] = useState(0)
  const [history, setHistory] = useState<ConsoleHubSessionRow[]>([])
  const [machineTimeline, setMachineTimeline] = useState<Awaited<ReturnType<typeof listVmTimeline>>>([])
  const [portForwardRules, setPortForwardRules] = useState<VmPortForwardRule[]>([])
  const [inventorySource, setInventorySource] = useState<string | null>(null)
  const [hostId, setHostId] = useState<string | null>(null)
  // Last-response-wins: only the newest load may commit console state, so a
  // slow fetch for a previously-viewed VM can't wire up a WS URL/token that
  // point at the wrong VM's console after the user has navigated away.
  const loadSeq = useRef(0)

  const cinemaChrome = experienceMode === 'cinema' && !isPopout

  useEffect(() => {
    setCinemaChromeHidden(cinemaChrome)
    return () => {
      setCinemaChromeHidden(false)
    }
  }, [cinemaChrome, setCinemaChromeHidden])

  useEffect(() => {
    if (!id) return
    const params = new URLSearchParams(location.search)
    if (params.has('mode')) return
    const saved = resolveConsoleMode(location.search, id)
    if (saved !== 'cinema') {
      params.set('mode', saved)
      navigate(`/platform/vms/${id}/consolehub?${params.toString()}`, { replace: true })
    }
  }, [id, location.search, navigate])

  const setExperienceMode = useCallback(
    (mode: ConsoleExperienceMode) => {
      if (!id) return
      saveConsoleModePreference(id, mode)
      const params = new URLSearchParams(location.search)
      if (mode === 'cinema') params.delete('mode')
      else params.set('mode', mode)
      navigate(`/platform/vms/${id}/consolehub?${params.toString()}`, { replace: true })
    },
    [id, location.search, navigate],
  )

  const load = useCallback(async () => {
    if (!id) return
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setLoading(true)
    setError(null)
    try {
      const [planRes, tokenRes, vm, sessions, timeline, health] = await Promise.all([
        getConsoleHubPlan(id).then((v) => ({ ok: true as const, v })).catch((e: unknown) => ({ ok: false as const, e })),
        issuePlatformVmWsToken(id).then((v) => ({ ok: true as const, v })).catch((e: unknown) => ({ ok: false as const, e })),
        getPlatformVm(id).catch(() => null),
        listConsoleHubSessions(id).catch(() => []),
        listVmTimeline(id).catch(() => []),
        runVmHealthCheck(id).catch(() => null),
      ])
      // A newer id was selected while this fetch was in flight — bail before
      // committing any state so a slow prior-VM response can't wire the
      // console viewer up to the wrong VM's WS URL/token.
      if (!alive()) return

      const hubPlan = planRes.ok ? planRes.v : null
      const wsToken = tokenRes.ok ? tokenRes.v.token : null

      if (!hubPlan && !wsToken) {
        const err = !planRes.ok ? planRes.e : !tokenRes.ok ? tokenRes.e : null
        setError(formatUserError(err))
        return
      }

      if (hubPlan) {
        setPlan(hubPlan)
        setVmName(hubPlan.vm_name)
        // Cockpit pattern: prefer VNC → SPICE → serial based on VM capabilities, not backend hint
        const preferred = protocolFromUrl && hubPlan.protocols.includes(protocolFromUrl)
          ? protocolFromUrl
          : getDefaultProtocol(hubPlan)
        setActiveProtocol(preferred)
        if (hubPlan.guest_ip?.trim()) {
          listVmPortForwards(id)
            .then((rules) => { if (alive()) setPortForwardRules(rules) })
            .catch(() => { if (alive()) setPortForwardRules([]) })
        } else {
          setPortForwardRules([])
        }
      } else if (vm?.name) {
        setVmName(vm.name)
      }

      if (!planRes.ok || !tokenRes.ok) {
        const partialErr = formatUserError(
          !planRes.ok ? planRes.e : !tokenRes.ok ? tokenRes.e : null,
        )
        const canDisplay = Boolean(
          wsToken
          || hubPlan?.native?.ws_path
          || vm?.inventory_source === 'kubevirt',
        )
        if (!canDisplay) setError(partialErr)
      }

      setHistory(sessions)
      setMachineTimeline(timeline)
      if (health) {
        const parts = (health.score ?? '0').split('/')
        const n = Number(parts[0])
        setHealthScore(Number.isFinite(n) ? n : null)
      } else {
        setHealthScore(null)
      }
      setVmState(vm?.observed_state ?? vm?.desired_state ?? null)
      setInventorySource(vm?.inventory_source ?? 'libvirt')
      setHostId(vm?.host_id ?? null)
      setNodeName(vm?.host_id ? vm.host_id.slice(0, 8) : null)
      const isKubevirt = vm?.inventory_source === 'kubevirt'
      setKubeVirtNamespace(isKubevirt ? (vm?.k8s_namespace ?? 'default') : null)
      if (isKubevirt) {
        setWsUrl(hubPlan?.native?.ws_path ? platformVncWsUrl(hubPlan.native.ws_path) : null)
        setSerialWsUrl(hubPlan?.native?.serial_ws_path ? platformVncWsUrl(hubPlan.native.serial_ws_path) : null)
        setPlatformSpiceWsPath(null)
        if (hubPlan) {
          setActiveProtocol(getDefaultProtocol(hubPlan))
        }
      } else if (wsToken) {
        setWsUrl(platformVmVncWsUrl(id, wsToken))
        setSerialWsUrl(platformVmSerialWsUrl(id, wsToken))
        setPlatformSpiceWsPath(platformVmSpiceWsPath(id, wsToken))
      } else if (hubPlan?.native?.ws_path) {
        const rawPath = hubPlan.native.ws_path
        const isSpicePath =
          hubPlan.native.console_type === 'spice' || rawPath.includes('/spice/')
        if (isSpicePath) {
          setPlatformSpiceWsPath(rawPath.replace(/^\//, ''))
          setWsUrl(null)
        } else {
          setWsUrl(platformVncWsUrl(rawPath))
          setPlatformSpiceWsPath(null)
        }
      }

      if (wsToken || hubPlan?.native?.ws_path) {
        setError(null)
      }

      setSession(null)
    } catch (e: unknown) {
      if (alive()) setError(formatUserError(e))
    } finally {
      if (alive()) setLoading(false)
    }
  }, [id, protocolFromUrl])

  // When the plan first loads and the VM has no display device, exit Cinema mode (requires a display).
  // Runs once per plan load — does NOT re-run when the user manually changes experienceMode.
  useEffect(() => {
    if (!plan) return
    if (getDefaultProtocol(plan) === 'serial' && experienceMode === 'cinema') {
      setExperienceMode('studio')
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [plan])

  useEffect(() => {
    setWsUrl(null)
    setSerialWsUrl(null)
    setPlatformSpiceWsPath(null)
    setSession(null)
    setPlan(null)
    setVmName(null)
    setError(null)
    setHistory([])
  }, [id])

  useEffect(() => {
    void load()
  }, [load, connectKey])

  const switchProtocol = async (protocol: string) => {
    if (!id) return
    setActiveProtocol(protocol)
    setError(null)
    try {
      {
        setSession(null)
        if (protocol === 'novnc' && !kubeVirtNamespace) {
          const tokenRes = await issuePlatformVmWsToken(id)
          setWsUrl(platformVmVncWsUrl(id, tokenRes.token))
        } else if ((protocol === 'spice' || protocol === 'webrtc_spice') && !kubeVirtNamespace) {
          const tokenRes = await issuePlatformVmWsToken(id)
          setPlatformSpiceWsPath(platformVmSpiceWsPath(id, tokenRes.token))
        } else if (protocol === 'serial' && !kubeVirtNamespace) {
          const tokenRes = await issuePlatformVmWsToken(id)
          setSerialWsUrl(platformVmSerialWsUrl(id, tokenRes.token))
        } else if (protocol === 'serial' && kubeVirtNamespace) {
          setSerialWsUrl(null)
        }
      }
      setHistory(await listConsoleHubSessions(id).catch(() => []))
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }

  const requestAccess = async () => {
    if (!id || !activeProtocol) return
    try {
      await requestConsoleAccess(id, { protocol: activeProtocol, reason: 'ConsoleHub operator access' })
      toast.success('JIT console access request submitted — check Zyra → Approvals')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  if (!id) return null

  if (!loading && !vmName) {
    return (
      <section className="rounded-2xl border border-amber-500/30 bg-amber-500/10 p-8 max-w-lg mx-auto text-center space-y-4" data-testid="consolehub-recovery">
        <h2 className="text-lg font-semibold text-amber-800">Machine not found</h2>
        <p className="text-sm text-amber-700/80">This VM does not exist or the session expired.</p>
        <div className="flex flex-wrap justify-center gap-2">
          <Link to="/platform" className="btn-primary text-sm">Mission Control</Link>
          <Link to="/platform/vms" className="btn-secondary text-sm">Machine Finder</Link>
        </div>
      </section>
    )
  }

  if (!vmName) {
    return <div className="flex items-center justify-center flex-1 text-[var(--text-muted)] text-sm p-8">Loading…</div>
  }

  return (
    <div
      className={
        cinemaChrome
          ? 'fixed inset-0 z-[50] flex flex-col min-h-0 h-dvh w-full'
          : 'flex flex-col flex-1 min-h-0 h-full'
      }
      data-cinema-route={cinemaChrome ? 'true' : undefined}
    >
      {!cinemaChrome && !isPopout && error?.toLowerCase().includes('approval') ? (
        <div className="mb-2 px-1">
          <button type="button" className="btn-secondary text-sm" onClick={() => void requestAccess()}>
            Request console access
          </button>
        </div>
      ) : null}
      <MachineCockpit
        vmId={id}
        vmName={vmName}
        plan={plan}
        session={session}
        wsUrl={wsUrl}
        serialWsUrl={serialWsUrl}
        platformSpiceWsPath={platformSpiceWsPath}
        activeProtocol={activeProtocol}
        onProtocolChange={(p) => void switchProtocol(p)}
        vmState={vmState}
        nodeName={nodeName}
        healthScore={healthScore}
        kubeVirtNamespace={kubeVirtNamespace}
        error={error}
        loading={loading}
        history={history}
        machineTimeline={machineTimeline}
        isPopout={isPopout}
        onPopout={id && !isPopout ? () => openCenterPopout(cinemaPopoutPath(id)) : undefined}
        onReconnect={() => setConnectKey((k) => k + 1)}
        connectKey={connectKey}
        hypervisorAddress={plan?.hypervisor_address ?? undefined}
        portForwardRules={portForwardRules}
        inventorySource={inventorySource}
        hostId={hostId}
        onPlanRefresh={() => void load()}
        experienceMode={experienceMode}
        onExperienceModeChange={setExperienceMode}
        onSessionStart={setSession}
      />
    </div>
  )
}
