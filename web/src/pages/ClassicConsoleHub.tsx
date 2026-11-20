// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Link, useParams, useSearchParams } from 'react-router'
import { hubLinkClasses } from '../utils/semanticColors'
import { ArrowLeft } from 'lucide-react'
import {
  createClassicConsoleHubSession,
  getClassicConsoleHubPlan,
  getVM,
  listClassicConsoleHubSessions,
  vmDetailRoute,
  type ClassicConsoleHubPlan,
  type ClassicConsoleHubSessionResponse,
} from '../api/vm'
import type { ConsoleHubPlan, ConsoleHubSessionResponse } from '../api/platform'
import { listPlatformVms } from '../api/platform'
import { getWsToken } from '../api/client'
import { formatUserError } from '../utils/apiError'
import MachineCockpit from '../components/consolehub/MachineCockpit'
import type { ConsoleHubSessionRow } from '../components/consolehub/ConsoleHubSessionHistory'
import PageLayout from '../components/PageLayout'
import { getDefaultProtocol, parseConsoleMode, type ConsoleExperienceMode } from '../utils/consoleExperienceMode'

function classicWsUrl(pathTemplate: string, token: string): string | null {
  const path = pathTemplate.replace('__WS_TOKEN__', encodeURIComponent(token))
  const protocol = typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  const host = typeof window !== 'undefined' ? window.location.host : ''
  return `${protocol}//${host}${path}`
}

function classicVncWsUrl(plan: ClassicConsoleHubPlan, token: string): string | null {
  if (!plan.native.available) return null
  return classicWsUrl(plan.native.ws_path, token)
}

function classicSerialWsUrl(plan: ClassicConsoleHubPlan, token: string): string | null {
  const path = plan.native.serial_ws_path ?? plan.native.ws_path.replace('/vnc/', '/console/')
  return classicWsUrl(path, token)
}

function toPlatformPlan(plan: ClassicConsoleHubPlan, platformVmId: string | null): ConsoleHubPlan {
  // MachineCockpit / hardware / power APIs expect the controller UUID, not the libvirt name.
  return { ...plan, vm_id: platformVmId ?? plan.vm_name }
}

function toPlatformSession(sess: ClassicConsoleHubSessionResponse): ConsoleHubSessionResponse {
  return { ...sess, vm_id: sess.vm_name }
}

// Adapts a break-glass response (platform-shaped, keyed by vm_id) back into
// this page's ClassicConsoleHubSessionResponse (keyed by vm_name) so it can
// be lifted into local `session` state.
function fromPlatformSession(sess: ConsoleHubSessionResponse, vmName: string): ClassicConsoleHubSessionResponse {
  const { vm_id: _vm_id, ...rest } = sess
  return { ...rest, vm_name: vmName }
}

export default function ClassicConsoleHub() {
  const { name } = useParams<{ name: string }>()
  const [searchParams, setSearchParams] = useSearchParams()
  const conn = searchParams.get('connection') ?? undefined
  const experienceMode = parseConsoleMode(searchParams.toString())
  const [plan, setPlan] = useState<ClassicConsoleHubPlan | null>(null)
  const [session, setSession] = useState<ClassicConsoleHubSessionResponse | null>(null)
  const [activeProtocol, setActiveProtocol] = useState('novnc')
  const [vmState, setVmState] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [connectKey, setConnectKey] = useState(0)
  const [history, setHistory] = useState<ConsoleHubSessionRow[]>([])
  const [wsUrl, setWsUrl] = useState<string | null>(null)
  const [serialWsUrl, setSerialWsUrl] = useState<string | null>(null)
  const [platformVmId, setPlatformVmId] = useState<string | null>(null)
  const [platformHostId, setPlatformHostId] = useState<string | null>(null)

  const platformPlan = useMemo(
    () => (plan ? toPlatformPlan(plan, platformVmId) : null),
    [plan, platformVmId],
  )
  const platformSession = useMemo(() => (session ? toPlatformSession(session) : null), [session])
  // Last-response-wins: only the newest load may commit console state, so a
  // slow fetch for a previously-viewed VM can't wire up a WS URL/token that
  // point at the wrong VM's console after the user has navigated away.
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    if (!name) return
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setLoading(true)
    setError(null)
    try {
      const [hubPlan, vm, sessions, platformVms] = await Promise.all([
        getClassicConsoleHubPlan(name, conn),
        getVM(name, conn).catch(() => null),
        listClassicConsoleHubSessions(name, conn).catch(() => []),
        listPlatformVms().catch(() => []),
      ])
      if (!alive()) return
      const linked = Array.isArray(platformVms)
        ? platformVms.find((v) => v.name === name) ?? null
        : null
      setPlatformVmId(linked?.id ?? null)
      setPlatformHostId(linked?.host_id ?? null)
      setPlan(hubPlan)
      // Cockpit pattern: pick protocol from VM capabilities, not backend hint
      const defaultProto = getDefaultProtocol(hubPlan)
      setActiveProtocol(defaultProto)
      setVmState(vm?.state ?? null)
      setHistory(sessions)
      setSession(null)
      const token = await getWsToken()
      if (!alive()) return
      setWsUrl(classicVncWsUrl(hubPlan, token))
      setSerialWsUrl(classicSerialWsUrl(hubPlan, token))
    } catch (e: unknown) {
      if (!alive()) return
      // Clear the previous VM's console artifacts so a failed load shows the
      // "unavailable" state instead of the prior VM's still-connected session.
      setPlan(null)
      setSession(null)
      setWsUrl(null)
      setSerialWsUrl(null)
      setPlatformVmId(null)
      setPlatformHostId(null)
      setError(formatUserError(e))
    } finally {
      if (alive()) setLoading(false)
    }
  }, [name, conn])

  useEffect(() => {
    void load()
  }, [load, connectKey])

  const switchProtocol = async (protocol: string) => {
    if (!name) return
    setActiveProtocol(protocol)
    setError(null)
    try {
      {
        setSession(null)
        if (protocol === 'novnc' && plan) {
          const token = await getWsToken()
          setWsUrl(classicVncWsUrl(plan, token))
        } else if (protocol === 'serial' && plan) {
          const token = await getWsToken()
          setSerialWsUrl(classicSerialWsUrl(plan, token))
        }
      }
      setHistory(await listClassicConsoleHubSessions(name, conn).catch(() => []))
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }

  const setExperienceMode = useCallback(
    (mode: ConsoleExperienceMode) => {
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev)
          if (mode === 'cinema') next.delete('mode')
          else next.set('mode', mode)
          return next
        },
        { replace: true },
      )
    },
    [setSearchParams],
  )

  const prepend = (
    <div className="flex flex-wrap items-center gap-3 text-sm">
      <Link to={vmDetailRoute(name ?? '', conn)} className={`inline-flex items-center gap-1 ${hubLinkClasses()}`}>
        <ArrowLeft className="w-4 h-4" /> Back to VM
      </Link>
    </div>
  )

  if (!name) return null

  return (
    <PageLayout
      compact
      hideHeader
      loading={loading && !plan}
      contentClassName="flex flex-col flex-1 min-h-0 h-full min-h-[calc(100dvh-14rem)]"
      title={name}
      subtitle="Classic ConsoleHub · Machine Canvas"
    >
      <h1 className="sr-only">{name} console</h1>
      {plan ? (
        <div className="flex flex-col flex-1 min-h-0 h-full">
          <MachineCockpit
            vmId={platformVmId ?? name}
            vmName={name}
            plan={platformPlan}
            session={platformSession}
            wsUrl={wsUrl}
            serialWsUrl={serialWsUrl}
            activeProtocol={activeProtocol}
            onProtocolChange={(p) => void switchProtocol(p)}
            vmState={vmState}
            error={error}
            loading={loading}
            history={history}
            onReconnect={() => setConnectKey((k) => k + 1)}
            connectKey={connectKey}
            prepend={prepend}
            experienceMode={experienceMode}
            onExperienceModeChange={setExperienceMode}
            onSessionStart={(s) => setSession(fromPlatformSession(s, name ?? ''))}
            hostId={platformHostId}
            inventorySource="libvirt"
          />
        </div>
      ) : !loading ? (
        <section className="rounded-2xl border border-amber-500/30 bg-amber-500/10 p-8 max-w-lg mx-auto text-center space-y-4">
          <h2 className="text-lg font-semibold text-amber-800">Console unavailable</h2>
          <p className="text-sm text-amber-700/80">{error ?? 'Could not load console plan for this VM.'}</p>
        </section>
      ) : null}
    </PageLayout>
  )
}
