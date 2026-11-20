// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, useCallback } from 'react'
import { Link } from 'react-router'
import { listVMs, getMetrics, VmInfo, VmMetrics } from '../api/vm'
import { getHostVirtualization, getLibvirtSummary, getHealthProblems, type HealthProblemItem } from '../api/host'
import { listNetworks, NetworkInfo } from '../api/network'
import { listPools, StoragePoolInfo } from '../api/storage'
import { getNodeInfo, NodeInfo } from '../api/node'
import { getHostStats, HostStats, hostShutdown, hostReboot } from '../api/extras'
import { timeAgo } from '../utils/time'
import { ArrowRight, ChevronRight, Power, RotateCcw, Plus, Trash2, AlertTriangle, X, RefreshCw } from 'lucide-react'
import { AreaChart, Area, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts'
import { useWebSocketContext } from '../contexts/WebSocketContext'
import { useToastContext } from '../contexts/ToastContext'
import { usePlatformInfo } from '../contexts/PlatformInfoContext'
import { getK8sEnvironment, getK8sOverview, type K8sOverview } from '../api/k8s'
import { getVesselStatus, listVesselContainers, type VesselStatus } from '../api/vessel'
import PageLayout from '../components/PageLayout'
import { formatUserError } from '../utils/apiError'
import { libvirtErrorHints } from '../utils/libvirtHints'
import { statusActionLinkClasses, statusBgClass, statusBorderClass, statusSurfaceClasses, statusToneClass, utilizationTone } from '../utils/semanticColors'

interface MetricsPoint { time: string; memory: number }

/** Open metric rhythm — apple.com product spacing, not a dense dashboard grid. */
const METRIC_GRID = 'apple-metric-band'

export default function Dashboard() {
  const [vms, setVMs] = useState<VmInfo[]>([])
  const [networks, setNetworks] = useState<NetworkInfo[]>([])
  const [pools, setPools] = useState<StoragePoolInfo[]>([])
  const [node, setNode] = useState<NodeInfo | null>(null)
  const [loading, setLoading] = useState(true)
  const [hostStats, setHostStats] = useState<HostStats | null>(null)
  const [virtHost, setVirtHost] = useState<Awaited<ReturnType<typeof getHostVirtualization>> | null>(null)
  const [libSummary, setLibSummary] = useState<Awaited<ReturnType<typeof getLibvirtSummary>> | null>(null)
  const [virtBannerDismissed, setVirtBannerDismissed] = useState(
    () => typeof localStorage !== 'undefined' && localStorage.getItem('machina_virt_banner_dismiss') === '1',
  )
  const [healthProblems, setHealthProblems] = useState<HealthProblemItem[]>([])
  const [metricsHistory, setMetricsHistory] = useState<MetricsPoint[]>([])
  const { subscribe, events } = useWebSocketContext()
  const toast = useToastContext()
  const { info, lastEvent, refreshKey } = usePlatformInfo()
  const [k8sOverview, setK8sOverview] = useState<K8sOverview | null>(null)
  const [k8sError, setK8sError] = useState<string | null>(null)
  const [vesselStatus, setVesselStatus] = useState<VesselStatus | null>(null)
  const [vesselContainerCount, setVesselContainerCount] = useState<number | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [showShutdownConfirm, setShowShutdownConfirm] = useState(false)
  const [showRebootConfirm, setShowRebootConfirm] = useState(false)

  const platformEnabled = Boolean(info?.control_plane?.proxy_url)

  const loadData = useCallback(async () => {
    const [vmR, netR, poolR, nodeR] = await Promise.allSettled([
      listVMs(),
      listNetworks(),
      listPools(),
      getNodeInfo(),
    ])
    if (vmR.status === 'fulfilled') {
      setVMs(vmR.value)
      setLoadError(null)
    } else {
      setLoadError(formatUserError(vmR.reason))
    }
    if (netR.status === 'fulfilled') setNetworks(netR.value)
    if (poolR.status === 'fulfilled') setPools(poolR.value)
    if (nodeR.status === 'fulfilled') setNode(nodeR.value)
    else if (import.meta.env.DEV) console.warn('Dashboard: getNodeInfo failed', nodeR.reason)

    await Promise.all([
      (async () => { try { setHostStats(await getHostStats()) } catch { /* optional */ } })(),
      (async () => { try { setVirtHost(await getHostVirtualization()) } catch { setVirtHost(null) } })(),
      (async () => { try { setLibSummary(await getLibvirtSummary()) } catch { setLibSummary(null) } })(),
      (async () => {
        try {
          const hp = await getHealthProblems()
          setHealthProblems(Array.isArray(hp.items) ? hp.items : [])
        } catch {
          setHealthProblems([])
        }
      })(),
      (async () => {
        try {
          const env = await getK8sEnvironment()
          if (env.kubectl_server_reachable) {
            setK8sOverview(await getK8sOverview())
            setK8sError(null)
          } else {
            setK8sOverview(null)
            setK8sError('kubectl cannot reach the API server — check kubeconfig on the host.')
          }
        } catch (e: unknown) {
          setK8sOverview(null)
          setK8sError(formatUserError(e))
        }
      })(),
      (async () => {
        try {
          const st = await getVesselStatus()
          setVesselStatus(st)
          if (st.connected) {
            const list = await listVesselContainers(true)
            setVesselContainerCount(list.length)
          } else {
            setVesselContainerCount(null)
          }
        } catch {
          setVesselStatus(null)
          setVesselContainerCount(null)
        }
      })(),
    ])
    setLoading(false)
  }, [])

  const loadMetrics = useCallback(async () => {
    try {
      const metrics = await getMetrics()
      const avgMem = metrics.length > 0
        ? metrics.reduce((sum: number, m: VmMetrics) => sum + m.memory_pct, 0) / metrics.length : 0
      setMetricsHistory((prev) => [
        ...prev.slice(-29),
        { time: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }), memory: parseFloat(avgMem.toFixed(1)) },
      ])
    } catch { /* no metrics */ }
  }, [])

  useEffect(() => {
    loadData(); loadMetrics()
    const interval = setInterval(() => { loadData(); loadMetrics() }, 10000)
    return () => clearInterval(interval)
  }, [loadData, loadMetrics])

  useEffect(() => {
    if (lastEvent && lastEvent.kind.startsWith('kubevirt.')) {
      loadData()
    }
  }, [refreshKey, lastEvent, loadData])

  useEffect(() => {
    const unsubscribe = subscribe(() => loadData())
    return () => unsubscribe()
  }, [subscribe, loadData])

  const handleHostShutdown = async () => {
    try { await hostShutdown() } catch (e: unknown) { toast.error(`Host shutdown failed: ${formatUserError(e)}`) }
    setShowShutdownConfirm(false)
  }

  const handleHostReboot = async () => {
    try { await hostReboot() } catch (e: unknown) { toast.error(`Host reboot failed: ${formatUserError(e)}`) }
    setShowRebootConfirm(false)
  }

  const running = vms.filter((v) => v.state === 'running').length
  const stopped = vms.filter((v) => v.state === 'shutoff').length
  const paused = vms.filter((v) => v.state === 'paused' || v.state === 'pmsuspended').length
  const totalVcpus = vms.reduce((s, v) => s + v.vcpus, 0)
  const totalMemGB = (vms.reduce((s, v) => s + v.memory_mb, 0) / 1024).toFixed(1)
  const activeNets = networks.filter((n) => n.active).length
  const activePools = pools.filter((p) => p.state === 'running').length
  const hostLine = node
    ? `${node.hypervisor} ${node.hypervisor_version}`
    : 'Host details unavailable — refresh if libvirt is reconnecting.'
  const exploreVisible =
    platformEnabled ||
    Boolean(k8sOverview || k8sError) ||
    Boolean(vesselStatus?.connected) ||
    Boolean(info?.guestkit?.enabled)

  if (loading) return <DashboardSkeleton />

  return (
    <PageLayout
      hideHeader
      className="min-w-0 !space-y-0"
      error={loadError}
      errorTitle="Could not load VMs"
      errorHints={loadError ? libvirtErrorHints(loadError) : undefined}
      onErrorRetry={loadData}
    >
      <div className="apple-story-stack w-full">
      <header className="apple-section apple-hero-band">
        <p className="apple-eyebrow">{node?.hostname ?? 'Hypervisor'}</p>
        <h1 className="apple-display">Machina</h1>
        <p className="apple-lede">{hostLine}</p>
        <div className="apple-cta-row">
          <Link to="/create" className="btn-primary inline-flex items-center gap-2">
            New VM
          </Link>
          <button
            type="button"
            onClick={() => void loadData().then(() => loadMetrics())}
            className="btn-secondary inline-flex items-center gap-2"
            title="Reload"
          >
            <RefreshCw className="w-4 h-4" />
            Refresh
          </button>
        </div>
      </header>

      {healthProblems.length > 0 && (
        <section className={`apple-section rounded-3xl px-6 py-7 sm:px-8 space-y-5 ${statusSurfaceClasses('error')}`}>
          <div className={`flex items-center gap-2 text-[17px] font-medium ${statusToneClass('error')}`}>
            <AlertTriangle className="w-5 h-5 shrink-0" aria-hidden />
            Host checklist ({healthProblems.length})
          </div>
          <ul className="space-y-5 text-[15px]">
            {healthProblems.map((p) => (
              <li key={p.id} className={`border-l-2 pl-4 ${statusBorderClass(p.severity === 'critical' ? 'error' : 'warn')}`}>
                <span className={`${statusToneClass(p.severity === 'critical' ? 'error' : 'warn')} font-medium`}>
                  {p.title}
                </span>
                {p.detail ? <p className={`text-sm mt-1.5 opacity-80 ${statusToneClass('error')}`}>{p.detail}</p> : null}
                {p.doc_url ? (
                  <a
                    href={p.doc_url}
                    target="_blank"
                    rel="noreferrer"
                    className={`text-sm hover:underline mt-1.5 inline-block ${statusActionLinkClasses('info')}`}
                  >
                    Documentation
                  </a>
                ) : null}
              </li>
            ))}
          </ul>
          <Link to="/system-check" className="apple-link text-[15px] inline-flex items-center gap-1.5">
            Run full system check
            <ChevronRight className="w-4 h-4" />
          </Link>
        </section>
      )}

      {!virtBannerDismissed && virtHost && (!virtHost.cpu_virt_supported || !virtHost.kvm_device_present || (!virtHost.libvirt_system_socket_present && !virtHost.libvirt_session_socket_present)) && (
        <section className={`rounded-3xl px-6 py-7 sm:px-8 flex flex-col sm:flex-row sm:items-start sm:justify-between gap-6 ${statusSurfaceClasses('warn')}`}>
          <div className="min-w-0 text-[15px]">
            <p className={`text-[17px] font-medium ${statusToneClass('warn')}`}>Virtualization readiness</p>
            <p className="mt-2 text-[var(--text-secondary)] leading-relaxed max-w-2xl">{virtHost.hint}</p>
            {libSummary?.dual_connection ? (
              <p className="mt-3 text-sm text-[var(--text-muted)]">
                Dual libvirt: system {libSummary.qemu_system_connected ? 'connected' : 'down'}, session {libSummary.qemu_session_connected ? 'connected' : 'down'} ({libSummary.configured_uri}).
              </p>
            ) : libSummary?.configured_uri ? (
              <p className="mt-3 text-sm text-[var(--text-muted)]">
                Libvirt URI: {libSummary.configured_uri}
              </p>
            ) : null}
          </div>
          <button
            type="button"
            onClick={() => {
              localStorage.setItem('machina_virt_banner_dismiss', '1')
              setVirtBannerDismissed(true)
            }}
            className={`shrink-0 self-start p-2 rounded-full hover:bg-black/10 ${statusToneClass('warn')}`}
            aria-label="Dismiss"
          >
            <X className="w-4 h-4" />
          </button>
        </section>
      )}

      <section aria-label="Fleet summary" className="apple-section apple-section--tight">
        <div className={METRIC_GRID}>
          <Metric
            figure={vms.length}
            label="Guests"
            hint={`${running} running${stopped ? ` · ${stopped} stopped` : ''}${paused ? ` · ${paused} paused` : ''}`}
          />
          <Metric
            figure={totalVcpus}
            label="vCPUs"
            hint={node ? `${node.cpu_cores}c / ${node.cpu_threads}t on host` : undefined}
          />
          <Metric
            figure={`${totalMemGB} GB`}
            label="Memory allocated"
            hint={node ? `${(node.memory_mb / 1024).toFixed(0)} GB host` : undefined}
          />
          <Metric
            figure={networks.length}
            label="Networks"
            hint={`${activeNets} active · ${activePools}/${pools.length} pools`}
          />
        </div>
      </section>

      {hostStats && (
        <section className="apple-section">
          <p className="apple-eyebrow">Host</p>
          <h2 className="apple-display apple-display--sm">This machine</h2>
          <p className="apple-lede">
            Uptime {formatUptime(hostStats.uptime_secs)} · {hostStats.processes} processes
            {node?.lib_version ? ` · libvirt v${node.lib_version}` : ''}
          </p>
          <div className="apple-cta-row">
            <button type="button" onClick={() => setShowRebootConfirm(true)} className="btn-secondary text-sm inline-flex items-center gap-2">
              <RotateCcw className="w-4 h-4" /> Reboot
            </button>
            <button type="button" onClick={() => setShowShutdownConfirm(true)} className="btn-destructive text-sm inline-flex items-center gap-2">
              <Power className="w-4 h-4" /> Shut Down
            </button>
          </div>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-12 md:gap-16 mt-10">
            <ResourceBar label="CPU" value={hostStats.cpu_percent} extra={`Load ${hostStats.load_1.toFixed(1)}`} />
            <ResourceBar label="Memory" value={hostStats.memory_percent} extra={`${(hostStats.memory_used_mb / 1024).toFixed(1)} / ${(hostStats.memory_total_mb / 1024).toFixed(1)} GB`} />
            <ResourceBar label="Disk" value={hostStats.disk_percent} extra={`${hostStats.disk_used_gb.toFixed(0)} / ${hostStats.disk_total_gb.toFixed(0)} GB`} />
          </div>
        </section>
      )}

      {exploreVisible && (
        <section className="apple-section" aria-label="Explore">
          <p className="apple-eyebrow">Explore</p>
          <h2 className="apple-display apple-display--sm">Go further</h2>
          <ul className="apple-dest-list">
            {platformEnabled && (
              <li>
                <Link to="/platform" className="apple-dest-row">
                  <span className="min-w-0">
                    <span className="apple-dest-title block">Platform</span>
                    <span className="apple-dest-sub block">Mission Control and fleet</span>
                  </span>
                  <span className="apple-dest-chevron" aria-hidden>›</span>
                </Link>
              </li>
            )}
            <li>
              <Link to="/vms" className="apple-dest-row">
                <span className="min-w-0">
                  <span className="apple-dest-title block">VM Center</span>
                  <span className="apple-dest-sub block">Local guests on this host</span>
                </span>
                <span className="apple-dest-chevron" aria-hidden>›</span>
              </Link>
            </li>
            {(k8sOverview || k8sError) && (
              <li>
                <Link to="/k8s" className="apple-dest-row">
                  <span className="min-w-0">
                    <span className="apple-dest-title block">
                      Kubernetes{k8sOverview ? ` · ${k8sOverview.ready_nodes}/${k8sOverview.nodes} nodes` : ''}
                    </span>
                    <span className="apple-dest-sub block">Cluster overview</span>
                  </span>
                  <span className="apple-dest-chevron" aria-hidden>›</span>
                </Link>
              </li>
            )}
            {vesselStatus?.connected && (
              <li>
                <Link to="/containers" className="apple-dest-row">
                  <span className="min-w-0">
                    <span className="apple-dest-title block">Containers · {vesselContainerCount ?? 0}</span>
                    <span className="apple-dest-sub block">Vessel runtime</span>
                  </span>
                  <span className="apple-dest-chevron" aria-hidden>›</span>
                </Link>
              </li>
            )}
            {Boolean(info?.guestkit?.enabled) && (
              <li>
                <Link to="/platform/migration?tab=jobs" className="apple-dest-row">
                  <span className="min-w-0">
                    <span className="apple-dest-title block">GuestKit</span>
                    <span className="apple-dest-sub block">Migration jobs</span>
                  </span>
                  <span className="apple-dest-chevron" aria-hidden>›</span>
                </Link>
              </li>
            )}
          </ul>
        </section>
      )}

      <section className="apple-section min-w-0 relative z-0">
        <p className="apple-eyebrow">Telemetry</p>
        <div className="flex items-end justify-between gap-4 mb-8">
          <h2 className="apple-display apple-display--sm">Memory</h2>
          <span className="text-[17px] tabular-nums text-[var(--text-muted)]">
            {metricsHistory.length > 0 ? `${metricsHistory[metricsHistory.length - 1].memory}%` : '—'}
          </span>
        </div>
        <div className="h-[280px] w-full min-w-0 isolate border-t border-b border-[var(--apple-hairline)] py-6">
          <ResponsiveContainer width="100%" height="100%">
            <AreaChart data={metricsHistory}>
              <defs>
                <linearGradient id="memGrad" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="5%" stopColor="var(--accent)" stopOpacity={0.2} />
                  <stop offset="95%" stopColor="var(--accent)" stopOpacity={0} />
                </linearGradient>
              </defs>
              <CartesianGrid strokeDasharray="3 3" stroke="var(--apple-hairline)" vertical={false} />
              <XAxis dataKey="time" stroke="var(--text-muted)" fontSize={12} tickLine={false} axisLine={false} />
              <YAxis stroke="var(--text-muted)" fontSize={12} tickLine={false} axisLine={false} domain={[0, 100]} width={36} />
              <Tooltip
                contentStyle={{
                  background: 'var(--apple-surface)',
                  border: '1px solid var(--apple-hairline)',
                  borderRadius: 12,
                  boxShadow: 'none',
                  color: 'var(--text-primary)',
                }}
                labelStyle={{ color: 'var(--text-muted)' }}
              />
              <Area type="monotone" dataKey="memory" stroke="var(--accent)" fill="url(#memGrad)" strokeWidth={2} />
            </AreaChart>
          </ResponsiveContainer>
        </div>
      </section>

      {/* Guests live in VM Center — home only surfaces counts + a deep link */}
      <section className="apple-section">
        <p className="apple-eyebrow">Inventory</p>
        <h2 className="apple-display apple-display--sm">Guests</h2>
        <p className="apple-lede">
          {vms.length === 0
            ? 'No guests on this host yet.'
            : `${running} running · ${stopped} stopped${paused ? ` · ${paused} paused` : ''} — manage them in VM Center.`}
        </p>
        <div className="apple-cta-row">
          <Link to="/vms" className="btn-primary inline-flex items-center gap-2 shrink-0">
            Open VM Center
            <ArrowRight className="w-4 h-4" />
          </Link>
        </div>
      </section>

      {events.length > 0 && (
        <section className="apple-section">
          <p className="apple-eyebrow">Live</p>
          <h2 className="apple-display apple-display--sm">Activity</h2>
          <ul className="mt-6 rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] divide-y divide-[var(--apple-hairline)] max-h-80 overflow-y-auto">
            {events.map((ev) => (
              <li key={`${ev.timestamp}-${ev.name}-${ev.event}`} className="px-6 sm:px-8 py-4 flex items-center justify-between gap-4 text-[15px]">
                <div className="flex items-center gap-3 min-w-0">
                  {ev.event === 'state_change' && <ArrowRight className={`w-4 h-4 shrink-0 ${statusToneClass('info')}`} />}
                  {ev.event === 'vm_added' && <Plus className={`w-4 h-4 shrink-0 ${statusToneClass('ok')}`} />}
                  {ev.event === 'vm_removed' && <Trash2 className={`w-4 h-4 shrink-0 ${statusToneClass('error')}`} />}
                  <span className="text-[var(--text-primary)] font-medium truncate">{ev.name}</span>
                  {ev.event === 'state_change' && (
                    <span className="text-[var(--text-muted)] shrink-0">{ev.old_state} → {ev.new_state}</span>
                  )}
                  {ev.event === 'vm_added' && <span className={`shrink-0 ${statusToneClass('ok')}`}>created</span>}
                  {ev.event === 'vm_removed' && <span className={`shrink-0 ${statusToneClass('error')}`}>removed</span>}
                </div>
                <span className="text-sm text-[var(--text-muted)] shrink-0">{timeAgo(ev.timestamp)}</span>
              </li>
            ))}
          </ul>
        </section>
      )}
      </div>

      {showShutdownConfirm && (
        <div className="fixed inset-0 bg-black/60 z-50 flex items-center justify-center animate-fade-in" onClick={() => setShowShutdownConfirm(false)}>
          <div role="dialog" aria-modal="true" aria-labelledby="host-shutdown-title" className="apple-surface-elevated p-8 max-w-md mx-4" onClick={e => e.stopPropagation()}>
            <h3 id="host-shutdown-title" className="text-[21px] font-semibold tracking-tight text-[var(--text-primary)] mb-3">Shut down this host?</h3>
            <p className="text-[15px] text-[var(--text-secondary)] leading-relaxed mb-8">All running VMs will stop and the system will power off.</p>
            <div className="flex justify-end gap-3">
              <button type="button" onClick={() => setShowShutdownConfirm(false)} className="btn-secondary text-sm">Cancel</button>
              <button type="button" onClick={() => void handleHostShutdown()} className="btn-destructive text-sm">Shut Down</button>
            </div>
          </div>
        </div>
      )}

      {showRebootConfirm && (
        <div className="fixed inset-0 bg-black/60 z-50 flex items-center justify-center animate-fade-in" onClick={() => setShowRebootConfirm(false)}>
          <div role="dialog" aria-modal="true" aria-labelledby="host-reboot-title" className="apple-surface-elevated p-8 max-w-md mx-4" onClick={e => e.stopPropagation()}>
            <h3 id="host-reboot-title" className="text-[21px] font-semibold tracking-tight text-[var(--text-primary)] mb-3">Reboot this host?</h3>
            <p className="text-[15px] text-[var(--text-secondary)] leading-relaxed mb-8">All running VMs will stop and the system will restart.</p>
            <div className="flex justify-end gap-3">
              <button type="button" onClick={() => setShowRebootConfirm(false)} className="btn-secondary text-sm">Cancel</button>
              <button type="button" onClick={() => void handleHostReboot()} className="btn-primary text-sm">Reboot</button>
            </div>
          </div>
        </div>
      )}
    </PageLayout>
  )
}

function Metric({ figure, label, hint }: { figure: string | number; label: string; hint?: string }) {
  return (
    <div className="min-w-0">
      <div className="apple-metric-value">{figure}</div>
      <div className="apple-metric-label">{label}</div>
      {hint && <div className="mt-1 text-[13px] text-[var(--text-muted)] leading-snug">{hint}</div>}
    </div>
  )
}

function ResourceBar({ label, value, extra }: { label: string; value: number; extra?: string }) {
  const tone = utilizationTone(value)
  /* Law 1: nominal readings stay graphite; color only when departing */
  const barClass = tone === 'ok' ? 'bg-[var(--mark-nominal,#646c7a)]' : statusBgClass(tone)
  return (
    <div className="min-w-0">
      <div className="flex items-baseline justify-between gap-3 mb-3">
        <span className="text-[17px] text-[var(--text-primary)] tracking-tight">{label}</span>
        <span className="text-[21px] font-semibold tabular-nums text-[var(--text-primary)]">{value.toFixed(0)}%</span>
      </div>
      <div className="w-full bg-[var(--mark-track,rgba(16,20,28,0.09))] rounded-full h-1">
        <div className={`${barClass} h-1 rounded-full transition-all`} style={{ width: `${Math.min(value, 100)}%` }} />
      </div>
      {extra && <div className="text-[13px] text-[var(--text-muted)] mt-3">{extra}</div>}
    </div>
  )
}

function formatUptime(secs: number): string {
  const days = Math.floor(secs / 86400)
  const hours = Math.floor((secs % 86400) / 3600)
  if (days > 0) return `${days}d ${hours}h`
  const mins = Math.floor((secs % 3600) / 60)
  return `${hours}h ${mins}m`
}

function DashboardSkeleton() {
  return (
    <div className="space-y-14 md:space-y-16 animate-fade-in">
      <div className="space-y-4 pt-4">
        <div className="h-4 w-32 skeleton rounded" />
        <div className="h-14 w-72 skeleton rounded" />
        <div className="h-6 w-96 max-w-full skeleton rounded" />
      </div>
      <div className={METRIC_GRID}>
        {[...Array(4)].map((_, i) => <div key={i} className="h-24 skeleton rounded" />)}
      </div>
      <div className="h-64 skeleton rounded-3xl" />
      <div className="h-80 skeleton rounded-3xl" />
    </div>
  )
}
