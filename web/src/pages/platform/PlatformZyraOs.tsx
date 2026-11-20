// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import ConfirmDialog from '../../components/ConfirmDialog'
import { Link } from 'react-router'
import { useToastContext } from '../../contexts/ToastContext'
import { usePlatformDesktopTier } from '../../hooks/usePlatformDesktopTier'
import { toastQueuedOperation } from '../../utils/platformTaskToast'
import { tasksHubHref } from '../../utils/platformHubLinks'
import { Cpu, Search, Server } from 'lucide-react'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import DetailTabs from '../../components/platform/DetailTabs'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import PageSkeleton from '../../components/PageSkeleton'
import { usePlatformTabState } from '../../hooks/usePlatformTabState'
import PlatformZyraHubLaunchpad from '../../components/platform/tahoe/PlatformZyraHubLaunchpad'
import MachinaInfraGraphBrain from '../../components/ai/MachinaInfraGraphBrain'
import MachinaInfrastructureMemory from '../../components/ai/MachinaInfrastructureMemory'
import ZyraAutonomousRunPanel from '../../components/ai/ZyraAutonomousRunPanel'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusToneClass } from '../../utils/semanticColors'
import { getFleetLinuxHealth, type FleetLinuxHealthOverview } from '../../api/platform'
import {
  analyzeAttackPath,
  diagnoseKnowledge,
  diagnoseFleet,
  executeFleetRebalance,
  getComplianceFrameworks,
  getGpuPlacement,
  getFleetPowerOptimize,
  getFleetHeatmap,
  getFleetSummary,
  getFleetLocal,
  getZyraSummary,
  getRemediateHub,
  getKnowledgeRunbook,
  simulateServiceImpact,
  getFleetRebalanceProposal,
  getInfrastructureMemory,
  getSecurityGraph,
  getServiceGraph,
  getBaremetalProvision,
  listBaremetalServers,
  planBaremetalCapacity,
  registerBaremetalServer,
  searchKnowledge,
  setBaremetalPower,
  type BaremetalServer,
  type FleetHeatmap,
  type KnowledgeHit,
  type RebalanceProposal,
} from '../../api/ai'

type Tab = 'fleet' | 'security' | 'knowledge' | 'services' | 'baremetal' | 'brain' | 'memory'

const ZEUS_TABS: Array<{ id: Tab; label: string }> = [
  { id: 'fleet', label: 'Fleet' },
  { id: 'brain', label: 'Graph Brain' },
  { id: 'memory', label: 'Memory' },
  { id: 'security', label: 'Security' },
  { id: 'knowledge', label: 'Knowledge' },
  { id: 'services', label: 'Services' },
  { id: 'baremetal', label: 'Bare Metal' },
]

export default function PlatformZyraOs() {
  const toast = useToastContext()
  const [tier] = usePlatformDesktopTier()
  const [tab, setTab] = usePlatformTabState<Tab>(ZEUS_TABS.map((t) => t.id), { defaultTab: 'fleet' })
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [heatmap, setHeatmap] = useState<FleetHeatmap | null>(null)
  const [rebalance, setRebalance] = useState<RebalanceProposal | null>(null)
  const [attackSummary, setAttackSummary] = useState<string | null>(null)
  const [knowledgeQuery, setKnowledgeQuery] = useState('payments')
  const [knowledgeHits, setKnowledgeHits] = useState<KnowledgeHit[]>([])
  const [serviceCount, setServiceCount] = useState(0)
  const [memoryCount, setMemoryCount] = useState(0)
  const [baremetal, setBaremetal] = useState<BaremetalServer[]>([])
  const [bmcHost, setBmcHost] = useState('')
  const [bmcAddr, setBmcAddr] = useState('')
  const [bmcVlan, setBmcVlan] = useState('')
  const [pxeVlan, setPxeVlan] = useState('')
  const [metalProfile, setMetalProfile] = useState('BareMetalBmc')
  const [capacitySummary, setCapacitySummary] = useState<string | null>(null)
  const [rebalancePreview, setRebalancePreview] = useState<string | null>(null)
  const [rebalanceExecuteBusy, setRebalanceExecuteBusy] = useState(false)
  const [confirmRebalanceExecute, setConfirmRebalanceExecute] = useState(false)
  const [rebalanceTaskIds, setRebalanceTaskIds] = useState<string[]>([])
  const [frameworksSummary, setFrameworksSummary] = useState<string | null>(null)
  const [gpuSummary, setGpuSummary] = useState<string | null>(null)
  const [diagnosisSummary, setDiagnosisSummary] = useState<string | null>(null)
  const [serviceImpact, setServiceImpact] = useState<string | null>(null)
  const [powerSummary, setPowerSummary] = useState<string | null>(null)
  const [zeusSummary, setZyraSummary] = useState<string | null>(null)
  const [hubSummary, setHubSummary] = useState<string | null>(null)
  const [hubItems, setHubItems] = useState<Array<{ id: string; source: string; label: string; review: string }>>([])
  const [runbookSummary, setRunbookSummary] = useState<string | null>(null)
  const [linuxHealth, setLinuxHealth] = useState<FleetLinuxHealthOverview | null>(null)
  const [fleetDiagnoseQuery, setFleetDiagnoseQuery] = useState('fleet linux pressure and failed tasks')
  const [fleetDiagnoseSummary, setFleetDiagnoseSummary] = useState<string | null>(null)
  const [fleetSummaryLine, setFleetSummaryLine] = useState<string | null>(null)
  const [securityLoaded, setSecurityLoaded] = useState(false)
  const [servicesLoaded, setServicesLoaded] = useState(false)
  const [baremetalLoaded, setBaremetalLoaded] = useState(false)

  const loadFleet = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      const [h, r, gpu, power, linux, summary, local] = await Promise.all([
        getFleetHeatmap(),
        getFleetRebalanceProposal(),
        getGpuPlacement('inference').catch(() => null),
        getFleetPowerOptimize().catch(() => null),
        getFleetLinuxHealth().catch(() => null),
        getFleetSummary().catch(() => null),
        getFleetLocal().catch(() => null),
      ])
      setHeatmap(h)
      setRebalance(r)
      setGpuSummary(gpu?.summary ?? null)
      setPowerSummary(power?.summary ?? null)
      setLinuxHealth(linux)
      // A summary response with a missing/non-numeric aggregate_monthly_usd (an empty-array
      // fallback from an unmocked/misbehaving endpoint, or a genuinely partial API response)
      // used to throw here and abort the whole load — heatmap/rebalance/etc had already been set
      // above, so the page ended up loaded but showing a cryptic JS TypeError instead of just
      // omitting the one summary line it couldn't build.
      const summaryStr = summary && typeof summary.aggregate_monthly_usd === 'number'
        ? `${summary.aggregate_vm_count} VMs · ${summary.reachable_peers}/${summary.peer_count} peers · $${summary.aggregate_monthly_usd.toFixed(0)}/mo`
        : null
      const localStr = local
        ? `Local: ${local.vm_count ?? 0} VMs · risk ${local.security_risk_level ?? 'unknown'}`
        : null
      setFleetSummaryLine([summaryStr, localStr].filter(Boolean).join(' · ') || null)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  const loadSecurity = useCallback(async () => {
    setError(null)
    setLoading(true)
    setSecurityLoaded(false)
    try {
      await getSecurityGraph()
      const path = await analyzeAttackPath('attacker', 'db-prod')
      setAttackSummary(path.summary)
      const fw = await getComplianceFrameworks()
      setFrameworksSummary(
        (fw.frameworks ?? []).map((f) => `${f.framework} ${f.grade} (${f.score})`).join(' · ') || fw.summary,
      )
      setSecurityLoaded(true)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  const loadServices = useCallback(async () => {
    setError(null)
    setLoading(true)
    setServicesLoaded(false)
    try {
      const [sg, mem, impact] = await Promise.all([
        getServiceGraph(),
        getInfrastructureMemory(),
        simulateServiceImpact('payments').catch(() => null),
      ])
      setServiceCount(sg.service_count)
      setMemoryCount(mem.incidents?.length ?? 0)
      if (impact) setServiceImpact(impact.summary)
      setServicesLoaded(true)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  const loadBaremetal = useCallback(async () => {
    setError(null)
    setLoading(true)
    setBaremetalLoaded(false)
    try {
      setBaremetal(await listBaremetalServers())
      setBaremetalLoaded(true)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void getZyraSummary().then((z) => setZyraSummary(`${z.status} · ${z.highlights?.[0] ?? z.tagline}`)).catch(() => {})
    void getRemediateHub().then((h) => {
      setHubSummary(h.summary)
      setHubItems((h.items ?? []).slice(0, 6))
    }).catch(() => {})
  }, [])

  useEffect(() => {
    if (tab === 'fleet') void loadFleet()
    if (tab === 'security') void loadSecurity()
    if (tab === 'services') void loadServices()
    if (tab === 'baremetal') void loadBaremetal()
  }, [tab, loadFleet, loadSecurity, loadServices, loadBaremetal])

  const doExecuteRebalance = async () => {
    setConfirmRebalanceExecute(false)
    const n = rebalance?.moves.length ?? 0
    setRebalanceExecuteBusy(true)
    try {
      const r = await executeFleetRebalance(false, n)
      setRebalancePreview(r.summary)
      setRebalanceTaskIds(r.task_ids ?? [])
      if (r.task_ids?.length) {
        toastQueuedOperation(toast, `Rebalance queued (${r.task_ids.length} moves)`, r.task_ids[0], tier)
      } else {
        toast.success(r.summary)
      }
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setRebalanceExecuteBusy(false)
    }
  }

  const runKnowledge = async () => {
    try {
      const [r, diag, rb] = await Promise.all([
        searchKnowledge(knowledgeQuery),
        diagnoseKnowledge(knowledgeQuery),
        getKnowledgeRunbook(knowledgeQuery),
      ])
      setKnowledgeHits(r.hits)
      setDiagnosisSummary(diag.hypotheses[0]?.title ?? diag.summary)
      setRunbookSummary(`${rb.runbook_title}: ${rb.steps?.[0] ?? rb.summary}`)
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }

  const load = useCallback(() => {
    if (tab === 'fleet') return loadFleet()
    if (tab === 'security') return loadSecurity()
    if (tab === 'services') return loadServices()
    if (tab === 'baremetal') return loadBaremetal()
  }, [tab, loadFleet, loadSecurity, loadServices, loadBaremetal])

  const tabContentLoading = loading && (
    (tab === 'fleet' && !heatmap) ||
    (tab === 'security' && !securityLoaded) ||
    (tab === 'services' && !servicesLoaded) ||
    (tab === 'baremetal' && !baremetalLoaded)
  )

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform" label="Dashboard" />}
      title="Machina Zyra OS"
      subtitle="Fleet intelligence · security graph · knowledge · services · bare metal"
      icon={<Cpu className="w-6 h-6 text-orange-400/80" />}
      actions={
        <>
          <PlatformRefreshButton onClick={() => void load()} />
          <Link to="/mission-control" className="btn-secondary text-sm">Mission Control</Link>
        </>
      }
      contentClassName="space-y-4"
    >
      {zeusSummary && <p className="text-sm text-orange-700/90">{zeusSummary}</p>}
      {hubSummary && (
        <MacGlassPanel title="Remediation hub" subtitle="SRE · compliance · fleet power — unified review queue">
          <p className="text-sm text-[var(--text-secondary)]">{hubSummary}</p>
          {hubItems.length > 0 && (
            <ul className="mt-2 text-xs space-y-1 text-[var(--text-muted)]">
              {hubItems.map((i) => (
                <li key={i.id}><span className="text-orange-600/80">{i.source}</span> · {i.label} — {i.review}</li>
              ))}
            </ul>
          )}
        </MacGlassPanel>
      )}
      <PlatformZyraHubLaunchpad activeTab={tab} />
      <DetailTabs primary={ZEUS_TABS} active={tab} onChange={setTab} />

      {tabContentLoading && <PageSkeleton />}

      {tab === 'fleet' && !tabContentLoading && heatmap && (
        <div className="space-y-4">
          {fleetSummaryLine && <p className="text-sm text-[var(--text-muted)]">{fleetSummaryLine}</p>}
          {linuxHealth && (
            <MacGlassPanel title="Fleet Linux health" subtitle="PSI · thermal · SMART rollup from hypervisors">
              <div className="apple-metric-band mb-3">
                {[
                  { label: 'Pressure hosts', value: String(linuxHealth.pressure_hosts) },
                  { label: 'Thermal alerts', value: String(linuxHealth.thermal_alerts) },
                  { label: 'SMART alerts', value: String(linuxHealth.smart_alerts) },
                ].map((s) => (
                  <div key={s.label} className="min-w-0">
                    <div className="apple-metric-value">{s.value}</div>
                    <div className="apple-metric-label">{s.label}</div>
                  </div>
                ))}
              </div>
              <p className="text-sm text-[var(--text-secondary)]">{linuxHealth.summary}</p>
              <div className="flex flex-wrap gap-3 text-xs mt-2">
                <Link to="/platform/activity" className={hubLinkClasses()}>Activity Monitor →</Link>
                <Link to="/platform/maintenance?tab=mission" className={hubLinkClasses()}>Maintenance mission →</Link>
              </div>
              {(linuxHealth.hosts ?? []).length > 0 && (
                <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3 text-xs mt-3">
                  {(linuxHealth.hosts ?? []).slice(0, 6).map((h) => (
                    <Link key={h.host_id} to={`/platform/hosts/${h.host_id}?tab=linux`} className="rounded-lg border border-white/[0.06] p-2 hover:bg-[var(--apple-surface)]">
                      <p className="font-medium text-[var(--text-primary)]">{h.hostname}</p>
                      <p className="text-[var(--text-muted)]">IO {h.io_pressure_pct.toFixed(0)}% · {h.status}</p>
                    </Link>
                  ))}
                </div>
              )}
            </MacGlassPanel>
          )}
          <MacGlassPanel title="Fleet AI diagnose" subtitle="NL diagnosis across Zyra + Linux health">
            <div className="flex flex-wrap gap-2 mb-2">
              <input aria-label="Fleet diagnose query" className="input text-sm flex-1 min-w-[12rem]" value={fleetDiagnoseQuery} onChange={(e) => setFleetDiagnoseQuery(e.target.value)} />
              <button
                type="button"
                className="btn-secondary text-xs"
                onClick={() => void diagnoseFleet(fleetDiagnoseQuery).then((d) => setFleetDiagnoseSummary(d.summary)).catch((e: unknown) => setFleetDiagnoseSummary(formatUserError(e)))}
              >
                Diagnose fleet
              </button>
            </div>
            {fleetDiagnoseSummary && <p className="text-sm text-[var(--text-secondary)]">{fleetDiagnoseSummary}</p>}
          </MacGlassPanel>
          <MacGlassPanel title="Fleet heat map" subtitle="Hot, cold, and power-waste hosts">
            <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3 text-xs">
              {(heatmap.hosts ?? []).map((h) => (
                <div key={h.host_id} className="rounded-lg border border-white/[0.06] p-2">
                  <p className="font-medium text-[var(--text-primary)]">{h.hostname}</p>
                  <p className="text-[var(--text-muted)]">CPU {h.cpu_percent.toFixed(0)}% · Mem {h.memory_percent.toFixed(0)}% · {h.classification}</p>
                </div>
              ))}
            </div>
          </MacGlassPanel>
          {powerSummary && (
            <MacGlassPanel title="Fleet power optimizer" subtitle="Consolidate cold hosts and relieve hotspots">
              <p className="text-sm text-[var(--text-secondary)]">{powerSummary}</p>
            </MacGlassPanel>
          )}
          {gpuSummary && (
            <MacGlassPanel title="GPU / NUMA placement" subtitle="Tag hosts with gpu or nvidia for affinity">
              <p className="text-sm text-[var(--text-secondary)]">{gpuSummary}</p>
            </MacGlassPanel>
          )}
          {rebalance && (
            <MacGlassPanel title="Autonomous rebalancer" subtitle={rebalance.summary}>
              <ul className="text-xs space-y-2 text-[var(--text-muted)]">
                {(rebalance.moves ?? []).map((m) => (
                  <li key={m.vm_id}>{m.vm_name}: {m.from_host} → {m.to_host}</li>
                ))}
              </ul>
              <div className="flex flex-wrap gap-2 mt-3">
                <button
                  type="button"
                  className="btn-secondary text-xs"
                  onClick={async () => {
                    try {
                      const r = await executeFleetRebalance(true)
                      setRebalancePreview(r.summary)
                    } catch (e: unknown) {
                      toast.error(formatUserError(e))
                    }
                  }}
                >
                  Preview execute
                </button>
                {(rebalance.moves ?? []).length > 0 && (
                  <button
                    type="button"
                    className="btn-primary text-xs"
                    disabled={rebalanceExecuteBusy}
                    onClick={() => setConfirmRebalanceExecute(true)}
                  >
                    {rebalanceExecuteBusy ? 'Queuing…' : 'Execute moves'}
                  </button>
                )}
              </div>
              {rebalancePreview && <p className="text-xs text-[var(--text-muted)] mt-2">{rebalancePreview}</p>}
              {rebalanceTaskIds.length > 0 && (
                <p className="text-xs mt-2">
                  <Link to={tasksHubHref(tier)} className={hubLinkClasses()}>
                    View migration tasks ({rebalanceTaskIds.length}) →
                  </Link>
                </p>
              )}
            </MacGlassPanel>
          )}
        </div>
      )}
      {tab === 'fleet' && !tabContentLoading && <ZyraAutonomousRunPanel />}

      {tab === 'security' && !tabContentLoading && (
        <div className="space-y-4">
          <MacGlassPanel title="Security hubs" subtitle="Threat intelligence and host firewall — open a hub for full detail">
            <div className="flex flex-wrap gap-3">
              <Link to="/platform/zeus/security" className="btn-secondary text-sm">
                Security Center
              </Link>
              <Link to="/platform/zeus/security/firewall" className="btn-secondary text-sm">
                Zeus Firewall
              </Link>
            </div>
            <p className="text-xs text-[var(--text-muted)] mt-3">
              Threat hunting, runtime enforcement, ports, policies, and cloud rules live inside these hubs.
            </p>
          </MacGlassPanel>
          <MacGlassPanel title="Attack path discovery" subtitle="Example: attacker → db-prod">
            <p className="text-sm text-[var(--text-secondary)]">{attackSummary ?? 'Loading…'}</p>
            <p className="text-xs text-[var(--text-muted)] mt-2">Use Spotlight: &quot;show attack path to database VM&quot;</p>
          </MacGlassPanel>
          {frameworksSummary && (
            <MacGlassPanel title="Compliance frameworks" subtitle="CIS · PCI · SOC2 · HIPAA mapping">
              <p className="text-sm text-[var(--text-secondary)]">{frameworksSummary}</p>
            </MacGlassPanel>
          )}
        </div>
      )}

      {tab === 'brain' && <MachinaInfraGraphBrain />}

      {tab === 'memory' && <MachinaInfrastructureMemory />}

      {tab === 'knowledge' && (
        <MacGlassPanel title="Infrastructure knowledge engine" subtitle="Global search + NL diagnose">
          <div className="flex gap-2">
            <input aria-label="Infrastructure knowledge search query" className="input flex-1 text-sm" value={knowledgeQuery} onChange={(e) => setKnowledgeQuery(e.target.value)} />
            <button type="button" className="btn-primary text-xs" onClick={() => void runKnowledge()}>Search</button>
          </div>
          {diagnosisSummary && <p className={`text-xs mt-2 ${statusToneClass('warn')}`}>Diagnosis: {diagnosisSummary}</p>}
          {runbookSummary && <p className={`text-xs mt-1 ${statusToneClass('ok')}`}>Runbook: {runbookSummary}</p>}
          <ul className="mt-3 space-y-1.5 text-xs">
            {knowledgeHits.map((h) => (
              <li key={`${h.kind}-${h.id}`}>
                {h.navigate ? <Link to={h.navigate} className={`hover:underline ${hubLinkClasses()}`}>{h.title}</Link> : h.title}
                <span className="text-[var(--text-muted)]"> — {h.snippet}</span>
              </li>
            ))}
          </ul>
        </MacGlassPanel>
      )}

      {tab === 'services' && !tabContentLoading && (
        <MacGlassPanel title="Service graph & infrastructure memory" subtitle="Application → VM dependencies + incident recall">
          <p className="text-sm text-[var(--text-muted)]">{serviceCount} application service(s) mapped · {memoryCount} remembered incident(s)</p>
          {serviceImpact && <p className="text-xs text-[var(--text-muted)] mt-2">{serviceImpact}</p>}
          <Link to="/platform/applications" className={`text-xs mt-2 inline-block ${hubLinkClasses()}`}>Open applications →</Link>
        </MacGlassPanel>
      )}

      {tab === 'baremetal' && !tabContentLoading && (
        <div className="space-y-4">
          <MacGlassPanel title="Bare metal servers" subtitle="Redfish / IPMI inventory + Zeus Firewall policy">
            <div className="flex flex-wrap gap-2 mb-3">
              <input className="input text-sm" aria-label="Hostname" placeholder="hostname" value={bmcHost} onChange={(e) => setBmcHost(e.target.value)} />
              <input className="input text-sm" aria-label="BMC address" placeholder="BMC address" value={bmcAddr} onChange={(e) => setBmcAddr(e.target.value)} />
              <input className="input text-sm w-24" aria-label="BMC VLAN" placeholder="BMC VLAN" value={bmcVlan} onChange={(e) => setBmcVlan(e.target.value)} />
              <input className="input text-sm w-24" aria-label="PXE VLAN" placeholder="PXE VLAN" value={pxeVlan} onChange={(e) => setPxeVlan(e.target.value)} />
              <select className="input text-sm" aria-label="Bare metal profile" value={metalProfile} onChange={(e) => setMetalProfile(e.target.value)}>
                <option value="BareMetalBmc">BareMetal BMC</option>
                <option value="BareMetalPxe">BareMetal PXE</option>
                <option value="BareMetalRedfish">BareMetal Redfish</option>
                <option value="MetalLockdown">Metal Lockdown</option>
              </select>
              <button
                type="button"
                className="btn-secondary text-xs"
                onClick={async () => {
                  try {
                    await registerBaremetalServer({
                      hostname: bmcHost,
                      bmc_address: bmcAddr,
                      bmc_vlan: bmcVlan,
                      pxe_vlan: pxeVlan,
                      firewall_profile: metalProfile,
                    })
                    setBmcHost('')
                    setBmcAddr('')
                    await loadBaremetal()
                  } catch (e: unknown) {
                    toast.error(formatUserError(e))
                  }
                }}
              >
                Register
              </button>
            </div>
            <div className="space-y-1">
              {baremetal.map((s) => (
                <MacListRow
                  key={s.id}
                  title={s.hostname}
                  subtitle={`${s.bmc_type} @ ${s.bmc_address || '—'} · ${s.firewall_profile || 'BareMetalBmc'} · ${s.state}`}
                  href={`/platform/zeus/security/firewall/${s.id}`}
                  trailing={
                    <span className="flex gap-2 text-[10px]">
                      <button type="button" className={hubLinkClasses()} onClick={(e) => {
                        e.preventDefault()
                        void setBaremetalPower(s.id, 'on', true)
                          .then((r) => setCapacitySummary(r.summary))
                          .catch((err: unknown) => toast.error(formatUserError(err)))
                      }}>Power</button>
                      <button type="button" className={hubLinkClasses()} onClick={(e) => {
                        e.preventDefault()
                        void getBaremetalProvision(s.id)
                          .then((r) => setCapacitySummary(r.summary))
                          .catch((err: unknown) => toast.error(formatUserError(err)))
                      }}>PXE</button>
                    </span>
                  }
                />
              ))}
              {baremetal.length === 0 && (
                <PlatformEmptyState
                  icon={Server}
                  title="No bare-metal servers"
                  subtitle="Register BMC targets from Zeus Firewall to manage power, PXE, and firewall profiles."
                  action={<Link to="/platform/zeus/security/firewall" className="btn-primary text-sm">Open Zeus Firewall</Link>}
                />
              )}
            </div>
            <Link to="/platform/zeus/security/firewall" className={`text-xs mt-3 inline-block ${hubLinkClasses()}`}>
              Open Zeus Firewall fleet →
            </Link>
          </MacGlassPanel>
          <MacGlassPanel title="AI capacity planner" subtitle="How many servers for N engineers?">
            <button
              type="button"
              className="btn-primary text-xs"
              onClick={async () => {
                try {
                  const p = await planBaremetalCapacity('How many servers for 500 AI engineers?')
                  setCapacitySummary(p.summary)
                } catch (e: unknown) {
                  toast.error(formatUserError(e))
                }
              }}
            >
              Plan 500 AI engineers
            </button>
            {capacitySummary && <p className="text-sm text-[var(--text-secondary)] mt-2">{capacitySummary}</p>}
          </MacGlassPanel>
        </div>
      )}
      <ConfirmDialog
        open={confirmRebalanceExecute}
        title="Execute Fleet Rebalance"
        message={`Queue up to ${rebalance?.moves.length ?? 0} live migration${(rebalance?.moves.length ?? 0) === 1 ? '' : 's'}? VMs will be live-migrated to more balanced hosts.`}
        confirmLabel="Execute"
        variant="warning"
        onCancel={() => setConfirmRebalanceExecute(false)}
        onConfirm={() => void doExecuteRebalance()}
      />
    </PlatformPageChrome>
  )
}
