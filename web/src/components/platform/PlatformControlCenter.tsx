// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Link, useLocation } from 'react-router'
import {
  SlidersHorizontal,
  X,
  CheckCircle2,
  AlertTriangle,
  Loader2,
  HelpCircle,
  Bot,
  Shield,
  Server,
  RefreshCw,
  Sparkles,
  Activity,
  Boxes,
  Wrench,
  Settings,
} from 'lucide-react'
import {
  getCapacityReport,
  getClusterSummary,
  getFleetUpdates,
  getTaskFailures,
  listNotifications,
  listPlatformHosts,
  listPlatformTasks,
  listPlatformVms,
  getNetworkSegmentsOverview,
  getStorageTiersOverview,
  syncAllHosts,
  type CapacityReport,
  type ClusterSummary,
  type PlatformHost,
  type PlatformTask,
} from '../../api/platform'
import { getZyraSummary } from '../../api/ai'
import { getOperatorSecurePlan } from '../../api/zeusFirewall'
import { ZYRA_ASSISTANT_NAME } from '../../config/aiBrand'
import { useAi } from '../../contexts/AiContext'
import { useFleetDesktop } from '../../hooks/useFleetDesktop'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { statusBgClass, statusSurfaceClasses, statusToneClass } from '../../utils/semanticColors'
import { usePlatformDesktopTier } from '../../hooks/usePlatformDesktopTier'
import { tierAtLeast } from '../../utils/platformDesktopTier'
import { usePlatformInfo } from '../../contexts/PlatformInfoContext'
import { hubTilesForTier, type DesktopHubTile } from '../../utils/platformHubZones'
import { operationsHubHref } from '../../utils/platformHubLinks'
import { DISMISS_PLATFORM_SHELL_EVENT, loadJarvisShell, saveJarvisShell } from '../../utils/platformJarvisShell'

export default function PlatformControlCenter() {
  const location = useLocation()
  const { mode, openCopilot } = useAi()
  const toast = useToastContext()
  const [tier] = usePlatformDesktopTier()
  const { info } = usePlatformInfo()
  const [open, setOpen] = useState(false)
  const { desktop, linuxHealth } = useFleetDesktop(open, 0)
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [vms, setVms] = useState<{ observed_state: string }[]>([])
  const [tasks, setTasks] = useState<PlatformTask[]>([])
  // Failures of the last 24 h that nobody acknowledged (a raw count of every failed task ever never goes down).
  const [openFailures, setOpenFailures] = useState<number | null>(null)
  const [cluster, setCluster] = useState<ClusterSummary | null>(null)
  const [capacity, setCapacity] = useState<CapacityReport | null>(null)
  const [unreadAlerts, setUnreadAlerts] = useState(0)
  const [zyra, setZyra] = useState<{ firewall_critical_hosts?: number; firewall_drift_hosts?: number; baremetal_critical_count?: number } | null>(null)
  const [operatorSummary, setOperatorSummary] = useState<string | null>(null)
  const [segmentCount, setSegmentCount] = useState(0)
  const [storageTierCount, setStorageTierCount] = useState(0)
  const [syncing, setSyncing] = useState(false)
  const [hostsRebootRequired, setHostsRebootRequired] = useState(0)
  const [jarvisShell, setJarvisShell] = useState(() => loadJarvisShell(tier))

  const load = useCallback(async () => {
    try {
      const [h, v, t, c, cap, alerts, zs, op, segs, storageTiers, updates, failures] = await Promise.all([
        listPlatformHosts(),
        listPlatformVms(),
        listPlatformTasks(),
        getClusterSummary(),
        getCapacityReport().catch(() => null),
        listNotifications(true).catch(() => []),
        getZyraSummary().catch(() => null),
        getOperatorSecurePlan().catch(() => null),
        getNetworkSegmentsOverview().catch(() => ({ segments: [] })),
        getStorageTiersOverview().catch(() => ({ tiers: [], summary: '' })),
        getFleetUpdates().catch(() => null),
        getTaskFailures(24).catch(() => null),
      ])
      setHosts(h)
      setVms(v)
      setTasks(t)
      setOpenFailures(failures ? failures.unacknowledged : null)
      setCluster(c)
      setCapacity(cap)
      setUnreadAlerts(alerts.length)
      setZyra(zs)
      setOperatorSummary(op?.summary ?? null)
      setSegmentCount(segs.segments?.length ?? 0)
      setStorageTierCount(storageTiers.tiers?.length ?? 0)
      setHostsRebootRequired(updates?.hosts_reboot_required ?? 0)
    } catch {
      /* optional panel */
    }
  }, [])

  useEffect(() => {
    if (open) void load()
  }, [open, load])

  useEffect(() => {
    const dismiss = () => setOpen(false)
    window.addEventListener(DISMISS_PLATFORM_SHELL_EVENT, dismiss)
    return () => window.removeEventListener(DISMISS_PLATFORM_SHELL_EVENT, dismiss)
  }, [])

  useEffect(() => {
    setOpen(false)
  }, [location.pathname, location.search])

  const running = vms.filter((v) => v.observed_state === 'running').length
  const activeTasks = tasks.filter((t) => t.status === 'running' || t.status === 'pending').length
  const failedTasks = openFailures ?? tasks.filter((t) => t.status === 'failed').length
  const offlineHosts = hosts.filter((h) => h.state === 'offline')
  const offlineCount = offlineHosts.length
  const warnings = offlineCount + failedTasks
  const memPct = capacity && capacity.memory_total_mib > 0
    ? Math.round((capacity.memory_used_mib / capacity.memory_total_mib) * 100)
    : null
  const fwCritical = zyra?.firewall_critical_hosts ?? 0
  const fwDrift = zyra?.firewall_drift_hosts ?? 0
  const metalCritical = zyra?.baremetal_critical_count ?? 0
  const showPower = tierAtLeast(tier, 'power')
  const hubTiles = hubTilesForTier(tier)
  const closePanel = () => setOpen(false)

  const hubIcon = (id: DesktopHubTile['id']) => {
    switch (id) {
      case 'infrastructure':
        return <Server className={`w-4 h-4 ${statusToneClass('info')}`} />
      case 'workloads':
        return <Boxes className={`w-4 h-4 ${statusToneClass('info')}`} />
      case 'operations':
        return <Wrench className={`w-4 h-4 ${statusToneClass('ok')}`} />
      case 'administration':
        return <Settings className={`w-4 h-4 ${statusToneClass('info')}`} />
      case 'security':
        return <Shield className="w-4 h-4 text-[var(--accent)]" />
      default:
        return <Boxes className="w-4 h-4 text-[var(--text-muted)]" />
    }
  }

  const hubValue = (id: DesktopHubTile['id']): { value: string; tone?: 'ok' | 'warn'; spark?: string } => {
    switch (id) {
      case 'infrastructure':
        return {
          value: storageTierCount || segmentCount ? 'Fleet foundation' : 'Open hub',
          spark: [storageTierCount ? `${storageTierCount} tier(s)` : null, segmentCount ? `${segmentCount} segment(s)` : null]
            .filter(Boolean)
            .join(' · ') || undefined,
          tone: storageTierCount || segmentCount ? 'ok' : undefined,
        }
      case 'workloads':
        return {
          value: `${running} running`,
          spark: vms.length ? `${vms.length} total VMs` : undefined,
          tone: vms.length ? 'ok' : undefined,
        }
      case 'administration':
        return {
          value: 'Users & policies',
        }
      case 'operations':
        return {
          value: `${activeTasks} active task${activeTasks === 1 ? '' : 's'}`,
          spark: [unreadAlerts ? `${unreadAlerts} alert(s)` : null, failedTasks ? `${failedTasks} failed` : null]
            .filter(Boolean)
            .join(' · ') || undefined,
          tone: unreadAlerts || failedTasks ? 'warn' : 'ok',
        }
      case 'security':
        return {
          value: fwCritical || metalCritical ? `${fwCritical + metalCritical} critical` : 'Posture OK',
          spark: fwDrift ? `${fwDrift} firewall drift` : operatorSummary ?? undefined,
          tone: fwCritical || metalCritical || fwDrift ? 'warn' : 'ok',
        }
      default:
        return { value: 'Open' }
    }
  }

  const syncHosts = async () => {
    setSyncing(true)
    try {
      await syncAllHosts()
      toast.success('Host sync queued')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSyncing(false)
    }
  }

  return (
    <div className="relative">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex items-center gap-2 px-3 py-1.5 rounded-full bg-[var(--apple-fill-tertiary)]/80 border border-[var(--apple-hairline)] text-sm text-[var(--text-primary)] hover:bg-[var(--surface-hover)]/80 transition"
        aria-label="Control Center"
        aria-haspopup="menu"
        aria-expanded={open}
      >
        <SlidersHorizontal className="w-4 h-4" />
        <span className="hidden xl:inline">Control Center</span>
        {(warnings > 0 || fwCritical > 0 || metalCritical > 0) && (
          <span className="w-2 h-2 rounded-full bg-[var(--machina-status-warn)] animate-pulse-dot" role="status" aria-label="Active alerts present" />
        )}
      </button>
      {open && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} aria-hidden />
          <div className="absolute right-0 top-full mt-2 z-50 w-[min(26rem,calc(100vw-1.5rem))] glass-strong rounded-liquid-lg overflow-hidden animate-fade-in">
            <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--apple-hairline)]">
              <span className="font-semibold text-sm">Control Center</span>
              <button type="button" onClick={() => setOpen(false)} className="p-1 rounded-lg hover:bg-[var(--apple-fill-tertiary)] text-[var(--text-muted)]" aria-label="Close control center">
                <X className="w-4 h-4" />
              </button>
            </div>
            <div className="p-4 space-y-4 text-sm">
              <div className="grid gap-2 grid-cols-2">
                <ModuleTile
                  icon={<Server className={`w-4 h-4 ${statusToneClass(offlineCount === 0 ? 'ok' : 'warn')}`} />}
                  label="Cluster"
                  value={offlineCount === 0 ? 'Healthy' : `${offlineCount} offline`}
                  href="/platform"
                  tone={offlineCount === 0 ? 'ok' : 'warn'}
                  spark={memPct != null ? `${memPct}% mem` : undefined}
                  onNavigate={closePanel}
                />
                {showPower && (
                  <ModuleTile
                    icon={<Loader2 className={`w-4 h-4 ${statusToneClass(failedTasks ? 'warn' : 'ok')} ${activeTasks ? 'animate-spin' : ''}`} />}
                    label="Tasks"
                    value={String(activeTasks)}
                    href={operationsHubHref(tier)}
                    spark={failedTasks ? `${failedTasks} failed` : undefined}
                    tone={failedTasks ? 'warn' : undefined}
                    onNavigate={closePanel}
                  />
                )}
                {hubTiles.map((hub) => {
                  const meta = hubValue(hub.id)
                  return (
                    <ModuleTile
                      key={hub.id}
                      icon={hubIcon(hub.id)}
                      label={hub.label}
                      value={meta.value}
                      href={hub.href}
                      tone={meta.tone}
                      spark={meta.spark}
                      onNavigate={closePanel}
                    />
                  )
                })}
                {showPower && (
                  <ModuleTile
                    icon={<Bot className="w-4 h-4 text-[var(--accent)]" />}
                    label={ZYRA_ASSISTANT_NAME}
                    value={mode === 'off' ? 'Off' : mode === 'autopilot' ? 'Autopilot' : 'Advisor'}
                    onClick={() => { openCopilot(); setOpen(false) }}
                  />
                )}
              </div>

              {showPower && (memPct != null || activeTasks > 0) && (
                <div className="rounded-xl border border-white/[0.06] bg-[var(--apple-surface)] p-3 space-y-2">
                  <p className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">Capacity</p>
                  {memPct != null && (
                    <SparklineBar label="Memory" pct={memPct} tone={memPct > 85 ? 'warn' : 'ok'} />
                  )}
                  <SparklineBar
                    label="Task rate"
                    pct={Math.min(100, activeTasks * 10)}
                    tone={failedTasks ? 'warn' : 'ok'}
                    caption={`${activeTasks} active · ${failedTasks} failed`}
                  />
                </div>
              )}

              {operatorSummary && (
                <Row
                  icon={<Sparkles className="w-4 h-4 text-[var(--accent)]" />}
                  label="AI operator"
                  value={operatorSummary}
                  href="/platform/zeus/security"
                  tone="warn"
                  onNavigate={closePanel}
                />
              )}
              {((desktop?.pressure_hosts ?? linuxHealth?.pressure_hosts ?? 0) > 0) && (
                <Row
                  icon={<Activity className={`w-4 h-4 ${statusToneClass('warn')}`} />}
                  label="Linux pressure"
                  value={desktop?.linux_summary ?? linuxHealth?.summary ?? 'Hosts under IO/thermal pressure'}
                  href="/platform/hosts"
                  tone="warn"
                  onNavigate={closePanel}
                />
              )}
              {hostsRebootRequired > 0 && (
                <Row
                  icon={<RefreshCw className={`w-4 h-4 ${statusToneClass('warn')}`} />}
                  label="Reboot required"
                  value={`${hostsRebootRequired} host(s) pending reboot after patches`}
                  href="/platform/maintenance?tab=updates"
                  tone="warn"
                  onNavigate={closePanel}
                />
              )}
              {offlineCount > 0 && (
                <Row
                  icon={<AlertTriangle className={`w-4 h-4 ${statusToneClass('warn')}`} />}
                  label={offlineHosts[0]?.hostname ?? 'Offline host'}
                  value="Fix — sync agent"
                  href={`/platform/hosts/${offlineHosts[0]?.id ?? ''}`}
                  tone="warn"
                  onNavigate={closePanel}
                />
              )}
              <Row
                icon={<AlertTriangle className={`w-4 h-4 ${statusToneClass('warn')}`} />}
                label="Alerts"
                value={unreadAlerts ? `${unreadAlerts} unread` : warnings ? `${warnings} item(s)` : 'None'}
                href={operationsHubHref(tier)}
                tone={unreadAlerts || warnings ? 'warn' : 'ok'}
                onNavigate={closePanel}
              />
            </div>
            <div className="px-4 py-3 border-t border-[var(--apple-hairline)] space-y-2">
              <label className="flex items-center justify-between gap-3 text-xs text-[var(--text-secondary)] cursor-pointer">
                <span className="flex items-center gap-2">
                  <Sparkles className="w-3.5 h-3.5 text-[var(--link)]" />
                  Jarvis shell (minimal menus)
                </span>
                <input
                  type="checkbox"
                  checked={jarvisShell}
                  onChange={(e) => {
                    const next = e.target.checked
                    setJarvisShell(next)
                    saveJarvisShell(next)
                  }}
                  className="rounded border-[var(--apple-hairline)]"
                />
              </label>
              {showPower && (
              <>
              <p className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">Quick actions</p>
              <div className="flex flex-wrap gap-2">
                <button type="button" className="btn-secondary text-xs flex items-center gap-1" disabled={syncing} onClick={() => void syncHosts()}>
                  <RefreshCw className={`w-3 h-3 ${syncing ? 'animate-spin' : ''}`} /> Sync hosts
                </button>
              </div>
              </>
              )}
            </div>
            <div className="px-4 py-3 border-t border-[var(--apple-hairline)] flex gap-2">
              <Link to="/platform/support" className="btn-secondary text-xs flex items-center justify-center gap-1" onClick={() => setOpen(false)}>
                <HelpCircle className="w-3 h-3" /> Help
              </Link>
              <Link to="/platform/settings?section=general" className="btn-secondary text-xs flex-1 text-center" onClick={() => setOpen(false)}>Settings</Link>
              {showPower && (
              <Link to={operationsHubHref(tier)} className="btn-primary text-xs flex-1 text-center" onClick={() => setOpen(false)}>Operations</Link>
              )}
            </div>
            <div className="px-4 pb-3 text-xs text-[var(--text-muted)]">
              {running} VMs running · {hosts.filter((h) => h.state === 'online').length}/{hosts.length} hosts online
            </div>
          </div>
        </>
      )}
    </div>
  )
}

function ModuleTile({
  icon,
  label,
  value,
  href,
  onClick,
  onNavigate,
  tone,
  spark,
}: {
  icon: React.ReactNode
  label: string
  value: string
  href?: string
  onClick?: () => void
  onNavigate?: () => void
  tone?: 'ok' | 'warn'
  spark?: string
}) {
  const cls = `rounded-xl border p-3 text-left transition hover:bg-[var(--apple-surface)] ${
    tone === 'warn' ? statusSurfaceClasses('warn') : 'border-white/[0.06] bg-[var(--apple-surface)]'
  }`
  const inner = (
    <>
      <div className="flex items-center gap-2 mb-1">{icon}<span className="text-xs text-[var(--text-muted)]">{label}</span></div>
      <p className="font-medium text-[var(--text-primary)] text-sm">{value}</p>
      {spark && <p className="text-[10px] text-[var(--text-muted)] mt-0.5">{spark}</p>}
    </>
  )
  if (href) {
    return (
      <Link to={href} className={cls} onClick={onNavigate}>
        {inner}
      </Link>
    )
  }
  return (
    <button type="button" className={`${cls} w-full`} onClick={onClick}>
      {inner}
    </button>
  )
}

function SparklineBar({
  label,
  pct,
  tone,
  caption,
}: {
  label: string
  pct: number
  tone?: 'ok' | 'warn'
  caption?: string
}) {
  return (
    <div>
      <div className="flex justify-between text-xs mb-1">
        <span className="text-[var(--text-muted)]">{label}</span>
        <span className={tone === 'warn' ? statusToneClass('warn') : 'text-[var(--text-secondary)]'}>{caption ?? `${pct}%`}</span>
      </div>
      <div className="h-1.5 rounded-full bg-[var(--apple-fill-tertiary)] overflow-hidden">
        <div
          role="progressbar"
          aria-label={label}
          aria-valuenow={Math.round(Math.min(100, Math.max(0, pct)))}
          aria-valuemin={0}
          aria-valuemax={100}
          className={`h-full rounded-full ${statusBgClass(tone === 'warn' ? 'warn' : 'info')}`}
          style={{ width: `${Math.min(100, Math.max(0, pct))}%` }}
        />
      </div>
    </div>
  )
}

function Row({
  icon,
  label,
  value,
  href,
  tone,
  onNavigate,
}: {
  icon: React.ReactNode
  label: string
  value: string
  href?: string
  tone?: 'ok' | 'warn'
  onNavigate?: () => void
}) {
  const cls = `flex items-center gap-3 p-2 rounded-xl hover:bg-[var(--apple-surface)] transition ${tone === 'warn' ? statusToneClass('warn') : ''}`
  const inner = (
    <>
      {icon}
      <div className="flex-1 min-w-0">
        <p className="text-[var(--text-secondary)] truncate">{label}</p>
        <p className="text-xs text-[var(--text-muted)]">{value}</p>
      </div>
      {href && <CheckCircle2 className="w-3 h-3 text-[var(--text-faint)] shrink-0" />}
    </>
  )
  if (href) {
    return (
      <Link to={href} className={cls} onClick={onNavigate}>
        {inner}
      </Link>
    )
  }
  return <div className={cls}>{inner}</div>
}
