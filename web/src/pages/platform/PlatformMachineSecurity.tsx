// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { usePlatformTabState } from '../../hooks/usePlatformTabState'
import PageLayout from '../../components/PageLayout'
import { Link, useParams } from 'react-router'
import { ArrowLeft, Shield } from 'lucide-react'
import {
  MacGlassPanel,
  MacListRow,
} from '../../components/platform/mac/PlatformMacUi'
import PageSkeleton from '../../components/PageSkeleton'
import ProcessGraphCanvas from '../../components/platform/ProcessGraphCanvas'
import ContainerHierarchyPanel from '../../components/platform/ContainerHierarchyPanel'
import JsonInspector from '../../components/platform/JsonInspector'
import {
  explainSecurityEvent,
  getHostConnections,
  getHostContainers,
  getHostDns,
  getHostProcessGraph,
  getHostProcesses,
  getHostSecurityFiles,
  getHostSecurityPorts,
  getHostSecuritySummary,
  getHostSecurityTimeline,
  getHostFabricStatus,
  getHostEnforcement,
  getEnforcementPolicies,
  applyEnforcementPolicy,
  type EnforcementPolicy,
  type HostFabricStatusResponse,
  reconstructAttack,
  type SecurityEvent,
} from '../../api/zeusSecurity'
import { formatUserError } from '../../utils/apiError'
import { useToastContext } from '../../contexts/ToastContext'
import { useBreadcrumbName } from '../../contexts/BreadcrumbNameContext'
import AskZyraButton from '../../components/ai/AskZyraButton'
import DetailTabs from '../../components/platform/DetailTabs'
import { statusPillClasses, hubLinkClasses } from '../../utils/semanticColors'
import EbpfActionMenu from '../../components/platform/EbpfActionMenu'

const MACHINE_SECURITY_TABS = ['processes', 'connections', 'dns', 'ports', 'files', 'events', 'containers', 'users', 'graph', 'enforcement'] as const
type TabId = (typeof MACHINE_SECURITY_TABS)[number]

const PRIMARY_TABS: Array<{ id: TabId; label: string }> = [
  { id: 'processes', label: 'Processes' },
  { id: 'connections', label: 'Connections' },
  { id: 'events', label: 'Events' },
  { id: 'graph', label: 'Graph' },
]

const MORE_TABS: Array<{ id: TabId; label: string; group?: string }> = [
  { id: 'dns', label: 'DNS', group: 'Network' },
  { id: 'ports', label: 'Open ports', group: 'Network' },
  { id: 'files', label: 'Files', group: 'Artifacts' },
  { id: 'containers', label: 'Containers', group: 'Runtime' },
  { id: 'users', label: 'Users', group: 'Runtime' },
  { id: 'enforcement', label: 'Enforcement', group: 'Runtime' },
]

function eventRow(e: SecurityEvent) {
  return e.summary || e.process?.binary || e.kind || 'event'
}

function eventSub(e: SecurityEvent) {
  const parts = [e.process?.user, e.process?.binary, e.timestamp].filter(Boolean)
  return parts.join(' · ')
}

export default function PlatformMachineSecurity() {
  const { hostId } = useParams<{ hostId: string }>()
  const toast = useToastContext()
  const [tab, setTab] = usePlatformTabState(MACHINE_SECURITY_TABS, { defaultTab: 'processes' })
  const [summary, setSummary] = useState<Record<string, unknown> | null>(null)
  const [items, setItems] = useState<SecurityEvent[]>([])
  const [ports, setPorts] = useState<Array<Record<string, unknown>>>([])
  const [graph, setGraph] = useState<Record<string, unknown> | null>(null)
  const [containers, setContainers] = useState<Record<string, unknown> | null>(null)
  const [timeline, setTimeline] = useState<SecurityEvent[]>([])
  const [attackChain, setAttackChain] = useState<string[] | null>(null)
  const [fabricStatus, setFabricStatus] = useState<HostFabricStatusResponse | null>(null)
  const [hostEnforcement, setHostEnforcement] = useState<Record<string, unknown> | null>(null)
  const [enforcementPolicies, setEnforcementPolicies] = useState<EnforcementPolicy[]>([])
  const [explain, setExplain] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    if (!hostId) return
    // Last-response-wins: rapid navigation between hosts/tabs can leave stale
    // awaits in flight; only the newest load may commit state so one host's
    // data can't interleave into another's.
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setError(null)
    setLoading(true)
    // Clear the shared per-tab collection so the previous tab's rows don't
    // render under the new tab's header while its fetch is in flight.
    setItems([])
    try {
      const sum = await getHostSecuritySummary(hostId)
      if (!alive()) return
      setSummary(sum)
      void getHostFabricStatus(hostId)
        .then((s) => { if (alive()) setFabricStatus(s) })
        .catch(() => { if (alive()) setFabricStatus(null) })
      if (tab === 'processes') {
        const r = await getHostProcesses(hostId)
        if (alive()) setItems(r.processes ?? [])
      } else if (tab === 'connections') {
        const r = await getHostConnections(hostId)
        if (alive()) setItems(r.connections ?? [])
      } else if (tab === 'dns') {
        const r = await getHostDns(hostId)
        if (alive()) setItems(r.dns ?? [])
      } else if (tab === 'files') {
        const r = await getHostSecurityFiles(hostId)
        if (alive()) setItems(r.files ?? [])
      } else if (tab === 'ports') {
        const r = await getHostSecurityPorts(hostId)
        if (alive()) {
          setPorts(r.ports ?? [])
          setItems([])
        }
      } else if (tab === 'events') {
        const r = await getHostSecurityTimeline(hostId)
        if (alive()) {
          setTimeline(r.events ?? [])
          setItems(r.events ?? [])
        }
      } else if (tab === 'containers') {
        const r = await getHostContainers(hostId)
        if (alive()) {
          setContainers(r as Record<string, unknown>)
          setItems([])
        }
      } else if (tab === 'enforcement') {
        const [enf, pol] = await Promise.all([
          getHostEnforcement(hostId).catch(() => null),
          getEnforcementPolicies().catch(() => ({ policies: [] as EnforcementPolicy[] })),
        ])
        if (alive()) {
          setHostEnforcement(enf)
          setEnforcementPolicies(pol.policies ?? [])
          setItems([])
        }
      } else {
        if (alive()) setItems([])
      }
    } catch (e: unknown) {
      if (alive()) setError(formatUserError(e))
    } finally {
      if (alive()) setLoading(false)
    }
  }, [hostId, tab])

  useEffect(() => { void load() }, [hostId, tab])

  useEffect(() => {
    if (tab === 'graph' && hostId) {
      void getHostProcessGraph(hostId).then(setGraph).catch(() => setGraph(null))
    }
  }, [tab, hostId])

  useBreadcrumbName((summary?.hostname as string) ?? '')

  if (!hostId) return null

  const sensor = (summary?.sensor as Record<string, unknown>) ?? {}
  const threatScore = summary?.threat_score ?? '—'
  const fabric = fabricStatus?.fabric
  const fabricLine = fabricStatus?.agent_reachable && fabric?.available
    ? `machina-bpfd ${fabric.version ?? ''} · ${fabric.programs_compiled ? 'datapath loaded' : 'datapath not compiled'} · ${
        fabric.mode?.mode === 'enforce' ? 'enforcing' : 'observing'
      } · ${fabric.policies_enabled ?? 0} policy(ies) · ${fabric.interfaces?.length ?? 0} interface(s)`
    : 'Agent fabric status unavailable'

  const threatTone = typeof threatScore === 'number' && threatScore > 70 ? 'error' : typeof threatScore === 'number' && threatScore > 40 ? 'warn' : 'ok'

  return (
    <PageLayout
      compact
      contentLoading={loading && !summary}
      error={error}
      prepend={
        <Link to="/platform/zeus/security" className={`text-sm inline-flex items-center gap-1 min-h-9 ${hubLinkClasses()}`}>
          <ArrowLeft className="w-4 h-4" /> Security Center
        </Link>
      }
      title={`Machine security`}
      subtitle={
        <span className="flex flex-wrap items-center gap-2 text-sm">
          <span className={statusPillClasses(threatTone)}>Threat {String(threatScore)}</span>
          <span className="text-[var(--text-muted)]">Sensor {String(sensor.status ?? 'unknown')}</span>
          <span className="text-[var(--text-muted)] text-xs">{fabricLine}</span>
        </span>
      }
      actions={
        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            className="btn-secondary text-sm"
            onClick={() => void reconstructAttack(hostId).then((r) => setAttackChain(r.attack_chain)).catch((e: unknown) => toast.error(formatUserError(e)))}
          >
            Attack chain
          </button>
          <AskZyraButton />
        </div>
      }
      contentClassName="space-y-4"
    >
      {loading && !summary && <PageSkeleton />}

      <DetailTabs primary={PRIMARY_TABS} more={MORE_TABS} active={tab} onChange={setTab} />

      <MacGlassPanel title={PRIMARY_TABS.find((t) => t.id === tab)?.label ?? MORE_TABS.find((t) => t.id === tab)?.label ?? 'Security'}>
        {tab === 'ports' ? (
          ports.length === 0 ? (
            <p className="text-sm text-[var(--text-muted)] p-3">No open ports reported.</p>
          ) : (
            ports.map((p) => (
              <MacListRow
                key={`${p.port}-${p.protocol}`}
                title={`${p.port} ${p.service ?? p.protocol}`}
                subtitle={[p.process, p.user, p.bind].filter(Boolean).map(String).join(' · ')}
              />
            ))
          )
        ) : tab === 'graph' ? (
          <MacGlassPanel title="Process ancestry" subtitle="Native eBPF exec / fork tracepoints">
            <ProcessGraphCanvas data={graph as Parameters<typeof ProcessGraphCanvas>[0]['data']} />
          </MacGlassPanel>
        ) : tab === 'containers' ? (
          <ContainerHierarchyPanel data={containers as Parameters<typeof ContainerHierarchyPanel>[0]['data']} />
        ) : tab === 'users' ? (
          <p className="text-sm text-[var(--text-muted)] p-3">
            User session events correlate from process exec and privilege escalation timelines.
          </p>
        ) : tab === 'enforcement' ? (
          <div className="p-3 space-y-3 text-sm">
            {hostEnforcement ? (
              <>
                <p className="text-[var(--text-secondary)]">
                  Mode: <span className="font-mono">{String(hostEnforcement.mode ?? 'observe')}</span>
                  {' · '}
                  {String(hostEnforcement.summary ?? 'Per-host native eBPF enforcement posture')}
                </p>
                {Array.isArray(hostEnforcement.policies) && (hostEnforcement.policies as unknown[]).length > 0 ? (
                  <ul className="divide-y divide-white/[0.04] -mx-1">
                    {(hostEnforcement.policies as Array<Record<string, unknown>>).map((p, i) => (
                      <MacListRow
                        key={String(p.id ?? i)}
                        title={String(p.name ?? p.id ?? 'policy')}
                        subtitle={[p.kind, p.match].filter(Boolean).map(String).join(' · ')}
                      />
                    ))}
                  </ul>
                ) : (
                  <p className="text-[var(--text-muted)]">No enforcement policies applied on this host yet.</p>
                )}
              </>
            ) : (
              <p className="text-[var(--text-muted)]">Enforcement status unavailable — ensure machina-bpfd is running on this host.</p>
            )}
            {enforcementPolicies.length > 0 && (
              <div className="pt-2 border-t border-white/[0.06] space-y-2">
                <p className="text-xs text-[var(--text-muted)] uppercase tracking-wider">Fleet policies</p>
                {enforcementPolicies.slice(0, 5).map((p) => (
                  <div key={p.id ?? p.name} className="flex flex-wrap items-center justify-between gap-2">
                    <span className="text-[var(--text-secondary)]">{p.name}</span>
                    <button
                      type="button"
                      className="btn-secondary text-xs"
                      disabled={!p.id}
                      onClick={() => {
                        if (!p.id || !hostId) return
                        void applyEnforcementPolicy(p.id, [hostId])
                          .then((r) => toast.success(r.summary || 'Policy applied'))
                          .catch((e: unknown) => toast.error(formatUserError(e)))
                      }}
                    >
                      Apply here
                    </button>
                  </div>
                ))}
                <Link to="/platform/zyra/security/enforcement" className={`text-xs inline-flex items-center gap-1 ${hubLinkClasses()}`}>
                  <Shield className="w-3 h-3" /> Runtime enforcement workspace
                </Link>
              </div>
            )}
          </div>
        ) : items.length === 0 ? (
          <p className="text-sm text-[var(--text-muted)] p-3">No events in this category yet. machina-bpfd records them once its datapath is loaded.</p>
        ) : (
          <>
            {items.slice(0, 30).map((e, i) => {
              const isDns = e.kind === 'dns_query' || Boolean(e.dns?.query)
              const isProcess = Boolean(e.process?.binary)
              return (
                <MacListRow
                  key={e.id ?? i}
                  title={eventRow(e)}
                  subtitle={eventSub(e)}
                  onClick={() => void explainSecurityEvent(e, hostId).then((r) => setExplain(r.explanation)).catch(() => setExplain(null))}
                  trailing={
                    (isDns || isProcess) ? (
                      <EbpfActionMenu
                        hostId={hostId}
                        suggestedKind={isDns ? 'deny_dns' : 'deny_process'}
                        suggestedMatch={isDns ? String(e.dns?.query ?? '*.xyz') : String(e.process?.binary ?? '')}
                        policyName={eventRow(e)}
                        compact
                      />
                    ) : undefined
                  }
                />
              )
            })}
            {explain && (
              <div className="p-3 text-sm text-[var(--text-secondary)] border-t border-white/[0.06]">{explain}</div>
            )}
            <JsonInspector data={items.slice(0, 5)} />
          </>
        )}
      </MacGlassPanel>

      {attackChain && attackChain.length > 0 && (
        <MacGlassPanel title="Attack reconstruction" subtitle="AI timeline analysis">
          <ol className="list-decimal pl-5 text-sm text-[var(--text-secondary)] space-y-1">
            {attackChain.map((s) => (
              <li key={s}>{s}</li>
            ))}
          </ol>
        </MacGlassPanel>
      )}

      {tab === 'events' && timeline.length > 0 && (
        <MacGlassPanel title="Security timeline" subtitle="Flight recorder">
          {timeline.slice(0, 15).map((e, i) => (
            <MacListRow key={String(e.timestamp ?? i)} title={eventRow(e)} subtitle={String(e.timestamp ?? '')} />
          ))}
        </MacGlassPanel>
      )}
    </PageLayout>
  )
}
