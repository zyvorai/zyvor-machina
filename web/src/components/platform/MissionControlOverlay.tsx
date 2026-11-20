// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link, useLocation } from 'react-router'
import { AlertTriangle, ArrowRightLeft, Boxes, Server, X } from 'lucide-react'
import {
  getClusterSummary,
  getFleetMission,
  listNotifications,
  listPlatformHosts,
  listPlatformTasks,
  listPlatformVms,
  type ClusterSummary,
  type FleetMissionOverview,
  type PlatformHost,
  type PlatformTask,
  type PlatformVm,
} from '../../api/platform'
import { getAiCapacity, getAiCompliance, getAiCost, getSreForecast, getZyraSummary, type CapacityPlan, type ComplianceReport, type CostAnalysis, type SreForecast } from '../../api/ai'
import MachinaEnvironmentPlanner from '../ai/MachinaEnvironmentPlanner'
import MachinaInfrastructureTimeline from '../ai/MachinaInfrastructureTimeline'
import MachinaMissionStack from '../ai/MachinaMissionStack'
import InfrastructureDnaStrip from './InfrastructureDnaStrip'
import InfrastructureEarthView from './InfrastructureEarthView'
import MissionControlDesktopZones from './mac/MissionControlDesktopZones'
import { MacSectionTitle } from './mac/PlatformMacUi'
import { useKeyboardShortcut } from '../../hooks/useKeyboardShortcut'
import { formatUserError } from '../../utils/apiError'
import { hostStateTone, statusActionLinkClasses, statusChipClasses, statusToneClass } from '../../utils/semanticColors'
import { useFleetDesktop } from '../../hooks/useFleetDesktop'
import { useMissionControl } from './mac/MissionControlContext'
import { loadPlatformDesktopTabs } from '../../utils/platformDesktopTabs'
import { usePlatformDesktopTier } from '../../hooks/usePlatformDesktopTier'
import { operationsHubHref, tasksHubHref } from '../../utils/platformHubLinks'
import { formatFleetDisplayTitle } from '../../utils/fleetDisplayName'

export default function MissionControlOverlay() {
  const location = useLocation()
  const { open, closeMissionControl } = useMissionControl()
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [vms, setVms] = useState<PlatformVm[]>([])
  const [tasks, setTasks] = useState<PlatformTask[]>([])
  const [cluster, setCluster] = useState<ClusterSummary | null>(null)
  const [mission, setMission] = useState<FleetMissionOverview | null>(null)
  const [alerts, setAlerts] = useState<Array<{ id: string; kind: string; created_at: string }>>([])
  const [aiCost, setAiCost] = useState<CostAnalysis | null>(null)
  const [aiCap, setAiCap] = useState<CapacityPlan | null>(null)
  const [aiComp, setAiComp] = useState<ComplianceReport | null>(null)
  const [sreForecasts, setSreForecasts] = useState<SreForecast[]>([])
  const [zyraStatus, setZyraStatus] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const { desktop, linuxHealth } = useFleetDesktop(open, 120_000)
  const [tier] = usePlatformDesktopTier()
  const fleetTitle = useMemo(
    () => formatFleetDisplayTitle(cluster, hosts),
    [cluster, hosts],
  )

  const load = useCallback(async () => {
    setError(null)
    try {
      const [h, v, t, c, m, n, cost, cap, comp, sre, zyra] = await Promise.all([
        listPlatformHosts().catch(() => []),
        listPlatformVms().catch(() => []),
        listPlatformTasks().catch(() => []),
        getClusterSummary().catch(() => null),
        getFleetMission().catch(() => null),
        listNotifications(true).catch(() => []),
        getAiCost().catch(() => null),
        getAiCapacity().catch(() => null),
        getAiCompliance().catch(() => null),
        getSreForecast().catch(() => ({ forecasts: [] })),
        getZyraSummary().catch(() => null),
      ])
      setHosts(h)
      setVms(v)
      setTasks(t)
      setCluster(c)
      setMission(m)
      setAlerts(n)
      setAiCost(cost)
      setAiCap(cap)
      setAiComp(comp)
      setSreForecasts(sre.forecasts ?? [])
      setZyraStatus(zyra ? `${zyra.status} · ${zyra.highlights?.[0] ?? zyra.tagline}` : null)
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }, [])

  useEffect(() => {
    if (!open) return
    void load()
  }, [open, load])

  useKeyboardShortcut({
    key: 'Escape',
    handler: () => { if (open) closeMissionControl() },
    enabled: open,
  })

  if (!open || location.pathname.replace(/\/$/, '') === '/platform') return null

  const failedTasks = tasks.filter((t) => t.status === 'failed')
  const migrations = tasks.filter((t) => t.operation.includes('migrate'))
  const openWindows = loadPlatformDesktopTabs().filter((t) => t.path !== '/platform')

  return (
    <div
      className="fixed inset-0 z-[90] bg-[var(--apple-surface)]/95 backdrop-blur-xl overflow-y-auto animate-fade-in"
      role="dialog"
      aria-modal="true"
      aria-label="Mission Control"
    >
      <button
        type="button"
        className="absolute inset-0 cursor-default"
        aria-label="Close Mission Control"
        onClick={closeMissionControl}
      />
      <div className="relative z-[1] min-h-full">
        <header className="sticky top-0 z-10 flex items-center justify-between px-6 py-4 border-b border-white/[0.06] bg-[var(--apple-surface)]/90">
          <div>
            <h1 className="text-xl font-bold text-[var(--text-primary)]">Mission Control</h1>
            <p className="text-sm text-[var(--text-muted)]">
              Infrastructure Earth · {fleetTitle} · {hosts.length} hosts · {vms.length} VMs
            </p>
          </div>
          <button type="button" className="btn-secondary text-sm flex items-center gap-2" onClick={closeMissionControl}>
            <X className="w-4 h-4" /> Close
          </button>
        </header>

        {error && <p className={`px-6 py-2 text-sm ${statusToneClass('error')}`}>{error}</p>}

        <div className="px-6 py-2">
          <InfrastructureDnaStrip compact />
        </div>

        {openWindows.length > 0 && (
          <div className="px-6 py-3 border-b border-white/[0.06]">
            <p className="text-[10px] font-semibold uppercase tracking-wider text-[var(--text-muted)] mb-2">Open windows</p>
            <div className="flex flex-wrap gap-2">
              {openWindows.map((tab) => (
                <Link
                  key={tab.path}
                  to={tab.path}
                  className="rounded-full border border-[color-mix(in_srgb,var(--apple-hairline)_90%,transparent)] bg-[var(--apple-surface)] px-3 py-1 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[color-mix(in_srgb,var(--apple-surface)_80%,transparent)] transition"
                  onClick={closeMissionControl}
                >
                  {tab.label}
                </Link>
              ))}
            </div>
          </div>
        )}

        {(aiCost || aiCap || aiComp) && (
          <div className="px-6 pb-2 flex flex-wrap gap-3 text-xs">
            {aiCost && (
              <Link to="/platform/reports" className={statusChipClasses('ok')} onClick={closeMissionControl}>
                Cost ${aiCost.estimated_monthly_usd.toFixed(0)}/mo · {aiCost.idle_vm_count} idle
              </Link>
            )}
            {aiCap && (
              <Link to="/platform/reports" className={statusChipClasses('info')} onClick={closeMissionControl}>
                Capacity {aiCap.memory_headroom_mib} MiB headroom
              </Link>
            )}
            {aiComp && (
              <Link to="/platform/reports" className={statusChipClasses('warn')} onClick={closeMissionControl}>
                Compliance {aiComp.score}/100 (Grade {aiComp.grade})
              </Link>
            )}
          </div>
        )}

        {sreForecasts.length > 0 && (
          <div className="px-6 pb-2 flex flex-wrap gap-2 text-xs">
            {sreForecasts.slice(0, 4).map((f) => (
              <span
                key={`${f.vm_id}-${f.resource}`}
                className={statusChipClasses(f.severity === 'critical' ? 'error' : 'warn')}
              >
                AI SRE: {f.message}
              </span>
            ))}
          </div>
        )}

        {zyraStatus && (
          <div className="px-6 pb-2">
            <Link to="/platform/zyra" className="text-xs text-orange-600/90 hover:underline" onClick={closeMissionControl}>{zyraStatus}</Link>
          </div>
        )}

        {desktop && (
          <div className="px-6 pb-2 flex flex-wrap gap-2 text-xs">
            <span className="rounded-full border border-white/[0.08] bg-[var(--apple-surface)] px-3 py-1 text-[var(--text-secondary)]">{desktop.summary}</span>
            {(linuxHealth?.pressure_hosts ?? desktop.pressure_hosts) > 0 && (
              <Link to="/platform/hosts" className={statusChipClasses('warn')} onClick={closeMissionControl}>
                {linuxHealth?.summary ?? desktop.linux_summary}
              </Link>
            )}
          </div>
        )}

        <div className="px-6 py-2">
          <MissionControlDesktopZones onNavigate={closeMissionControl} />
        </div>

        <div className="px-6 pt-2 pb-1">
          <MacSectionTitle title="Live fleet" subtitle="Infrastructure Earth, AI planners, and inventory" />
        </div>

        <div className="px-6 py-4">
          <InfrastructureEarthView mission={mission} />
        </div>

        <div className="px-6 pb-4 space-y-4">
          <MachinaEnvironmentPlanner />
          <MachinaMissionStack />
          <MachinaInfrastructureTimeline hours={4} />
        </div>

        <div className="p-6 grid gap-6 lg:grid-cols-2 xl:grid-cols-4">
          <section className="rounded-2xl border border-white/[0.06] bg-[var(--apple-surface)] p-4 space-y-3">
            <h2 className="text-sm font-semibold text-[var(--text-muted)] flex items-center gap-2"><Server className="w-4 h-4" /> Hosts</h2>
            <ul className="space-y-2 text-sm max-h-64 overflow-y-auto">
              {hosts.map((h) => (
                <li key={h.id}>
                  <Link
                    to={`/platform/vms?lens=topology&host=${encodeURIComponent(h.id)}`}
                    className={`flex justify-between ${statusActionLinkClasses('info', 'hover:opacity-90')}`}
                    onClick={closeMissionControl}
                    title="Open in Machine Finder"
                  >
                    <span>{h.hostname}</span>
                    <span className={statusToneClass(hostStateTone(h.state))}>{h.state}</span>
                  </Link>
                </li>
              ))}
              <Link to="/platform/vms?lens=topology" className={`text-xs ${statusActionLinkClasses('info')}`} onClick={closeMissionControl}>
                Open Machine Finder →
              </Link>
            </ul>
          </section>
          <section className="rounded-2xl border border-white/[0.06] bg-[var(--apple-surface)] p-4 space-y-3">
            <h2 className="text-sm font-semibold text-[var(--text-muted)] flex items-center gap-2"><Boxes className="w-4 h-4" /> Virtual machines</h2>
            <ul className="space-y-2 text-sm max-h-64 overflow-y-auto">
              {vms.slice(0, 24).map((v) => (
                <li key={v.id}>
                  <Link
                    to={v.host_id
                      ? `/platform/vms?lens=topology&host=${encodeURIComponent(v.host_id)}&vm=${encodeURIComponent(v.id)}`
                      : `/platform/vms/${v.id}`}
                    className={`flex justify-between ${statusActionLinkClasses('info', 'hover:opacity-90')}`}
                    onClick={closeMissionControl}
                    title={v.host_id ? 'Open in Machine Finder' : 'Open VM'}
                  >
                    <span className="truncate">{v.name}</span>
                    <span className="text-[var(--text-muted)] shrink-0 ml-2">{v.observed_state}</span>
                  </Link>
                </li>
              ))}
            </ul>
          </section>
          <section className="rounded-2xl border border-white/[0.06] bg-[var(--apple-surface)] p-4 space-y-3">
            <h2 className="text-sm font-semibold text-[var(--text-muted)] flex items-center gap-2"><AlertTriangle className="w-4 h-4" /> Alerts</h2>
            {alerts.length === 0 ? (
              <p className="text-sm text-[var(--text-muted)]">No unread alerts</p>
            ) : (
              <ul className="space-y-2 text-sm max-h-64 overflow-y-auto">
                {alerts.map((a) => (
                  <li key={a.id} className={statusToneClass('warn')}>{a.kind}</li>
                ))}
              </ul>
            )}
            <Link to={operationsHubHref(tier)} className={`text-xs ${statusActionLinkClasses('info')}`} onClick={closeMissionControl}>Open Operations hub →</Link>
          </section>
          <section className="rounded-2xl border border-white/[0.06] bg-[var(--apple-surface)] p-4 space-y-3">
            <h2 className="text-sm font-semibold text-[var(--text-muted)] flex items-center gap-2"><ArrowRightLeft className="w-4 h-4" /> Migrations & tasks</h2>
            <p className="text-xs text-[var(--text-muted)]">
              {migrations.length} migration tasks ·{' '}
              {failedTasks.length > 0 ? (
                <Link to={tasksHubHref(tier)} className="text-amber-600 hover:underline" onClick={closeMissionControl}>
                  {failedTasks.length} failed
                </Link>
              ) : '0 failed'}
            </p>
            <ul className="space-y-2 text-sm max-h-48 overflow-y-auto">
              {failedTasks.slice(0, 8).map((t) => (
                <li key={t.id} className={`truncate ${statusToneClass('error')} opacity-90`}>{t.operation} — {t.status}</li>
              ))}
            </ul>
            <Link to={tasksHubHref(tier)} className={`text-xs ${statusActionLinkClasses('info')}`} onClick={closeMissionControl}>View all tasks →</Link>
          </section>
        </div>
      </div>
    </div>
  )
}
