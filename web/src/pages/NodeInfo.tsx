// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, useCallback } from 'react'
import ConfirmDialog from '../components/ConfirmDialog'
import { Link } from 'react-router'
import { getNodeInfo, getHealth, NodeInfo, HealthStatus } from '../api/node'
import {
  getHostStats,
  getHostLinuxObservability,
  getHostLinuxAudit,
  type LinuxHostObservability,
  type LinuxAuditReport,
  HostStats,
  getSystemInfo,
  setHostname,
  setTimezone,
  SystemInfo,
  getHostFilesystems,
  getHostTopProcesses,
  postHostKillProcess,
  getHostPackageUpdates,
  postHostPackageUpgrade,
  postHostPackageInstall,
  postHostPackageRemove,
  postHostPackageAutoremove,
  getHostNetCounters,
  getHostNetRates,
  getHostPasswdUsers,
  getHostGroups,
  getHostSecuritySummary,
  getHardwareInventory,
  getHardwareInventoryHistory,
  type HardwareInventoryHistoryResponse,
  type HardwareInventoryReport,
  HostFilesystem,
  HostProcess,
  PackageUpdateCheck,
  PackageActionResult,
  NetDevCounter,
  NetDevRatesResponse,
  PasswdEntry,
  GroupEntry,
  HostSecuritySummary,
} from '../api/extras'
import { AreaChart, Area, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts'
import { Cpu, HardDrive, Server, CheckCircle, XCircle, RefreshCw, MemoryStick, Database, Monitor, Pencil, Check, X, FolderTree, ListOrdered, Package, Shield, Network, Users, UserSquare, Activity, ScrollText, ArrowUpCircle, PlusCircle, MinusCircle, Ban } from 'lucide-react'
import { formatBytes } from '../utils/vm'
import { ChoiceCardGrid, ChoiceLinkCard } from '../components/ChoiceCards'
import HostCockpitPanels from '../components/platform/HostCockpitPanels'
import { useToastContext } from '../contexts/ToastContext'
import { getSession, type SessionRole } from '../api/auth'
import { getHostLibvirtBoot, type LibvirtBootStatus } from '../api/host'
import { serviceAction } from '../api/extras'
import { formatUserError } from '../utils/apiError'
import { statusBadgeClasses, statusBorderClass, statusSurfaceClasses, statusToneClass, utilizationTone } from '../utils/semanticColors'
import { libvirtErrorHints } from '../utils/libvirtHints'
import PageLayout from '../components/PageLayout'
import ErrorBanner from '../components/ErrorBanner'
import EmptyState from '../components/EmptyState'
import { AppleStoryHeader } from '../components/platform/apple/AppleStoryKit'

interface StatsPoint { time: string; cpu: number; mem: number; disk: number; load: number }

function HostProcessTableBlock({
  variant,
  processes,
  canKillHostProcess,
  killBusyPid,
  onKill,
}: {
  variant: 'memory' | 'cpu'
  processes: HostProcess[]
  canKillHostProcess: boolean
  killBusyPid: number | null
  onKill: (p: HostProcess, signal: 'TERM' | 'KILL') => void
}) {
  const title =
    variant === 'memory' ? 'Top processes by memory' : 'Top processes by CPU'
  const hintLead =
    variant === 'memory'
      ? 'Highest RSS from ps; full argv from /proc (all Linux distros).'
      : 'Highest %CPU from ps; full argv from /proc (same columns as the memory list).'
  const headCpu = variant === 'cpu' ? 'text-[var(--accent)]/95 font-semibold' : ''
  const headRss = variant === 'memory' ? 'text-[var(--accent)]/95 font-semibold' : ''
  const cellCpu = variant === 'cpu' ? 'text-[var(--text-primary)]' : 'text-[var(--text-primary)]'
  const cellRss = variant === 'memory' ? 'text-[var(--text-primary)]' : 'text-[var(--text-primary)]'
  const headerIcon =
    variant === 'memory' ? (
      <ListOrdered className="w-5 h-5 text-[var(--accent)]" />
    ) : (
      <Cpu className={`w-5 h-5 ${statusToneClass('warn')}`} />
    )

  return (
    <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
      <div className="px-6 py-4 border-b border-[var(--apple-hairline)] flex items-center justify-between gap-3 flex-wrap">
        <h2 className="text-lg font-semibold flex items-center gap-2">
          {headerIcon}
          {title}
        </h2>
        <span className="text-xs text-[var(--text-muted)]">
          {hintLead}
          {canKillHostProcess
            ? ' Term / Kill send signals as the daemon user (root).'
            : ' Ending processes requires an admin session.'}
        </span>
      </div>
      <div className="overflow-x-auto">
        <table className="w-full text-sm" aria-label="Running processes">
          <thead>
            <tr className="text-left text-[var(--text-muted)] border-b border-[var(--apple-hairline)]">
              <th scope="col" className="px-6 py-3 font-medium">PID</th>
              <th scope="col" className="px-6 py-3 font-medium">User</th>
              <th scope="col" className={`px-6 py-3 font-medium text-right ${headCpu}`}>CPU%</th>
              <th scope="col" className={`px-6 py-3 font-medium text-right ${headRss}`}>RSS</th>
              <th scope="col" className="px-6 py-3 font-medium">Comm</th>
              <th scope="col" className="px-6 py-3 font-medium hidden xl:table-cell">Command line</th>
              {canKillHostProcess && (
                <th scope="col" className="px-6 py-3 font-medium text-right">Actions</th>
              )}
            </tr>
          </thead>
          <tbody className="divide-y divide-[var(--apple-hairline)]/50">
            {processes.map((p) => (
              <tr key={`${variant}-${p.pid}`} className="hover:bg-[var(--surface-hover)]/30">
                <td className="px-6 py-3 font-mono text-[var(--text-secondary)]">{p.pid}</td>
                <td className="px-6 py-3 text-[var(--text-secondary)]">{p.user}</td>
                <td className={`px-6 py-3 text-right ${cellCpu}`}>{p.cpu_percent.toFixed(1)}</td>
                <td className={`px-6 py-3 text-right ${cellRss}`}>{formatBytes(p.rss_kb * 1024)}</td>
                <td className="px-6 py-3 font-mono text-xs text-[var(--text-muted)] truncate max-w-[10rem]" title={p.command}>{p.command}</td>
                <td className="px-6 py-3 font-mono text-xs text-[var(--text-muted)] truncate max-w-xl hidden xl:table-cell" title={p.args || ''}>{p.args || '—'}</td>
                {canKillHostProcess && (
                  <td className="px-6 py-3 text-right whitespace-nowrap">
                    <div className="inline-flex flex-wrap items-center justify-end gap-1">
                      <button
                        type="button"
                        title="Send SIGTERM"
                        disabled={killBusyPid !== null}
                        onClick={() => void onKill(p, 'TERM')}
                        className="inline-flex items-center gap-1 px-2 py-1 rounded text-xs font-medium bg-[var(--apple-fill-tertiary)] text-[var(--text-primary)] border border-[var(--apple-hairline)] hover:bg-[var(--surface-hover)] disabled:opacity-50 transition"
                      >
                        Term
                      </button>
                      <button
                        type="button"
                        title="Send SIGKILL"
                        disabled={killBusyPid !== null}
                        onClick={() => void onKill(p, 'KILL')}
                        className="inline-flex items-center gap-1 px-2 py-1 rounded text-xs font-medium bg-[var(--nl-status-danger-bg)] text-[var(--nl-accent-red-text)] border border-[var(--nl-status-danger-border)] hover:opacity-80 disabled:opacity-50 transition"
                      >
                        <Ban className="w-3.5 h-3.5 shrink-0" />
                        Kill
                      </button>
                    </div>
                  </td>
                )}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

export default function NodeInfoPage() {
  const toast = useToastContext()
  const [node, setNode] = useState<NodeInfo | null>(null)
  const [health, setHealth] = useState<HealthStatus | null>(null)
  const [stats, setStats] = useState<HostStats | null>(null)
  const [history, setHistory] = useState<StatsPoint[]>([])
  const [loading, setLoading] = useState(true)
  const [sysInfo, setSysInfo] = useState<SystemInfo | null>(null)
  const [filesystems, setFilesystems] = useState<HostFilesystem[]>([])
  const [topProcesses, setTopProcesses] = useState<HostProcess[]>([])
  const [topProcessesByCpu, setTopProcessesByCpu] = useState<HostProcess[]>([])
  const [pkgUpdates, setPkgUpdates] = useState<PackageUpdateCheck | null>(null)
  const [pkgMutBusy, setPkgMutBusy] = useState(false)
  const [pkgInstallInput, setPkgInstallInput] = useState('')
  const [pkgRemoveInput, setPkgRemoveInput] = useState('')
  const [pkgRemovePurge, setPkgRemovePurge] = useState(false)
  const [pkgActionResult, setPkgActionResult] = useState<PackageActionResult | null>(null)
  const [netCounters, setNetCounters] = useState<NetDevCounter[]>([])
  const [netRates, setNetRates] = useState<NetDevRatesResponse | null>(null)
  const [netRatesLoading, setNetRatesLoading] = useState(false)
  const [rateSampleMs, setRateSampleMs] = useState(1000)
  const [passwdUsers, setPasswdUsers] = useState<PasswdEntry[]>([])
  const [groups, setGroups] = useState<GroupEntry[]>([])
  const [securitySummary, setSecuritySummary] = useState<HostSecuritySummary | null>(null)
  const [showSystemAccounts, setShowSystemAccounts] = useState(false)
  const [editingHostname, setEditingHostname] = useState(false)
  const [editingTimezone, setEditingTimezone] = useState(false)
  const [hostnameInput, setHostnameInput] = useState('')
  const [timezoneInput, setTimezoneInput] = useState('')
  const [savingHostname, setSavingHostname] = useState(false)
  const [savingTimezone, setSavingTimezone] = useState(false)
  const [sessionRole, setSessionRole] = useState<SessionRole | null>(null)
  const [killBusyPid, setKillBusyPid] = useState<number | null>(null)
  const [pkgConfirmOp, setPkgConfirmOp] = useState<'upgrade' | 'autoremove' | 'remove' | null>(null)
  const [killConfirm, setKillConfirm] = useState<{ process: HostProcess; signal: 'TERM' | 'KILL' } | null>(null)
  const [libvirtBoot, setLibvirtBoot] = useState<LibvirtBootStatus | null>(null)
  const [libvirtBootBusy, setLibvirtBootBusy] = useState(false)
  const [hardwareInventory, setHardwareInventory] = useState<HardwareInventoryReport | null>(null)
  const [hardwareInventoryHistory, setHardwareInventoryHistory] =
    useState<HardwareInventoryHistoryResponse | null>(null)
  const [linuxObs, setLinuxObs] = useState<LinuxHostObservability | null>(null)
  const [linuxAudit, setLinuxAudit] = useState<LinuxAuditReport | null>(null)

  const canKillHostProcess = sessionRole === 'admin'

  const load = useCallback(() => {
    Promise.allSettled([
      getNodeInfo(),
      getHealth(),
      getHostStats(),
      getSystemInfo(),
      getHostFilesystems(),
      getHostTopProcesses(20, { sort: 'rss' }),
      getHostTopProcesses(20, { sort: 'cpu' }),
      getHostPackageUpdates(),
      getHostNetCounters(),
      getHostPasswdUsers(200),
      getHostGroups(200),
      getHostSecuritySummary(),
      getHostLibvirtBoot(),
      getHardwareInventory(),
      getHardwareInventoryHistory(48),
      getHostLinuxObservability(),
      getHostLinuxAudit(),
    ])
      .then(([n, h, s, si, fs, tp, tpc, pk, nc, pw, gr, sec, lb, hi, hh, lo, la]) => {
        if (n.status === 'fulfilled') setNode(n.value)
        if (h.status === 'fulfilled') setHealth(h.value)
        if (si.status === 'fulfilled') setSysInfo(si.value)
        if (fs.status === 'fulfilled') setFilesystems(fs.value)
        else setFilesystems([])
        if (tp.status === 'fulfilled') setTopProcesses(tp.value)
        else setTopProcesses([])
        if (tpc.status === 'fulfilled') setTopProcessesByCpu(tpc.value)
        else setTopProcessesByCpu([])
        if (pk.status === 'fulfilled') {
          setPkgUpdates(pk.value)
        } else {
          setPkgUpdates({
            backend: 'unknown',
            probed: false,
            pending_count: null,
            summary: null,
            hint: 'Could not load package probe (non-Linux UI build, or API error).',
            error: pk.status === 'rejected' ? formatUserError(pk.reason) : null,
            reboot_required: false,
          })
        }
        if (nc.status === 'fulfilled') setNetCounters(nc.value)
        else setNetCounters([])
        if (pw.status === 'fulfilled') setPasswdUsers(pw.value)
        else setPasswdUsers([])
        if (gr.status === 'fulfilled') setGroups(gr.value)
        else setGroups([])
        if (sec.status === 'fulfilled') setSecuritySummary(sec.value)
        else setSecuritySummary(null)
        if (lb.status === 'fulfilled') setLibvirtBoot(lb.value)
        else setLibvirtBoot(null)
        if (hi.status === 'fulfilled') setHardwareInventory(hi.value)
        else setHardwareInventory(null)
        if (hh.status === 'fulfilled') setHardwareInventoryHistory(hh.value)
        else setHardwareInventoryHistory(null)
        if (lo.status === 'fulfilled') setLinuxObs(lo.value)
        else setLinuxObs(null)
        if (la.status === 'fulfilled') setLinuxAudit(la.value)
        else setLinuxAudit(null)
        if (s.status === 'fulfilled') {
          setStats(s.value)
          const time = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
          setHistory(prev => [...prev.slice(-59), {
            time, cpu: parseFloat(s.value.cpu_percent.toFixed(1)),
            mem: parseFloat(s.value.memory_percent.toFixed(1)),
            disk: parseFloat(s.value.disk_percent.toFixed(1)),
            load: s.value.load_1,
          }])
        }
      })
      .finally(() => setLoading(false))
  }, [])

  const parsePackageList = (raw: string): string[] => {
    const parts = raw.split(/[\s,]+/).map((s) => s.trim()).filter(Boolean)
    return [...new Set(parts)]
  }

  const runPackageUpgrade = useCallback(() => {
    if (!pkgUpdates || pkgUpdates.backend === 'unknown') return
    setPkgConfirmOp('upgrade')
  }, [pkgUpdates])

  const doRunPackageUpgrade = useCallback(async () => {
    setPkgMutBusy(true)
    setPkgActionResult(null)
    try {
      const r = await postHostPackageUpgrade()
      setPkgActionResult(r)
      if (r.ok) toast.success('Package upgrade completed.')
      else toast.error(`Package upgrade finished with errors (exit ${r.exit_code}).`)
      try {
        setPkgUpdates(await getHostPackageUpdates())
      } catch {
        /* keep prior probe */
      }
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setPkgMutBusy(false)
    }
  }, [toast])

  const runPackageUpgradeDryRun = useCallback(async () => {
    if (!pkgUpdates || pkgUpdates.backend === 'unknown') return
    setPkgMutBusy(true)
    setPkgActionResult(null)
    try {
      const r = await postHostPackageUpgrade({ dry_run: true })
      setPkgActionResult(r)
      if (r.ok) toast.success('Upgrade dry-run finished (no packages were changed).')
      else toast.error(`Upgrade preview exited with code ${r.exit_code}.`)
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setPkgMutBusy(false)
    }
  }, [pkgUpdates, toast])

  const runPackageAutoremove = useCallback(() => {
    if (!pkgUpdates || pkgUpdates.backend !== 'apt') {
      toast.error('Autoremove is only available when the package backend is apt.')
      return
    }
    setPkgConfirmOp('autoremove')
  }, [pkgUpdates, toast])

  const doRunPackageAutoremove = useCallback(async () => {
    setPkgMutBusy(true)
    setPkgActionResult(null)
    try {
      const r = await postHostPackageAutoremove()
      setPkgActionResult(r)
      if (r.ok) toast.success('Autoremove completed.')
      else toast.error(`Autoremove finished with errors (exit ${r.exit_code}).`)
      try {
        setPkgUpdates(await getHostPackageUpdates())
      } catch {
        /* ignore */
      }
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setPkgMutBusy(false)
    }
  }, [toast])

  const runPackageInstall = useCallback(async () => {
    if (!pkgUpdates || pkgUpdates.backend === 'unknown') return
    const pkgs = parsePackageList(pkgInstallInput)
    if (pkgs.length === 0) {
      toast.error('Enter at least one package name.')
      return
    }
    setPkgMutBusy(true)
    setPkgActionResult(null)
    try {
      const r = await postHostPackageInstall(pkgs)
      setPkgActionResult(r)
      if (r.ok) toast.success(`Installed: ${pkgs.join(', ')}`)
      else toast.error(`Install failed (exit ${r.exit_code}).`)
      try {
        setPkgUpdates(await getHostPackageUpdates())
      } catch {
        /* ignore */
      }
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setPkgMutBusy(false)
    }
  }, [pkgInstallInput, pkgUpdates, toast])

  const runPackageRemove = useCallback(() => {
    if (!pkgUpdates || pkgUpdates.backend === 'unknown') return
    const pkgs = parsePackageList(pkgRemoveInput)
    if (pkgs.length === 0) {
      toast.error('Enter at least one package name to remove.')
      return
    }
    setPkgConfirmOp('remove')
  }, [pkgRemoveInput, pkgUpdates, toast])

  const doRunPackageRemove = useCallback(async () => {
    const pkgs = parsePackageList(pkgRemoveInput)
    setPkgMutBusy(true)
    setPkgActionResult(null)
    try {
      const r = await postHostPackageRemove(pkgs, { purge: pkgRemovePurge })
      setPkgActionResult(r)
      if (r.ok) toast.success(pkgRemovePurge ? `Purged: ${pkgs.join(', ')}` : `Removed: ${pkgs.join(', ')}`)
      else toast.error(`Remove failed (exit ${r.exit_code}).`)
      try {
        setPkgUpdates(await getHostPackageUpdates())
      } catch {
        /* ignore */
      }
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setPkgMutBusy(false)
    }
  }, [pkgRemoveInput, pkgRemovePurge, toast])

  const measureNetRates = useCallback(async () => {
    setNetRatesLoading(true)
    try {
      const r = await getHostNetRates(rateSampleMs)
      setNetRates(r)
    } catch (e: unknown) {
      toast.error(`Bandwidth sample failed: ${formatUserError(e)}`)
    } finally {
      setNetRatesLoading(false)
    }
  }, [rateSampleMs, toast])

  const killHostProcess = useCallback(
    (p: HostProcess, signal: 'TERM' | 'KILL') => {
      if (!canKillHostProcess) return
      setKillConfirm({ process: p, signal })
    },
    [canKillHostProcess],
  )

  const doKillHostProcess = useCallback(
    async (p: HostProcess, signal: 'TERM' | 'KILL') => {
      setKillBusyPid(p.pid)
      try {
        await postHostKillProcess(p.pid, { signal })
        toast.success(
          signal === 'KILL' ? `SIGKILL sent to PID ${p.pid}` : `SIGTERM sent to PID ${p.pid}`,
        )
        try {
          const [mem, cpu] = await Promise.all([
            getHostTopProcesses(20, { sort: 'rss' }),
            getHostTopProcesses(20, { sort: 'cpu' }),
          ])
          setTopProcesses(mem)
          setTopProcessesByCpu(cpu)
        } catch {
          /* ignore refresh failure */
        }
      } catch (e) {
        toast.error(formatUserError(e))
      } finally {
        setKillBusyPid(null)
      }
    },
    [toast],
  )

  const enableLibvirtBootUnit = useCallback(async () => {
    if (!libvirtBoot?.needs_attention || !libvirtBoot.systemd_unit) return
    setLibvirtBootBusy(true)
    try {
      await serviceAction(libvirtBoot.systemd_unit, 'enable_now')
      toast.success(
        `Enabled and started ${libvirtBoot.systemd_unit}. If other tabs stall briefly, libvirt is reconnecting — wait a few seconds or refresh.`,
      )
      try {
        setLibvirtBoot(await getHostLibvirtBoot())
      } catch {
        setLibvirtBoot(null)
      }
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setLibvirtBootBusy(false)
    }
  }, [libvirtBoot, toast])

  useEffect(() => {
    getSession()
      .then((s) => {
        if (s.authenticated) setSessionRole(s.role ?? 'admin')
        else setSessionRole(null)
      })
      .catch(() => setSessionRole(null))
  }, [])

  useEffect(() => {
    load()
    const interval = setInterval(load, 5000)
    return () => clearInterval(interval)
  }, [load])

  if (!loading && !node) {
    return (
      <EmptyState
        title="Could not load host info"
        description="The hypervisor agent may be offline or libvirt is unreachable on this node."
        primaryAction={
          <button type="button" className="btn-primary text-sm" onClick={() => void load()}>
            Retry
          </button>
        }
        secondaryAction={
          <Link to="/node" className="btn-secondary text-sm">Node tools</Link>
        }
      />
    )
  }

  const formatUptime = (secs: number) => {
    const d = Math.floor(secs / 86400)
    const h = Math.floor((secs % 86400) / 3600)
    const m = Math.floor((secs % 3600) / 60)
    return d > 0 ? `${d}d ${h}h ${m}m` : `${h}h ${m}m`
  }

  return (
    <PageLayout
      hideHeader
      className="min-w-0 !space-y-0"
      loading={loading}
      error={undefined}
    >
      {node && (
      <>
      {libvirtBoot?.needs_attention && libvirtBoot.detail && (
        <div className={`rounded-xl px-4 py-3 flex flex-col sm:flex-row sm:items-start sm:justify-between gap-3 ${statusSurfaceClasses('warn')}`}>
          <div className="min-w-0">
            <p className={`text-sm font-medium ${statusToneClass('warn')}`}>Libvirt at host boot</p>
            <p className={`text-xs mt-1 leading-relaxed opacity-90 ${statusToneClass('warn')}`}>{libvirtBoot.detail}</p>
          </div>
          {libvirtBoot.systemd_unit ? (
            <button
              type="button"
              disabled={libvirtBootBusy}
              onClick={() => void enableLibvirtBootUnit()}
              className={`shrink-0 px-3 py-1.5 rounded-lg text-sm font-medium border disabled:opacity-50 transition ${statusBadgeClasses('warn')} border-[color-mix(in_srgb,var(--machina-status-warn)_40%,transparent)] hover:bg-[color-mix(in_srgb,var(--machina-status-warn)_15%,transparent)]`}
            >
              {libvirtBootBusy ? 'Running…' : `Enable at boot (${libvirtBoot.systemd_unit})`}
            </button>
          ) : null}
        </div>
      )}

      {health && !health.libvirt && (
        <ErrorBanner message={`Libvirt is not healthy: ${health.status}`} hints={libvirtErrorHints(health.status)} />
      )}

      {/* Health Status */}
      {health && (
        <div className={`flex items-center gap-3 p-4 rounded-xl border ${statusSurfaceClasses(health.libvirt ? 'ok' : 'error')}`}>
          {health.libvirt ? <CheckCircle className={`w-5 h-5 ${statusToneClass('ok')}`} /> : <XCircle className={`w-5 h-5 ${statusToneClass('error')}`} />}
          <span className="text-sm">Virtualization (libvirt): <strong className={statusToneClass(health.libvirt ? 'ok' : 'error')}>{health.status}</strong></span>
          <span className="text-xs text-[var(--text-muted)] ml-auto">{node.hypervisor} {node.hypervisor_version} / libvirt {node.lib_version}</span>
        </div>
      )}

      <div className="apple-story-stack w-full space-y-8">
        <AppleStoryHeader
          centered
          eyebrow="Hypervisor"
          title={node.hostname}
          lede={`${node.hypervisor} ${node.hypervisor_version} · libvirt ${node.lib_version} · ${node.active_vms} active / ${node.defined_vms} defined VMs`}
          cta={
            <button
              type="button"
              onClick={load}
              className="btn-secondary inline-flex items-center gap-2"
              aria-label="Refresh"
            >
              <RefreshCw className="w-4 h-4" />
              Refresh
            </button>
          }
        />

        {stats && (
          <section aria-label="Host utilization" className="apple-section apple-section--tight">
            <div className="apple-metric-band">
              <div className="min-w-0">
                <div className="apple-metric-value">{stats.cpu_percent.toFixed(0)}%</div>
                <div className="apple-metric-label">CPU</div>
                <div className="mt-1 text-[13px] text-[var(--text-muted)] leading-snug">
                  Load {stats.load_1.toFixed(2)} / {stats.load_5.toFixed(2)} / {stats.load_15.toFixed(2)}
                </div>
              </div>
              <div className="min-w-0">
                <div className="apple-metric-value">{stats.memory_percent.toFixed(0)}%</div>
                <div className="apple-metric-label">Memory</div>
                <div className="mt-1 text-[13px] text-[var(--text-muted)] leading-snug">
                  {(stats.memory_used_mb / 1024).toFixed(1)} / {(stats.memory_total_mb / 1024).toFixed(1)} GB
                </div>
              </div>
              <div className="min-w-0">
                <div className="apple-metric-value">{stats.disk_percent.toFixed(0)}%</div>
                <div className="apple-metric-label">Disk</div>
                <div className="mt-1 text-[13px] text-[var(--text-muted)] leading-snug">
                  {stats.disk_used_gb.toFixed(0)} / {stats.disk_total_gb.toFixed(0)} GB
                </div>
              </div>
              <div className="min-w-0">
                <div className="apple-metric-value">
                  {stats.swap_total_mb > 0 ? ((stats.swap_used_mb / stats.swap_total_mb) * 100).toFixed(0) : 0}%
                </div>
                <div className="apple-metric-label">Swap</div>
                <div className="mt-1 text-[13px] text-[var(--text-muted)] leading-snug">
                  {(stats.swap_used_mb / 1024).toFixed(1)} / {(stats.swap_total_mb / 1024).toFixed(1)} GB
                </div>
              </div>
              <div className="min-w-0">
                <div className="apple-metric-value">{formatUptime(stats.uptime_secs)}</div>
                <div className="apple-metric-label">Uptime</div>
                <div className="mt-1 text-[13px] text-[var(--text-muted)] leading-snug">{stats.processes} processes</div>
              </div>
            </div>
          </section>
        )}
      </div>

      {/* System Info — was below gauges; health banner stays above story */}
      {sysInfo && (
        <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--apple-hairline)] space-y-3">
          <h2 className="text-lg font-semibold flex items-center gap-2"><Monitor className="w-5 h-5 text-[var(--accent)]" /> System Configuration</h2>
          <InfoRow label="OS" value={sysInfo.os_pretty_name || `${sysInfo.os_name} ${sysInfo.os_version}`} />
          <InfoRow label="Kernel" value={sysInfo.kernel_version} />
          {sysInfo.architecture && <InfoRow label="Architecture" value={sysInfo.architecture} />}
          {sysInfo.hardware_model && <InfoRow label="Hardware model" value={sysInfo.hardware_model} />}
          {sysInfo.firmware_version && <InfoRow label="Firmware (hostnamectl)" value={sysInfo.firmware_version} />}
          {sysInfo.systemd_version && <InfoRow label="systemd" value={sysInfo.systemd_version} />}
          {sysInfo.boot_duration && <InfoRow label="Boot duration" value={sysInfo.boot_duration} />}
          {sysInfo.boot_time && <InfoRow label="Boot time" value={sysInfo.boot_time} />}
          {sysInfo.local_time && <InfoRow label="Local time" value={sysInfo.local_time} />}
          {sysInfo.universal_time && <InfoRow label="Universal time" value={sysInfo.universal_time} />}
          {sysInfo.rtc_time && <InfoRow label="RTC time" value={sysInfo.rtc_time} />}
          {sysInfo.rtc_in_local_tz && <InfoRow label="RTC in local TZ" value={sysInfo.rtc_in_local_tz} />}
          {sysInfo.ntp_service && <InfoRow label="NTP service" value={sysInfo.ntp_service} />}
          <InfoRow label="Clock sync" value={sysInfo.system_clock_synchronized ? 'yes' : 'no'} />
          {sysInfo.systemctl_is_system_running && (
            <InfoRow label="systemctl is-system-running" value={sysInfo.systemctl_is_system_running} />
          )}
          <InfoRow label="Logged-in users" value={sysInfo.logged_in_users} />
          {sysInfo.cpu_model && <InfoRow label="CPU Model" value={sysInfo.cpu_model} />}
          {sysInfo.sys_vendor && <InfoRow label="Vendor" value={sysInfo.sys_vendor} />}
          {sysInfo.product_name && <InfoRow label="Product" value={sysInfo.product_name} />}
          {sysInfo.board_name && <InfoRow label="Board" value={sysInfo.board_name} />}
          {sysInfo.bios_version && <InfoRow label="BIOS" value={`${sysInfo.bios_version} (${sysInfo.bios_date})`} />}
          {sysInfo.serial_number && sysInfo.serial_number !== 'None' && <InfoRow label="Serial" value={sysInfo.serial_number} />}
          {sysInfo.virtualization && sysInfo.virtualization !== 'none' && <InfoRow label="Virtualization" value={sysInfo.virtualization} />}
          {sysInfo.pretty_hostname && <InfoRow label="Pretty hostname" value={sysInfo.pretty_hostname} />}
          {sysInfo.transient_hostname && <InfoRow label="Transient hostname" value={sysInfo.transient_hostname} />}
          {sysInfo.icon_name && <InfoRow label="Icon name" value={sysInfo.icon_name} />}
          {sysInfo.chassis && <InfoRow label="Chassis" value={sysInfo.chassis} />}
          {sysInfo.deployment && <InfoRow label="Deployment" value={sysInfo.deployment} />}
          {sysInfo.location && <InfoRow label="Location" value={sysInfo.location} />}
          {sysInfo.machine_id && <InfoRow label="Machine ID" value={sysInfo.machine_id} />}
          {sysInfo.boot_id && <InfoRow label="Boot ID" value={sysInfo.boot_id} />}

          {/* Editable Hostname */}
          <div className="flex items-center justify-between py-2 border-b border-[var(--apple-hairline)]/30">
            <span className="text-[var(--text-muted)] text-sm">Hostname</span>
            {editingHostname ? (
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  aria-label="Hostname"
                  value={hostnameInput}
                  onChange={e => setHostnameInput(e.target.value)}
                  className="bg-[var(--surface-hover)] border border-[var(--apple-hairline)] rounded px-2 py-1 text-sm focus:outline-none focus-visible:ring-1 focus-visible:ring-[var(--machina-status-info)]"
                  autoFocus
                />
                <button
                  disabled={savingHostname}
                  onClick={async () => {
                    setSavingHostname(true)
                    try {
                      await setHostname(hostnameInput)
                      toast.success('Hostname updated')
                      setEditingHostname(false)
                      load()
                    } catch (e) {
                      toast.error(formatUserError(e))
                    } finally {
                      setSavingHostname(false)
                    }
                  }}
                  className={`p-1 rounded hover:bg-green-500/20 disabled:opacity-40 ${statusToneClass('ok')}`}
                  aria-label="Save hostname"
                ><Check className="w-4 h-4" /></button>
                <button onClick={() => setEditingHostname(false)} className={`p-1 rounded hover:bg-[color-mix(in_srgb,var(--machina-status-error)_25%,transparent)] ${statusToneClass('error')}`} aria-label="Cancel"><X className="w-4 h-4" /></button>
              </div>
            ) : (
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium">{sysInfo.hostname}</span>
                <button aria-label="Edit hostname" onClick={() => { setHostnameInput(sysInfo.hostname); setEditingHostname(true) }} className="p-1 hover:bg-[var(--surface-hover)] rounded text-[var(--text-muted)] hover:text-[var(--text-primary)] transition"><Pencil className="w-3.5 h-3.5" /></button>
              </div>
            )}
          </div>

          {/* Editable Timezone */}
          <div className="flex items-center justify-between py-2 border-b border-[var(--apple-hairline)]/30">
            <span className="text-[var(--text-muted)] text-sm">Timezone</span>
            {editingTimezone ? (
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  aria-label="Timezone"
                  value={timezoneInput}
                  onChange={e => setTimezoneInput(e.target.value)}
                  placeholder="e.g. America/New_York"
                  className="bg-[var(--surface-hover)] border border-[var(--apple-hairline)] rounded px-2 py-1 text-sm focus:outline-none focus-visible:ring-1 focus-visible:ring-[var(--machina-status-info)]"
                  autoFocus
                />
                <button
                  disabled={savingTimezone}
                  onClick={async () => {
                    setSavingTimezone(true)
                    try {
                      await setTimezone(timezoneInput)
                      toast.success('Timezone updated')
                      setEditingTimezone(false)
                      load()
                    } catch (e) {
                      toast.error(formatUserError(e))
                    } finally {
                      setSavingTimezone(false)
                    }
                  }}
                  className={`p-1 rounded hover:bg-green-500/20 disabled:opacity-40 ${statusToneClass('ok')}`}
                  aria-label="Save timezone"
                ><Check className="w-4 h-4" /></button>
                <button onClick={() => setEditingTimezone(false)} className={`p-1 rounded hover:bg-[color-mix(in_srgb,var(--machina-status-error)_25%,transparent)] ${statusToneClass('error')}`} aria-label="Cancel"><X className="w-4 h-4" /></button>
              </div>
            ) : (
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium">{sysInfo.timezone}</span>
                <button aria-label="Edit timezone" onClick={() => { setTimezoneInput(sysInfo.timezone); setEditingTimezone(true) }} className="p-1 hover:bg-[var(--surface-hover)] rounded text-[var(--text-muted)] hover:text-[var(--text-primary)] transition"><Pencil className="w-3.5 h-3.5" /></button>
              </div>
            )}
          </div>
        </div>
      )}

      {sysInfo && (
        <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--apple-hairline)] space-y-3">
          <h2 className="text-lg font-semibold flex items-center gap-2">
            <Activity className={`w-5 h-5 ${statusToneClass('ok')}`} /> Raw systemd diagnostics
          </h2>
          <p className="text-xs text-[var(--text-muted)]">
            Extended snapshot: raw <code className="text-[var(--text-muted)]">hostnamectl</code>/<code className="text-[var(--text-muted)]">timedatectl</code>,{' '}
            <code className="text-[var(--text-muted)]">resolvectl</code>, sockets/timers/jobs, unit lists, target dependencies,{' '}
            <code className="text-[var(--text-muted)]">journalctl --list-boots</code>, and more (truncated server-side).
          </p>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]" open>
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemctl --version (full)</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--accent)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-72 overflow-y-auto">
              {(sysInfo.systemd_version_full || sysInfo.systemd_version || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]" open>
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemd-analyze critical-chain (top)</summary>
            <div className="px-3 pb-3 border-t border-[var(--apple-hairline)]/30 pt-2 space-y-1 max-h-56 overflow-y-auto">
              {sysInfo.critical_chain_top?.length ? sysInfo.critical_chain_top.map((line, idx) => (
                <pre key={`${idx}-${line}`} className="text-xs text-[var(--text-secondary)] whitespace-pre-wrap break-words font-mono">{line}</pre>
              )) : <div className="text-xs text-[var(--text-muted)]">No critical-chain data.</div>}
            </div>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemd-analyze blame (top)</summary>
            <div className="px-3 pb-3 border-t border-[var(--apple-hairline)]/30 pt-2 space-y-0.5 max-h-72 overflow-y-auto">
              {sysInfo.systemd_analyze_blame_top?.length ? sysInfo.systemd_analyze_blame_top.map((line, idx) => (
                <pre key={`${idx}-${line}`} className="text-xs text-[var(--text-secondary)] whitespace-pre-wrap break-words font-mono">{line}</pre>
              )) : <div className="text-xs text-[var(--text-muted)]">No blame data.</div>}
            </div>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">loginctl list-users</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-48 overflow-y-auto">
              {(sysInfo.loginctl_users_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">loginctl list-sessions</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-64 overflow-y-auto">
              {(sysInfo.loginctl_sessions_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemctl show (manager, truncated)</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-80 overflow-y-auto">
              {(sysInfo.systemctl_show_manager || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemctl list-units --type=mount --state=active</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-64 overflow-y-auto">
              {(sysInfo.mount_units_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemctl list-units --state=failed</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-48 overflow-y-auto">
              {(sysInfo.failed_units_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemctl list-dependencies systemd-networkd</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-64 overflow-y-auto">
              {(sysInfo.networkd_dependencies_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">hostnamectl (raw)</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-72 overflow-y-auto">
              {(sysInfo.hostnamectl_status_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">timedatectl (raw)</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-72 overflow-y-auto">
              {(sysInfo.timedatectl_status_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">timedatectl show</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-64 overflow-y-auto">
              {(sysInfo.timedatectl_show_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemctl show-environment</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-64 overflow-y-auto">
              {(sysInfo.systemctl_show_environment_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemctl list-sockets</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-64 overflow-y-auto">
              {(sysInfo.systemctl_list_sockets_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemctl list-timers --all</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-64 overflow-y-auto">
              {(sysInfo.systemctl_list_timers_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemctl list-jobs</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-48 overflow-y-auto">
              {(sysInfo.systemctl_list_jobs_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemctl status systemd-networkd</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-72 overflow-y-auto">
              {(sysInfo.systemctl_status_networkd_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemctl status systemd-resolved</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-72 overflow-y-auto">
              {(sysInfo.systemctl_status_resolved_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemctl list-dependencies systemd-resolved</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-64 overflow-y-auto">
              {(sysInfo.resolved_dependencies_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">resolvectl status</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-80 overflow-y-auto">
              {(sysInfo.resolvectl_status_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">resolvectl statistics</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-48 overflow-y-auto">
              {(sysInfo.resolvectl_statistics_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">bootctl status</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-64 overflow-y-auto">
              {(sysInfo.bootctl_status_text || '').trim() || 'No output (not using systemd-boot or bootctl missing).'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemctl list-unit-files (enabled services)</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-72 overflow-y-auto">
              {(sysInfo.enabled_service_unit_files_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemctl list-units (running services)</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-72 overflow-y-auto">
              {(sysInfo.running_service_units_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">loginctl list-seats</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-40 overflow-y-auto">
              {(sysInfo.loginctl_list_seats_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">journalctl --list-boots</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-56 overflow-y-auto">
              {(sysInfo.journalctl_list_boots_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemd-analyze verify</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-72 overflow-y-auto">
              {(sysInfo.systemd_analyze_verify_text || '').trim() || 'No output.'}
            </pre>
          </details>
          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-surface)]">
            <summary className="px-3 py-2 text-sm text-[var(--text-secondary)] cursor-pointer hover:text-[var(--text-primary)]">systemctl list-dependencies default.target</summary>
            <pre className="px-3 pb-3 text-xs text-[var(--text-muted)] font-mono whitespace-pre-wrap break-words border-t border-[var(--apple-hairline)]/30 pt-2 max-h-72 overflow-y-auto">
              {(sysInfo.default_target_dependencies_text || '').trim() || 'No output.'}
            </pre>
          </details>
        </div>
      )}

      {/* Metrics History Charts */}
      {history.length > 2 && (
        <div className="flex flex-col gap-6">
          <ChartCard title="CPU Usage History" color="#3b82f6" dataKey="cpu" data={history} unit="%" />
          <ChartCard title="Memory Usage History" color="#10b981" dataKey="mem" data={history} unit="%" />
          <ChartCard title="Disk Usage History" color="#f59e0b" dataKey="disk" data={history} unit="%" />
          <ChartCard title="Load Average History" color="#a855f7" dataKey="load" data={history} unit="" domain={undefined} />
        </div>
      )}

      {/* Hardware Details */}
      <div className="space-y-6">
        <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--apple-hairline)] space-y-3">
          <h2 className="text-lg font-semibold flex items-center gap-2"><Server className={`w-5 h-5 ${statusToneClass('info')}`} /> System</h2>
          <InfoRow label="Hostname" value={node.hostname} />
          <InfoRow label="Hypervisor" value={`${node.hypervisor} ${node.hypervisor_version}`} />
          <InfoRow label="Libvirt" value={node.lib_version} />
          <InfoRow label="Active VMs" value={node.active_vms} />
          <InfoRow label="Defined VMs" value={node.defined_vms} />
          {stats && <InfoRow label="Processes" value={stats.processes} />}
        </div>

        <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--apple-hairline)] space-y-3">
          <h2 className="text-lg font-semibold flex items-center gap-2"><Cpu className="w-5 h-5 text-purple-400" /> CPU</h2>
          <InfoRow label="Architecture" value={node.cpu_model} />
          <InfoRow label="Cores" value={node.cpu_cores} />
          <InfoRow label="Threads per Core" value={node.cpu_threads} />
          <InfoRow label="Sockets" value={node.cpu_sockets} />
          <InfoRow label="Total vCPUs" value={node.cpu_cores * node.cpu_threads * node.cpu_sockets} />
          <InfoRow label="NUMA Nodes" value={node.numa_nodes} />
          {stats && <InfoRow label="Current Load" value={`${stats.load_1.toFixed(2)} / ${stats.load_5.toFixed(2)} / ${stats.load_15.toFixed(2)}`} />}
        </div>

        <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--apple-hairline)] space-y-3">
          <h2 className="text-lg font-semibold flex items-center gap-2"><MemoryStick className={`w-5 h-5 ${statusToneClass('ok')}`} /> Memory</h2>
          <InfoRow label="Total RAM" value={`${(node.memory_mb / 1024).toFixed(1)} GB`} />
          {stats && (
            <>
              <InfoRow label="Used" value={`${(stats.memory_used_mb / 1024).toFixed(1)} GB (${stats.memory_percent.toFixed(1)}%)`} />
              <InfoRow label="Available" value={`${((stats.memory_total_mb - stats.memory_used_mb) / 1024).toFixed(1)} GB`} />
              <InfoRow label="Swap Total" value={`${(stats.swap_total_mb / 1024).toFixed(1)} GB`} />
              <InfoRow label="Swap Used" value={`${(stats.swap_used_mb / 1024).toFixed(1)} GB`} />
            </>
          )}
        </div>
      </div>

      {/* Platform inventory — sysfs CPU topology + DMI + libvirt reconciliation */}
      {hardwareInventory && (
        <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--apple-hairline)] space-y-4">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <h2 className="text-lg font-semibold flex items-center gap-2">
                <FolderTree className="w-5 h-5 text-[var(--accent)]" /> Platform inventory
              </h2>
              <p className="text-xs text-[var(--text-muted)] mt-1 max-w-3xl">
                Kernel-exposed CPU topology and SMBIOS/DMI tables (same conceptual sources as proprietary hypervisors use for sockets/cores/threads/NUMA),
                cross-checked with libvirt&apos;s node caps for audit-style reconciliation.
              </p>
            </div>
            <span className="text-[10px] text-[var(--text-muted)] font-mono whitespace-nowrap">
              {hardwareInventory.collected_at_rfc3339}
            </span>
          </div>

          {hardwareInventory.consistency_notes.length > 0 && (
            <div className={`rounded-lg px-4 py-3 text-sm space-y-1 ${statusSurfaceClasses('warn')}`}>
              {hardwareInventory.consistency_notes.map((note) => (
                <p key={note}>{note}</p>
              ))}
            </div>
          )}

          <div className="space-y-6">
            <div className="space-y-2">
              <h3 className="text-sm font-medium text-[var(--text-secondary)]">Firmware / chassis (DMI)</h3>
              <InfoRow label="Hardware UUID" value={hardwareInventory.dmi.product_uuid ?? '—'} />
              <InfoRow label="Serial / tag" value={hardwareInventory.dmi.product_serial || '—'} />
              <InfoRow label="Vendor" value={hardwareInventory.dmi.sys_vendor || '—'} />
              <InfoRow label="Product" value={hardwareInventory.dmi.product_name || '—'} />
              <InfoRow label="Board" value={[hardwareInventory.dmi.board_vendor, hardwareInventory.dmi.board_name].filter(Boolean).join(' ') || '—'} />
              <InfoRow label="BIOS" value={[hardwareInventory.dmi.bios_version, hardwareInventory.dmi.bios_date].filter(Boolean).join(' · ') || '—'} />
            </div>
            <div className="space-y-2">
              <h3 className="text-sm font-medium text-[var(--text-secondary)]">CPU identity (/proc/cpuinfo)</h3>
              <InfoRow label="Vendor" value={hardwareInventory.cpuinfo_vendor_id ?? '—'} />
              <InfoRow label="Model" value={hardwareInventory.cpuinfo_model_name ?? '—'} />
            </div>
          </div>

          <div>
            <h3 className="text-sm font-medium text-[var(--text-secondary)] mb-2">Topology (sysfs)</h3>
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 mb-4">
              <div className="rounded-lg bg-[var(--apple-surface)] border border-[var(--apple-hairline)]/40 px-3 py-2">
                <div className="text-[10px] uppercase tracking-wide text-[var(--text-muted)]">Online CPUs</div>
                <div className="text-xl font-semibold text-[var(--text-primary)]">{hardwareInventory.cpu_topology.logical_cpus}</div>
              </div>
              <div className="rounded-lg bg-[var(--apple-surface)] border border-[var(--apple-hairline)]/40 px-3 py-2">
                <div className="text-[10px] uppercase tracking-wide text-[var(--text-muted)]">Sockets</div>
                <div className="text-xl font-semibold text-[var(--text-primary)]">{hardwareInventory.cpu_topology.sockets}</div>
              </div>
              <div className="rounded-lg bg-[var(--apple-surface)] border border-[var(--apple-hairline)]/40 px-3 py-2">
                <div className="text-[10px] uppercase tracking-wide text-[var(--text-muted)]">Physical cores</div>
                <div className="text-xl font-semibold text-[var(--text-primary)]">{hardwareInventory.cpu_topology.physical_cores}</div>
              </div>
              <div className="rounded-lg bg-[var(--apple-surface)] border border-[var(--apple-hairline)]/40 px-3 py-2">
                <div className="text-[10px] uppercase tracking-wide text-[var(--text-muted)]">Threads / core (max)</div>
                <div className="text-xl font-semibold text-[var(--text-primary)]">{hardwareInventory.cpu_topology.threads_per_core_max}</div>
              </div>
            </div>
            <div className="rounded-lg bg-[var(--apple-surface)] border border-[var(--apple-hairline)]/40 p-4 font-mono text-xs text-[var(--text-secondary)] leading-relaxed">
              <div className="text-[var(--text-muted)] mb-3 select-none">
                {(hardwareInventory.dmi.sys_vendor || 'Host').trim() || 'Host'} · {(hardwareInventory.dmi.product_name || 'platform').trim() || 'platform'}
              </div>
              {hardwareInventory.cpu_topology.socket_package_ids.length === 0 ? (
                <p className="text-[var(--text-muted)]">No sysfs topology (non-Linux host or cpus offline).</p>
              ) : (
                hardwareInventory.cpu_topology.socket_package_ids.map((pkgId, i) => (
                  <div key={`${pkgId}-${i}`} className="border-l border-[var(--apple-hairline)] ml-1 pl-3 pb-2 last:pb-0">
                    <span className={`${statusToneClass('ok')} opacity-90`}>Socket</span>{' '}
                    <span className="text-[var(--text-primary)]">{pkgId}</span>
                    <span className="text-[var(--text-muted)]"> — </span>
                    <span>{hardwareInventory.cpu_topology.cores_per_socket[i] ?? 0} cores</span>
                    <span className="text-[var(--text-muted)]"> × </span>
                    <span>≤{hardwareInventory.cpu_topology.threads_per_core_max} threads/core</span>
                  </div>
                ))
              )}
            </div>
          </div>

          {hardwareInventory.numa_nodes.length > 0 && (
            <div>
              <h3 className="text-sm font-medium text-[var(--text-secondary)] mb-2">NUMA (sysfs)</h3>
              <div className="overflow-x-auto apple-surface rounded-2xl/40">
                <table className="w-full text-sm" aria-label="NUMA topology">
                  <thead>
                    <tr className="text-left text-[var(--text-muted)] border-b border-[var(--apple-hairline)]">
                      <th scope="col" className="px-4 py-2">Node</th>
                      <th scope="col" className="px-4 py-2">CPUs</th>
                      <th scope="col" className="px-4 py-2">Memory</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-[var(--apple-hairline)]/40">
                    {hardwareInventory.numa_nodes.map((nn) => (
                      <tr key={nn.node_id}>
                        <td className="px-4 py-2 font-mono">{nn.node_id}</td>
                        <td className="px-4 py-2 font-mono text-xs text-[var(--text-secondary)]">{nn.cpu_list || '—'}</td>
                        <td className="px-4 py-2">{formatBytes(nn.memory_total_kb * 1024)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {hardwareInventory.libvirt && (
            <div>
              <h3 className="text-sm font-medium text-[var(--text-secondary)] mb-2">Libvirt node caps (reconciliation)</h3>
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 text-sm">
                <InfoRow label="libvirt model" value={hardwareInventory.libvirt.cpu_model} />
                <InfoRow label="NUMA cells (libvirt)" value={hardwareInventory.libvirt.numa_nodes} />
                <InfoRow label="Sockets × cores × threads" value={`${hardwareInventory.libvirt.cpu_sockets} × ${hardwareInventory.libvirt.cpu_cores} × ${hardwareInventory.libvirt.cpu_threads}`} />
                <InfoRow label="Derived logical CPUs" value={hardwareInventory.libvirt_logical_cpus_derived ?? '—'} />
                <InfoRow label="RAM (libvirt)" value={`${(hardwareInventory.libvirt.memory_mb / 1024).toFixed(1)} GB`} />
              </div>
            </div>
          )}

          <details className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-fill-tertiary)]">
            <summary className="px-3 py-2 text-xs text-[var(--text-muted)] cursor-pointer hover:text-[var(--text-secondary)]">Data sources</summary>
            <ul className="px-4 pb-3 text-[11px] font-mono text-[var(--text-muted)] space-y-0.5">
              {hardwareInventory.sources.map((s) => (
                <li key={s}>{s}</li>
              ))}
            </ul>
          </details>
        </div>
      )}

      {hardwareInventoryHistory && (
        <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--apple-hairline)] space-y-3">
          <div className="flex flex-wrap justify-between gap-2 items-start">
            <div>
              <h2 className="text-lg font-semibold flex items-center gap-2">
                <ScrollText className="w-5 h-5 text-[var(--text-muted)]" /> Inventory history
              </h2>
              <p className="text-xs text-[var(--text-muted)] mt-1 max-w-3xl">
                JSON Lines on the hypervisor (daemon appends on a schedule; default hourly). Newest snapshots first — useful for spotting UUID / topology drift over time.
              </p>
            </div>
            <span
              className="text-[10px] text-[var(--text-muted)] font-mono truncate max-w-[min(100%,28rem)] text-right"
              title={hardwareInventoryHistory.path}
            >
              {hardwareInventoryHistory.path}
            </span>
          </div>
          {hardwareInventoryHistory.entries.length === 0 ? (
            <p className="text-sm text-[var(--text-muted)]">
              No snapshots yet. Tune <code className="text-[var(--text-muted)]">[inventory_history]</code> in{' '}
              <code className="text-[var(--text-muted)]">config.toml</code> (enable, interval, max file size).
            </p>
          ) : (
            <div className="overflow-x-auto apple-surface rounded-2xl/40">
              <table className="w-full text-sm" aria-label="Hardware inventory history">
                <thead>
                  <tr className="text-left text-[var(--text-muted)] border-b border-[var(--apple-hairline)]">
                    <th scope="col" className="px-4 py-2 whitespace-nowrap">Collected (UTC)</th>
                    <th scope="col" className="px-4 py-2">CPUs</th>
                    <th scope="col" className="px-4 py-2">Sockets</th>
                    <th scope="col" className="px-4 py-2">Phys. cores</th>
                    <th scope="col" className="px-4 py-2 hidden md:table-cell">HW UUID</th>
                    <th scope="col" className="px-4 py-2">Alerts</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-[var(--apple-hairline)]/40">
                  {hardwareInventoryHistory.entries.map((e, idx) => (
                    <tr key={`${e.collected_at_rfc3339}-${idx}`} className="hover:bg-[var(--surface-hover)]/25">
                      <td className="px-4 py-2 font-mono text-xs whitespace-nowrap text-[var(--text-secondary)]">{e.collected_at_rfc3339}</td>
                      <td className="px-4 py-2 tabular-nums">{e.cpu_topology.logical_cpus}</td>
                      <td className="px-4 py-2 tabular-nums">{e.cpu_topology.sockets}</td>
                      <td className="px-4 py-2 tabular-nums">{e.cpu_topology.physical_cores}</td>
                      <td className="px-4 py-2 font-mono text-xs max-w-[160px] truncate hidden md:table-cell" title={e.dmi.product_uuid ?? ''}>
                        {e.dmi.product_uuid ? `${e.dmi.product_uuid.slice(0, 10)}…` : '—'}
                      </td>
                      <td className="px-4 py-2 text-xs">
                        {e.consistency_notes.length > 0 ? (
                          <span className={statusToneClass('warn')} title={e.consistency_notes.join('\n')}>
                            {e.consistency_notes.length} note{e.consistency_notes.length === 1 ? '' : 's'}
                          </span>
                        ) : (
                          <span className="text-[var(--text-muted)]">—</span>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {/* Storage Overview (root aggregate) */}
      {stats && (
        <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--apple-hairline)]">
          <h2 className="text-lg font-semibold flex items-center gap-2 mb-4"><Database className="w-5 h-5 text-orange-400" /> Root filesystem</h2>
          <div className="grid grid-cols-3 gap-6">
            <div>
              <div className="text-xs text-[var(--text-muted)] mb-1">Total</div>
              <div className="text-2xl font-bold">{stats.disk_total_gb.toFixed(0)} GB</div>
            </div>
            <div>
              <div className="text-xs text-[var(--text-muted)] mb-1">Used</div>
              <div className="text-2xl font-bold text-orange-400">{stats.disk_used_gb.toFixed(0)} GB</div>
            </div>
            <div>
              <div className="text-xs text-[var(--text-muted)] mb-1">Free</div>
              <div className={`text-2xl font-bold ${statusToneClass('ok')}`}>{(stats.disk_total_gb - stats.disk_used_gb).toFixed(0)} GB</div>
            </div>
          </div>
          <div className="mt-4 w-full bg-[var(--surface-hover)] rounded-full h-3">
            <div className={`h-3 rounded-full transition-all ${stats.disk_percent > 90 ? 'bg-red-500' : stats.disk_percent > 70 ? 'bg-yellow-500' : 'bg-[var(--accent)]'}`} style={{ width: `${Math.min(stats.disk_percent, 100)}%` }} />
          </div>
          <div className="text-xs text-[var(--text-muted)] mt-1 text-right">{stats.disk_percent.toFixed(1)}% used (/) </div>
        </div>
      )}

      {/* All mounts — df */}
      {filesystems.length > 0 && (
        <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
          <div className="px-6 py-4 border-b border-[var(--apple-hairline)] flex items-center justify-between gap-3 flex-wrap">
            <h2 className="text-lg font-semibold flex items-center gap-2"><FolderTree className={`w-5 h-5 ${statusToneClass('warn')}`} /> Filesystems</h2>
            <span className="text-xs text-[var(--text-muted)]">Per mount from the hypervisor (same idea as Cockpit Storage)</span>
          </div>
          <div className="overflow-x-auto">
            <table className="w-full text-sm" aria-label="Filesystem mounts">
              <thead>
                <tr className="text-left text-[var(--text-muted)] border-b border-[var(--apple-hairline)]">
                  <th scope="col" className="px-6 py-3 font-medium">Mounted on</th>
                  <th scope="col" className="px-6 py-3 font-medium hidden lg:table-cell">Device</th>
                  <th scope="col" className="px-6 py-3 font-medium">Type</th>
                  <th scope="col" className="px-6 py-3 font-medium text-right">Size</th>
                  <th scope="col" className="px-6 py-3 font-medium text-right hidden md:table-cell">Used</th>
                  <th scope="col" className="px-6 py-3 font-medium text-right hidden md:table-cell">Avail</th>
                  <th scope="col" className="px-6 py-3 font-medium text-right">Use</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/50">
                {filesystems.map((row) => (
                  <tr key={`${row.source}-${row.mount_point}`} className="hover:bg-[var(--surface-hover)]/30">
                    <td className="px-6 py-3 font-mono text-[var(--text-primary)]">{row.mount_point}</td>
                    <td className="px-6 py-3 font-mono text-[var(--text-muted)] text-xs max-w-[14rem] truncate hidden lg:table-cell" title={row.source}>{row.source}</td>
                    <td className="px-6 py-3 text-[var(--text-secondary)]">{row.fstype}</td>
                    <td className="px-6 py-3 text-right text-[var(--text-primary)]">{formatBytes(row.size_bytes)}</td>
                    <td className="px-6 py-3 text-right text-[var(--text-secondary)] hidden md:table-cell">{formatBytes(row.used_bytes)}</td>
                    <td className="px-6 py-3 text-right text-[var(--text-secondary)] hidden md:table-cell">{formatBytes(row.avail_bytes)}</td>
                    <td className="px-6 py-3 text-right">
                      <span className={`${statusToneClass(utilizationTone(row.use_percent))}${row.use_percent > 75 ? ' font-medium' : ''}`}>
                        {row.use_percent.toFixed(0)}%
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* Top processes: highest memory (RSS) and highest CPU */}
      {topProcesses.length > 0 && (
        <HostProcessTableBlock
          variant="memory"
          processes={topProcesses}
          canKillHostProcess={canKillHostProcess}
          killBusyPid={killBusyPid}
          onKill={killHostProcess}
        />
      )}
      {topProcessesByCpu.length > 0 && (
        <HostProcessTableBlock
          variant="cpu"
          processes={topProcessesByCpu}
          canKillHostProcess={canKillHostProcess}
          killBusyPid={killBusyPid}
          onKill={killHostProcess}
        />
      )}

      <div className="rounded-lg border border-[var(--apple-hairline)]/40 bg-[var(--apple-fill-tertiary)] px-4 py-3 text-xs text-[var(--text-muted)] leading-relaxed">
        <span className="font-medium text-[var(--text-secondary)]">How this works across distros: </span>
        Updates use apt on Debian/Ubuntu (before dnf so WSL/mixed installs stay correct), then microdnf/dnf, yum, Alpine <code className="text-[var(--text-muted)]">apk</code>, pacman, zypper.
        User and group tables prefer <code className="text-[var(--text-muted)]">getent</code> (honours LDAP/NIS), falling back to <code className="text-[var(--text-muted)]">/etc/passwd</code> / <code className="text-[var(--text-muted)]">/etc/group</code>.
        Network totals are cumulative since boot from <code className="text-[var(--text-muted)]">/proc/net/dev</code>; live throughput uses two samples on demand (blocks the chosen interval on the daemon). Firewall detection matches Host networking (ufw vs firewalld vs iptables).
        Process tables use <code className="text-[var(--text-muted)]">ps</code> sorted by <code className="text-[var(--text-muted)]">-rss</code> or <code className="text-[var(--text-muted)]">-pcpu</code> (two separate snapshots).
      </div>

      {!loading && pkgUpdates && (
        <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 p-6 space-y-4">
          <h2 className="text-lg font-semibold flex items-center gap-2"><Package className={`w-5 h-5 ${statusToneClass('ok')}`} /> Package updates</h2>
          <div className="text-sm text-[var(--text-secondary)] space-y-1">
            <div><span className="text-[var(--text-muted)]">Backend:</span> <code className={statusToneClass('warn')}>{pkgUpdates.backend}</code></div>
            {pkgUpdates.summary && <div>{pkgUpdates.summary}</div>}
            {pkgUpdates.pending_count != null && <div><span className="text-[var(--text-muted)]">Pending count:</span> {pkgUpdates.pending_count}</div>}
            {pkgUpdates.reboot_required && (
              <div className={`rounded-md px-3 py-2 text-sm ${statusSurfaceClasses('warn')}`}>
                A reboot appears to be required on this host (e.g. Debian/Ubuntu <code className="opacity-90">/var/run/reboot-required</code> is present). Plan maintenance before applying kernel or libc upgrades.
              </div>
            )}
            {pkgUpdates.hint && <div className="text-[var(--text-muted)] text-xs">{pkgUpdates.hint}</div>}
            {pkgUpdates.error && <div className={`text-xs break-words ${statusToneClass('error')}`}>{pkgUpdates.error}</div>}
          </div>
          <p className="text-xs text-[var(--text-muted)] leading-relaxed">
            The probe above is read-only. Upgrade, install, and remove require a browser session (not an API token), use the same backend as the probe, and are limited to one action at a time on the daemon (commands may run up to an hour).
          </p>
          {pkgUpdates.backend !== 'unknown' && (
            <div className="space-y-3 pt-1 border-t border-[var(--apple-hairline)]">
              <div className="flex flex-wrap items-center gap-2">
                <button
                  type="button"
                  onClick={() => void runPackageUpgrade()}
                  disabled={pkgMutBusy}
                  className={`inline-flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm font-medium border disabled:opacity-50 transition ${statusBadgeClasses('warn')} border-[color-mix(in_srgb,var(--machina-status-warn)_40%,transparent)] hover:bg-[color-mix(in_srgb,var(--machina-status-warn)_15%,transparent)]`}
                >
                  <ArrowUpCircle className="w-4 h-4 shrink-0" />
                  {pkgMutBusy ? 'Running…' : 'Upgrade all packages'}
                </button>
                <button
                  type="button"
                  onClick={() => void runPackageUpgradeDryRun()}
                  disabled={pkgMutBusy}
                  className="inline-flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm font-medium bg-[var(--apple-fill-tertiary)] text-[var(--text-primary)] border border-[var(--apple-hairline)] hover:bg-[var(--surface-hover)] disabled:opacity-50 transition"
                >
                  Dry-run / preview upgrade
                </button>
                {pkgUpdates.backend === 'apt' && (
                  <button
                    type="button"
                    onClick={() => void runPackageAutoremove()}
                    disabled={pkgMutBusy}
                    className="inline-flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm font-medium bg-[var(--accent)]/20 text-[var(--text-primary)] border border-violet-500/35 hover:bg-[var(--accent)]/30 disabled:opacity-50 transition"
                  >
                    apt autoremove
                  </button>
                )}
              </div>
              <div className="flex flex-col sm:flex-row sm:items-end gap-2">
                <label className="flex-1 flex flex-col gap-1 text-xs text-[var(--text-muted)]">
                  <span>Install packages (space or comma separated, max 32)</span>
                  <input
                    type="text"
                    value={pkgInstallInput}
                    onChange={(e) => setPkgInstallInput(e.target.value)}
                    disabled={pkgMutBusy}
                    placeholder="e.g. htop curl"
                    className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm py-2 px-3 text-[var(--text-primary)] placeholder:text-[var(--text-faint)] disabled:opacity-50"
                  />
                </label>
                <button
                  type="button"
                  onClick={() => void runPackageInstall()}
                  disabled={pkgMutBusy}
                  className="btn-secondary text-sm disabled:opacity-50 transition sm:shrink-0"
                >
                  <PlusCircle className="w-4 h-4 shrink-0" />
                  Install
                </button>
              </div>
              <div className="flex flex-col sm:flex-row sm:items-end gap-2">
                <label className="flex-1 flex flex-col gap-1 text-xs text-[var(--text-muted)]">
                  <span>Remove packages (space or comma separated, max 32)</span>
                  <input
                    type="text"
                    value={pkgRemoveInput}
                    onChange={(e) => setPkgRemoveInput(e.target.value)}
                    disabled={pkgMutBusy}
                    placeholder="e.g. cowsay"
                    className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm py-2 px-3 text-[var(--text-primary)] placeholder:text-[var(--text-faint)] disabled:opacity-50"
                  />
                </label>
                <label className="flex items-center gap-2 text-xs text-[var(--text-muted)] sm:pb-2 shrink-0">
                  <input
                    type="checkbox"
                    checked={pkgRemovePurge}
                    onChange={(e) => setPkgRemovePurge(e.target.checked)}
                    disabled={pkgMutBusy}
                    className="rounded border-[var(--apple-hairline)]"
                  />
                  Purge (apt: remove config files)
                </label>
                <button
                  type="button"
                  onClick={() => void runPackageRemove()}
                  disabled={pkgMutBusy}
                  className="inline-flex items-center justify-center gap-2 px-3 py-2 rounded-lg text-sm font-medium bg-rose-600/20 text-rose-700 border border-rose-600/35 hover:bg-rose-600/30 disabled:opacity-50 transition sm:shrink-0"
                >
                  <MinusCircle className="w-4 h-4 shrink-0" />
                  Remove
                </button>
              </div>
            </div>
          )}
          {pkgActionResult && (
            <div className="space-y-2 text-xs">
              <div className="flex flex-wrap items-center gap-2">
                <span className={statusToneClass(pkgActionResult.ok ? 'ok' : 'error')}>
                  {pkgActionResult.ok ? 'Finished successfully' : 'Finished with errors'}
                </span>
                <span className="text-[var(--text-muted)]">exit {pkgActionResult.exit_code}</span>
              </div>
              <div className="text-[var(--text-muted)] font-mono break-all">{pkgActionResult.command}</div>
              {(pkgActionResult.stdout || pkgActionResult.stderr) && (
                <pre className="max-h-64 overflow-auto rounded-lg bg-[var(--apple-surface)]/80 border border-[var(--apple-hairline)] p-3 text-[var(--text-secondary)] whitespace-pre-wrap break-words">
                  {pkgActionResult.stdout ? `--- stdout ---\n${pkgActionResult.stdout}\n` : ''}
                  {pkgActionResult.stderr ? `--- stderr ---\n${pkgActionResult.stderr}` : ''}
                </pre>
              )}
            </div>
          )}
        </div>
      )}

      {linuxObs?.pressure.available && (
        <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 p-6 space-y-3">
          <h2 className="text-lg font-semibold">Resource pressure (PSI)</h2>
          <p className="text-xs text-[var(--text-muted)]">From <code className="text-[var(--text-faint)]">/proc/pressure/*</code> — Linux kernel stall metrics (same on all distros with PSI enabled).</p>
          <div className="grid sm:grid-cols-3 gap-4 text-sm">
            <div>
              <div className="text-[var(--text-muted)] mb-1">CPU</div>
              <div className="text-[var(--text-primary)]">some {linuxObs.pressure.cpu.some.toFixed(1)}% · full {linuxObs.pressure.cpu.full.toFixed(1)}%</div>
            </div>
            <div>
              <div className="text-[var(--text-muted)] mb-1">Memory</div>
              <div className="text-[var(--text-primary)]">some {linuxObs.pressure.memory.some.toFixed(1)}% · full {linuxObs.pressure.memory.full.toFixed(1)}%</div>
            </div>
            <div>
              <div className="text-[var(--text-muted)] mb-1">I/O</div>
              <div className="text-[var(--text-primary)]">some {linuxObs.pressure.io.some.toFixed(1)}% · full {linuxObs.pressure.io.full.toFixed(1)}%</div>
            </div>
          </div>
          {linuxObs.disk_io.length > 0 && (
            <div className="pt-2 border-t border-[var(--apple-hairline)]">
              <div className="text-[var(--text-muted)] text-xs mb-2">Block devices (/proc/diskstats)</div>
              <div className="overflow-x-auto">
                <table className="w-full text-xs" aria-label="Block device I/O">
                  <thead>
                    <tr className="text-[var(--text-muted)] text-left">
                      <th scope="col" className="py-1 pr-3">Device</th>
                      <th scope="col" className="py-1 pr-3 text-right">Read</th>
                      <th scope="col" className="py-1 text-right">Write</th>
                    </tr>
                  </thead>
                  <tbody>
                    {linuxObs.disk_io.slice(0, 8).map((d) => (
                      <tr key={d.device} className="border-t border-[var(--apple-hairline)]/30">
                        <td className="py-1 pr-3 font-mono text-[var(--text-secondary)]">{d.device}</td>
                        <td className="py-1 pr-3 text-right text-[var(--text-muted)]">{formatBytes(d.read_bytes)}</td>
                        <td className="py-1 text-right text-[var(--text-muted)]">{formatBytes(d.write_bytes)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}
          {linuxObs.smart.some((s) => s.probed) && (
            <div className="pt-2 border-t border-[var(--apple-hairline)] space-y-1">
              <div className="text-[var(--text-muted)] text-xs">SMART (smartctl -H)</div>
              {linuxObs.smart.filter((s) => s.probed).map((s) => (
                <div key={s.device} className={`text-xs ${statusToneClass(s.passed ? 'ok' : 'error')}`}>
                  {s.device}: {s.summary || (s.passed ? 'PASSED' : 'FAILED')}
                </div>
              ))}
            </div>
          )}
          {linuxObs.cgroup?.available && (
            <div className="pt-2 border-t border-[var(--apple-hairline)] text-sm">
              <div className="text-[var(--text-muted)] text-xs mb-1">Daemon cgroup v2 ({linuxObs.cgroup.unified_path || 'self'})</div>
              {linuxObs.cgroup.memory_current_bytes != null && (
                <div className="text-[var(--text-secondary)]">
                  memory.current {formatBytes(linuxObs.cgroup.memory_current_bytes)}
                  {linuxObs.cgroup.memory_max_bytes != null
                    ? ` / ${formatBytes(linuxObs.cgroup.memory_max_bytes)}`
                    : ''}
                </div>
              )}
            </div>
          )}
          {(linuxObs.thermal?.length ?? 0) > 0 && (
            <div className="pt-2 border-t border-[var(--apple-hairline)] text-sm">
              <div className="text-[var(--text-muted)] text-xs mb-2">Hardware temperature (hwmon)</div>
              <div className="grid sm:grid-cols-2 gap-2">
                {linuxObs.thermal!.map((t) => (
                  <div key={t.sensor} className="text-[var(--text-secondary)] text-xs">
                    <span className="text-[var(--text-muted)]">{t.sensor}:</span>{' '}
                    {t.temp_celsius.toFixed(1)} °C
                    {t.critical_celsius != null ? ` (crit ${t.critical_celsius.toFixed(0)} °C)` : ''}
                  </div>
                ))}
              </div>
            </div>
          )}
          {linuxObs.bpf?.available && (
            <div className="pt-2 border-t border-[var(--apple-hairline)] text-sm">
              <div className="text-[var(--text-muted)] text-xs mb-1">eBPF (bpftool)</div>
              <div className="text-[var(--text-secondary)] text-xs">
                {linuxObs.bpf.program_count} programs · {linuxObs.bpf.map_count} maps ·{' '}
                {linuxObs.bpf.cgroup_program_count} cgroup
              </div>
            </div>
          )}
          {linuxAudit?.available && (linuxAudit.events?.length ?? 0) > 0 && (
            <div className="pt-2 border-t border-[var(--apple-hairline)] text-sm overflow-x-auto max-h-48 overflow-y-auto">
              <div className="text-[var(--text-muted)] text-xs mb-2">
                Linux auditd ({linuxAudit.source}) — {linuxAudit.avc_count} AVC in window
              </div>
              {linuxAudit.events.slice(-12).map((ev, i) => (
                <div key={`${ev.timestamp}-${i}`} className="text-xs text-[var(--text-muted)] font-mono mb-1">
                  <span className={`${statusToneClass('warn')} opacity-90`}>{ev.event_type}</span>{' '}
                  {ev.summary}
                </div>
              ))}
            </div>
          )}
          {(linuxObs.vm_cgroups?.length ?? 0) > 0 && (
            <div className="pt-2 border-t border-[var(--apple-hairline)] text-sm overflow-x-auto">
              <div className="text-[var(--text-muted)] text-xs mb-2">VM cgroups (machine-qemu)</div>
              <table className="w-full text-xs" aria-label="VM cgroups">
                <thead>
                  <tr className="text-[var(--text-muted)] text-left">
                    <th scope="col" className="pr-3 pb-1">VM</th>
                    <th scope="col" className="pr-3 pb-1">Memory</th>
                    <th scope="col" className="pb-1">CPU use</th>
                  </tr>
                </thead>
                <tbody>
                  {linuxObs.vm_cgroups!.map((cg) => (
                    <tr key={cg.vm_name} className="text-[var(--text-secondary)]">
                      <td className="pr-3 py-0.5 font-mono">{cg.vm_name}</td>
                      <td className="pr-3 py-0.5">
                        {cg.memory_current_bytes != null
                          ? formatBytes(cg.memory_current_bytes)
                          : '—'}
                        {cg.memory_max_bytes != null ? ` / ${formatBytes(cg.memory_max_bytes)}` : ''}
                      </td>
                      <td className="py-0.5">
                        {cg.cpu_usage_usec != null
                          ? `${(cg.cpu_usage_usec / 1_000_000).toFixed(1)} s`
                          : '—'}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {securitySummary && (
        <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 p-6 space-y-2">
          <h2 className="text-lg font-semibold flex items-center gap-2"><Shield className="w-5 h-5 text-orange-400" /> Firewall &amp; host networking stack</h2>
          <div className="text-sm text-[var(--text-secondary)] grid sm:grid-cols-2 gap-2">
            <div><span className="text-[var(--text-muted)]">Network tooling:</span> <code className="text-[var(--accent)]">{securitySummary.network_backend}</code></div>
            <div><span className="text-[var(--text-muted)]">Firewall:</span> <code className="text-[var(--accent)]">{securitySummary.firewall_backend}</code></div>
            {securitySummary.ufw_status_line && <div className="sm:col-span-2 text-xs font-mono text-[var(--text-muted)]">{securitySummary.ufw_status_line}</div>}
            {securitySummary.firewalld_default_zone && (
              <div className="sm:col-span-2"><span className="text-[var(--text-muted)]">firewalld default zone:</span> <code className="text-[var(--accent)]">{securitySummary.firewalld_default_zone}</code></div>
            )}
            {securitySummary.selinux_mode && (
              <div><span className="text-[var(--text-muted)]">SELinux:</span> <code className="text-[var(--accent)]">{securitySummary.selinux_mode}</code></div>
            )}
          </div>
        </div>
      )}

      {netCounters.length > 0 && (
        <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
          <div className="px-6 py-4 border-b border-[var(--apple-hairline)] flex items-center justify-between gap-3 flex-wrap">
            <h2 className="text-lg font-semibold flex items-center gap-2"><Network className="w-5 h-5 text-[var(--link)]" /> Network I/O (since boot)</h2>
            <span className="text-xs text-[var(--text-muted)]">From /proc/net/dev — same counters on Ubuntu, Fedora, Arch, …</span>
          </div>
          <div className="overflow-x-auto">
            <table className="w-full text-sm" aria-label="Network I/O counters">
              <thead>
                <tr className="text-left text-[var(--text-muted)] border-b border-[var(--apple-hairline)]">
                  <th scope="col" className="px-6 py-3 font-medium">Interface</th>
                  <th scope="col" className="px-6 py-3 font-medium text-right">RX</th>
                  <th scope="col" className="px-6 py-3 font-medium text-right hidden md:table-cell">RX pkt</th>
                  <th scope="col" className="px-6 py-3 font-medium text-right">TX</th>
                  <th scope="col" className="px-6 py-3 font-medium text-right hidden md:table-cell">TX pkt</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/50">
                {netCounters.filter((r) => r.iface !== 'lo').map((r) => (
                  <tr key={r.iface} className="hover:bg-[var(--surface-hover)]/30">
                    <td className="px-6 py-3 font-mono text-[var(--text-primary)]">{r.iface}</td>
                    <td className="px-6 py-3 text-right text-[var(--text-primary)]">{formatBytes(r.rx_bytes)}</td>
                    <td className="px-6 py-3 text-right text-[var(--text-muted)] hidden md:table-cell">{r.rx_packets.toLocaleString()}</td>
                    <td className="px-6 py-3 text-right text-[var(--text-primary)]">{formatBytes(r.tx_bytes)}</td>
                    <td className="px-6 py-3 text-right text-[var(--text-muted)] hidden md:table-cell">{r.tx_packets.toLocaleString()}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
        <div className="px-6 py-4 border-b border-[var(--apple-hairline)] flex flex-wrap items-center justify-between gap-3">
          <div>
            <h2 className="text-lg font-semibold flex items-center gap-2"><Activity className={`w-5 h-5 ${statusToneClass('ok')}`} /> Live throughput</h2>
            <p className="text-xs text-[var(--text-muted)] mt-1">Two reads of <code className="text-[var(--text-faint)]">/proc/net/dev</code>; excludes <code className="text-[var(--text-faint)]">lo</code>. Same on all Linux distros.</p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <select
              aria-label="Throughput sample window"
              value={rateSampleMs}
              onChange={(e) => setRateSampleMs(Number(e.target.value))}
              disabled={netRatesLoading}
              className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm py-1.5 px-2 text-[var(--text-primary)]"
            >
              <option value={250}>250 ms window</option>
              <option value={500}>500 ms</option>
              <option value={1000}>1 s</option>
              <option value={2000}>2 s</option>
              <option value={5000}>5 s</option>
            </select>
            <button
              type="button"
              onClick={() => void measureNetRates()}
              disabled={netRatesLoading}
              className={`px-3 py-1.5 rounded-lg text-sm font-medium border disabled:opacity-50 transition ${statusBadgeClasses('ok')} border-[color-mix(in_srgb,var(--machina-status-ok)_40%,transparent)] hover:bg-[color-mix(in_srgb,var(--machina-status-ok)_15%,transparent)]`}
            >
              {netRatesLoading ? 'Sampling…' : 'Sample now'}
            </button>
          </div>
        </div>
        {netRates && netRates.interfaces.length > 0 && (
          <div className="overflow-x-auto">
            <p className="text-xs text-[var(--text-muted)] px-6 pt-3">Averaged over {netRates.sample_interval_ms} ms</p>
            <table className="w-full text-sm" aria-label="Network interface rates">
              <thead>
                <tr className="text-left text-[var(--text-muted)] border-b border-[var(--apple-hairline)]">
                  <th scope="col" className="px-6 py-3 font-medium">Interface</th>
                  <th scope="col" className="px-6 py-3 font-medium text-right">RX</th>
                  <th scope="col" className="px-6 py-3 font-medium text-right">TX</th>
                  <th scope="col" className="px-6 py-3 font-medium text-right hidden md:table-cell">RX pkt/s</th>
                  <th scope="col" className="px-6 py-3 font-medium text-right hidden md:table-cell">TX pkt/s</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/50">
                {netRates.interfaces.map((r) => (
                  <tr key={r.iface} className="hover:bg-[var(--surface-hover)]/30">
                    <td className="px-6 py-3 font-mono text-[var(--text-primary)]">{r.iface}</td>
                    <td className="px-6 py-3 text-right text-[var(--text-primary)]">{formatBytes(r.rx_bytes_per_sec)}/s</td>
                    <td className="px-6 py-3 text-right text-[var(--text-primary)]">{formatBytes(r.tx_bytes_per_sec)}/s</td>
                    <td className="px-6 py-3 text-right text-[var(--text-muted)] hidden md:table-cell">{r.rx_packets_per_sec.toFixed(0)}</td>
                    <td className="px-6 py-3 text-right text-[var(--text-muted)] hidden md:table-cell">{r.tx_packets_per_sec.toFixed(0)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        {netRates && netRates.interfaces.length === 0 && !netRatesLoading && (
          <p className="text-sm text-[var(--text-muted)] px-6 py-4">No non-loopback interfaces in sample.</p>
        )}
        {!netRates && !netRatesLoading && (
          <p className="text-sm text-[var(--text-muted)] px-6 py-4">Choose a window and click Sample now. The API blocks for that duration on the hypervisor (not run on every auto-refresh).</p>
        )}
      </div>

      {passwdUsers.length > 0 && (
        <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
          <div className="px-6 py-4 border-b border-[var(--apple-hairline)] flex flex-wrap items-center justify-between gap-3">
            <h2 className="text-lg font-semibold flex items-center gap-2"><Users className="w-5 h-5 text-[var(--accent)]" /> Accounts (passwd)</h2>
            <label className="flex items-center gap-2 text-xs text-[var(--text-muted)] cursor-pointer">
              <input type="checkbox" checked={showSystemAccounts} onChange={(e) => setShowSystemAccounts(e.target.checked)} className="rounded border-[var(--apple-hairline)] bg-[var(--apple-surface)]" />
              Show system accounts (UID {'<'} 1000)
            </label>
          </div>
          <div className="overflow-x-auto max-h-80 overflow-y-auto">
            <table className="w-full text-sm" aria-label="System users">
              <thead className="sticky top-0 bg-[var(--apple-fill-tertiary)] z-10">
                <tr className="text-left text-[var(--text-muted)] border-b border-[var(--apple-hairline)]">
                  <th scope="col" className="px-6 py-3 font-medium">User</th>
                  <th scope="col" className="px-6 py-3 font-medium">UID</th>
                  <th scope="col" className="px-6 py-3 font-medium">GID</th>
                  <th scope="col" className="px-6 py-3 font-medium hidden lg:table-cell">Home</th>
                  <th scope="col" className="px-6 py-3 font-medium hidden md:table-cell">Shell</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/50">
                {passwdUsers.filter((u) => showSystemAccounts || !u.system_account).map((u) => (
                  <tr key={`${u.username}-${u.uid}`} className="hover:bg-[var(--surface-hover)]/30">
                    <td className="px-6 py-2 font-mono text-[var(--text-primary)]">{u.username}{u.system_account && <span className="text-[var(--text-muted)] text-[10px] ml-1">sys</span>}</td>
                    <td className="px-6 py-2 text-[var(--text-secondary)]">{u.uid}</td>
                    <td className="px-6 py-2 text-[var(--text-secondary)]">{u.gid}</td>
                    <td className="px-6 py-2 text-[var(--text-muted)] text-xs hidden lg:table-cell truncate max-w-xs" title={u.home}>{u.home}</td>
                    <td className="px-6 py-2 text-[var(--text-muted)] text-xs hidden md:table-cell truncate max-w-[12rem]" title={u.shell}>{u.shell}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {groups.length > 0 && (
        <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
          <div className="px-6 py-4 border-b border-[var(--apple-hairline)]">
            <h2 className="text-lg font-semibold flex items-center gap-2"><UserSquare className="w-5 h-5 text-fuchsia-400" /> Groups (first rows)</h2>
            <p className="text-xs text-[var(--text-muted)] mt-1">Truncated list from getent/file; large LDAP domains may be incomplete.</p>
          </div>
          <div className="overflow-x-auto max-h-72 overflow-y-auto">
            <table className="w-full text-sm" aria-label="System groups">
              <thead className="sticky top-0 bg-[var(--apple-fill-tertiary)] z-10">
                <tr className="text-left text-[var(--text-muted)] border-b border-[var(--apple-hairline)]">
                  <th scope="col" className="px-6 py-3 font-medium">Group</th>
                  <th scope="col" className="px-6 py-3 font-medium">GID</th>
                  <th scope="col" className="px-6 py-3 font-medium">Members</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--apple-hairline)]/50">
                {groups.map((g) => (
                  <tr key={`${g.name}-${g.gid}`} className="hover:bg-[var(--surface-hover)]/30">
                    <td className="px-6 py-2 font-mono text-[var(--text-primary)]">{g.name}</td>
                    <td className="px-6 py-2 text-[var(--text-secondary)]">{g.gid}</td>
                    <td className="px-6 py-2 text-[var(--text-muted)] text-xs break-all">{g.members.length ? g.members.join(', ') : '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      <div>
        <h2 className="text-sm font-semibold text-[var(--text-muted)] uppercase tracking-wide mb-2">Cockpit host inventory</h2>
        <HostCockpitPanels classic section="all" />
      </div>

      <div>
        <h2 className="text-sm font-semibold text-[var(--text-muted)] uppercase tracking-wide mb-2">Host tools</h2>
        <ChoiceCardGrid>
          <ChoiceLinkCard to="/services" icon={<Server className="w-4 h-4" />} title="Systemd services" description="Unit status, start/stop, and journal tails." />
          <ChoiceLinkCard to="/logs" icon={<ScrollText className="w-4 h-4" />} title="Journal logs" description="Filter and follow messages on the hypervisor." />
          <ChoiceLinkCard to="/host-networking" icon={<Network className="w-4 h-4" />} title="Host networking" description="Bridges, routes, DNS, and firewall context." />
          <ChoiceLinkCard to="/k8s" icon={<Server className="w-4 h-4" />} title="Kubernetes cluster" description="Control-plane and worker node status with click actions." />
          <ChoiceLinkCard to="/k8s/workloads" icon={<Activity className="w-4 h-4" />} title="K8s workloads" description="Deployments, pods, services, and rollout/scale/delete actions." />
          <ChoiceLinkCard to="/k8s/kata" icon={<Package className="w-4 h-4" />} title="Kata / Cloud Hypervisor" description="Install kata-deploy and use runtimeClassName kata-clh on your cluster." />
        </ChoiceCardGrid>
      </div>
      </>
      )}
      <ConfirmDialog
        open={pkgConfirmOp === 'upgrade'}
        title="Run system package upgrade"
        message="Run a full system package upgrade on this host? This uses your distro package manager (apt, dnf, …), can take a long time, and may restart services."
        confirmLabel="Upgrade"
        variant="warning"
        onCancel={() => setPkgConfirmOp(null)}
        onConfirm={() => { setPkgConfirmOp(null); void doRunPackageUpgrade() }}
      />
      <ConfirmDialog
        open={pkgConfirmOp === 'autoremove'}
        title="Run apt autoremove"
        message="Remove packages that were installed only as dependencies and are no longer needed?"
        confirmLabel="Autoremove"
        variant="warning"
        onCancel={() => setPkgConfirmOp(null)}
        onConfirm={() => { setPkgConfirmOp(null); void doRunPackageAutoremove() }}
      />
      <ConfirmDialog
        open={pkgConfirmOp === 'remove'}
        title="Remove packages"
        message={`Remove these packages from the host? This may break dependent software: ${pkgRemoveInput}`}
        confirmLabel={pkgRemovePurge ? 'Purge' : 'Remove'}
        variant="danger"
        onCancel={() => setPkgConfirmOp(null)}
        onConfirm={() => { setPkgConfirmOp(null); void doRunPackageRemove() }}
      />
      <ConfirmDialog
        open={killConfirm !== null}
        title={killConfirm?.signal === 'KILL' ? 'Force-kill process' : 'Terminate process'}
        message={
          killConfirm?.signal === 'KILL'
            ? `Force-kill PID ${killConfirm.process.pid} (${killConfirm.process.command}) with SIGKILL? The application cannot catch this signal.`
            : `Send SIGTERM to PID ${killConfirm?.process.pid} (${killConfirm?.process.command})? The process should exit gracefully if it handles the signal.`
        }
        confirmLabel={killConfirm?.signal === 'KILL' ? 'Force kill' : 'Terminate'}
        variant="danger"
        onCancel={() => setKillConfirm(null)}
        onConfirm={() => {
          const k = killConfirm
          setKillConfirm(null)
          if (k) void doKillHostProcess(k.process, k.signal)
        }}
      />
    </PageLayout>
  )
}

function ChartCard({ title, color, dataKey, data, unit, domain }: { title: string; color: string; dataKey: string; data: StatsPoint[]; unit: string; domain?: [number, number] }) {
  return (
    <div className="bg-[var(--apple-surface)] rounded-xl p-5 border border-[var(--apple-hairline)]">
      <h2 className="text-sm font-semibold text-[var(--text-primary)] mb-3">{title}</h2>
      <ResponsiveContainer width="100%" height={160}>
        <AreaChart data={data}>
          <defs>
            <linearGradient id={`grad-${dataKey}`} x1="0" y1="0" x2="0" y2="1">
              <stop offset="5%" stopColor={color} stopOpacity={0.3} />
              <stop offset="95%" stopColor={color} stopOpacity={0} />
            </linearGradient>
          </defs>
          <CartesianGrid strokeDasharray="3 3" stroke="var(--apple-hairline)" />
          <XAxis dataKey="time" stroke="#475569" fontSize={10} tickLine={false} />
          <YAxis stroke="#475569" fontSize={10} tickLine={false} domain={domain || [0, 100]} />
          <Tooltip contentStyle={{ backgroundColor: 'var(--apple-surface)', border: '1px solid var(--apple-hairline)', borderRadius: '0.5rem' }} labelStyle={{ color: 'var(--text-secondary)' }} formatter={(v) => `${Number(v).toFixed(1)}${unit}`} />
          <Area type="monotone" dataKey={dataKey} stroke={color} strokeWidth={2} fillOpacity={1} fill={`url(#grad-${dataKey})`} />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  )
}

function InfoRow({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="flex items-center justify-between py-2 border-b border-[var(--apple-hairline)]/30">
      <span className="text-[var(--text-muted)] text-sm">{label}</span>
      <span className="text-sm font-medium">{String(value)}</span>
    </div>
  )
}
