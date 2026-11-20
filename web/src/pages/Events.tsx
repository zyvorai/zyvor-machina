// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Link } from 'react-router'
import { getMetrics, getMetricsHistory, VmMetrics, vmDetailRoute } from '../api/vm'
import {
  Activity,
  RefreshCw,
  Download,
  Cpu,
  HardDrive,
  Network,
  ChevronRight,
} from 'lucide-react'
import { downloadJSON, downloadCSV } from '../utils/export'
import EmptyState from '../components/EmptyState'
import PageLayout from '../components/PageLayout'
import { TahoeTableWrap, TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { formatUserError } from '../utils/apiError'
import { statusActionLinkClasses, statusBgClass, statusToneClass, utilizationTone } from '../utils/semanticColors'
import { libvirtErrorHints } from '../utils/libvirtHints'
import { formatBytes, formatThroughput } from '../utils/vm'
import {
  Area,
  AreaChart,
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
  Bar,
  BarChart,
} from 'recharts'

const POLL_MS = 5000
const MAX_POINTS = 48
const MAX_VM_LINES = 10

const VM_LINE_COLORS = [
  '#3b82f6',
  '#10b981',
  '#f59e0b',
  '#ec4899',
  '#8b5cf6',
  '#06b6d4',
  '#eab308',
  '#f97316',
  '#14b8a6',
  '#fb7185',
]

function chartMemKey(name: string) {
  return `mem:${name}`
}
function chartCpuKey(name: string) {
  return `cpu:${name}`
}

interface VmExtras {
  cpuPct: number
  rdBps: number
  wrBps: number
  rxBps: number
  txBps: number
}

type TimelineRow = Record<string, number | string>

export default function EventsPage() {
  const [metrics, setMetrics] = useState<VmMetrics[]>([])
  const [timeline, setTimeline] = useState<TimelineRow[]>([])
  const [vmExtras, setVmExtras] = useState<Record<string, VmExtras>>({})
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const prevRef = useRef<{ byName: Map<string, VmMetrics>; at: number } | null>(null)
  const historySeeded = useRef(false)

  useEffect(() => {
    if (historySeeded.current) return
    historySeeded.current = true
    getMetricsHistory(MAX_POINTS)
      .then((h) => {
        if (!h.points?.length) return
        const rows: TimelineRow[] = h.points.map((p) => {
          const timeStr = new Date(p.timestamp_ms).toLocaleTimeString([], {
            hour: '2-digit',
            minute: '2-digit',
            second: '2-digit',
          })
          const row: TimelineRow = { time: timeStr }
          for (const m of p.vm_metrics) {
            row[chartMemKey(m.name)] = parseFloat(m.memory_pct.toFixed(1))
          }
          return row
        })
        setTimeline(rows.slice(-MAX_POINTS))
        const last = h.points[h.points.length - 1]
        if (last?.vm_metrics?.length) {
          prevRef.current = {
            byName: new Map(last.vm_metrics.map((m) => [m.name, m])),
            at: last.timestamp_ms,
          }
        }
      })
      .catch(() => {})
  }, [])

  const sortedMetrics = useMemo(
    () => [...metrics].sort((a, b) => a.name.localeCompare(b.name)),
    [metrics],
  )

  const filteredMetrics = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return sortedMetrics
    return sortedMetrics.filter((m) => m.name.toLowerCase().includes(q))
  }, [sortedMetrics, search])

  const load = useCallback(async () => {
    try {
      setLoadError(null)
      const list = await getMetrics()
      setMetrics(list)

      if (list.length === 0) {
        prevRef.current = null
        return
      }

      const now = Date.now()
      const prev = prevRef.current
      const wallSec = prev?.at ? Math.max(0.5, (now - prev.at) / 1000) : POLL_MS / 1000

      const timeStr = new Date().toLocaleTimeString([], {
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
      })
      const row: TimelineRow = { time: timeStr }

      const extras: Record<string, VmExtras> = {}
      let sumMem = 0
      let sumCpu = 0
      let aggRd = 0
      let aggWr = 0
      let aggRx = 0
      let aggTx = 0

      const byName = new Map(list.map((m) => [m.name, m]))

      for (const m of list) {
        row[chartMemKey(m.name)] = parseFloat(m.memory_pct.toFixed(1))
        sumMem += m.memory_pct

        const p = prev?.byName.get(m.name)
        let cpuPct = 0
        let rdBps = 0
        let wrBps = 0
        let rxBps = 0
        let txBps = 0
        if (p) {
          const cpuDelta = m.cpu_time_ns - p.cpu_time_ns
          cpuPct =
            m.vcpus > 0
              ? Math.min(100, Math.max(0, (cpuDelta / 1e9) / wallSec / m.vcpus * 100))
              : 0
          rdBps = Math.max(0, m.disk_rd_bytes - p.disk_rd_bytes) / wallSec
          wrBps = Math.max(0, m.disk_wr_bytes - p.disk_wr_bytes) / wallSec
          rxBps = Math.max(0, m.net_rx_bytes - p.net_rx_bytes) / wallSec
          txBps = Math.max(0, m.net_tx_bytes - p.net_tx_bytes) / wallSec
        }
        row[chartCpuKey(m.name)] = parseFloat(cpuPct.toFixed(1))
        extras[m.name] = { cpuPct, rdBps, wrBps, rxBps, txBps }
        sumCpu += cpuPct
        aggRd += rdBps
        aggWr += wrBps
        aggRx += rxBps
        aggTx += txBps
      }

      row.avgMem = sumMem / list.length
      row.avgCpu = sumCpu / list.length
      row.diskRdBps = aggRd
      row.diskWrBps = aggWr
      row.netRxBps = aggRx
      row.netTxBps = aggTx

      setTimeline((t) => [...t.slice(-(MAX_POINTS - 1)), row])
      setVmExtras(extras)
      prevRef.current = { byName, at: now }
    } catch (e: unknown) {
      setLoadError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    load()
    const i = setInterval(load, POLL_MS)
    return () => clearInterval(i)
  }, [load])

  const barData = useMemo(
    () =>
      [...filteredMetrics]
        .sort((a, b) => b.memory_pct - a.memory_pct)
        .map((m) => ({
          label: m.name.length > 32 ? `${m.name.slice(0, 30)}…` : m.name,
          mem: Number(m.memory_pct.toFixed(1)),
          fullName: m.name,
        })),
    [filteredMetrics],
  )

  const chartVmSubset = filteredMetrics.slice(0, MAX_VM_LINES)
  const vmLineOverflow = filteredMetrics.length - chartVmSubset.length

  const tooltipStyle = {
    backgroundColor: 'var(--apple-surface)',
    border: '1px solid var(--apple-hairline)',
    borderRadius: '0.5rem',
    boxShadow: '0 25px 50px -12px rgba(0,0,0,0.5)',
  }

  return (
    <PageLayout
      eyebrow="Hypervisor"
      className="min-w-0"
      loading={loading}
      title="Live Metrics"
      icon={<Activity className={`w-6 h-6 ${statusToneClass('info')}`} />}
      subtitle={
        <>
          Running guests from libvirt. Throughput and CPU % use deltas between polls (~
          {POLL_MS / 1000}s); cumulative disk/net counters match VM detail views.
        </>
      }
      actions={
        <>
          <button
            type="button"
            onClick={() => downloadJSON(metrics, 'metrics.json')}
            className="p-2 hover:bg-[var(--surface-hover)] rounded transition"
            title="Export JSON"
            aria-label="Export JSON"
          >
            <Download className="w-4 h-4" aria-hidden="true" />
          </button>
          <button
            type="button"
            onClick={() => downloadCSV(metrics as unknown as Record<string, unknown>[], 'metrics.csv')}
            className="p-2 hover:bg-[var(--surface-hover)] rounded transition"
            title="Export CSV"
            aria-label="Export CSV"
          >
            <Download className={`w-4 h-4 ${statusToneClass('ok')}`} aria-hidden="true" />
          </button>
          <button type="button" onClick={load} className="p-2 hover:bg-[var(--surface-hover)] rounded transition" aria-label="Refresh">
            <RefreshCw className="w-4 h-4" aria-hidden="true" />
          </button>
        </>
      }
      error={loadError}
      errorTitle="Could not load metrics"
      errorHints={loadError ? libvirtErrorHints(loadError) : undefined}
      onErrorRetry={load}
      contentClassName="space-y-6"
    >
      {metrics.length === 0 ? (
        <EmptyState
          icon={<Activity className="w-6 h-6" />}
          title="No running VMs with metrics"
          description="Start a guest or open VM details to see per-domain CPU, memory, and throughput charts."
          primaryAction={
            <Link to="/vms" className="btn-primary text-sm">
              Virtual machines
            </Link>
          }
        />
      ) : (
        <>
          <TahoeToolbar
            search={search}
            onSearchChange={setSearch}
            placeholder="Search VMs…"
            trailing={
              <span className="text-sm text-[var(--text-muted)] shrink-0 pr-2">
                {filteredMetrics.length} guest{filteredMetrics.length !== 1 ? 's' : ''}
              </span>
            }
          />

          {/* Charts */}
          <div className="grid grid-cols-1 xl:grid-cols-2 gap-6 min-w-0 relative z-0">
            <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 p-5 min-w-0">
              <div className="flex items-center justify-between gap-2 mb-3">
                <h2 className="text-sm font-semibold text-[var(--text-primary)] flex items-center gap-2">
                  <Activity className={`w-4 h-4 ${statusToneClass('info')}`} /> Memory % (per VM)
                </h2>
                {vmLineOverflow > 0 && (
                  <span className="text-[10px] text-[var(--text-muted)]">
                    +{vmLineOverflow} more in table / snapshot bar
                  </span>
                )}
              </div>
              <div className="h-[260px] w-full min-w-0 isolate">
                {timeline.length > 1 ? (
                  <ResponsiveContainer width="100%" height="100%">
                    <LineChart data={timeline}>
                      <CartesianGrid strokeDasharray="3 3" stroke="var(--apple-hairline)" />
                      <XAxis dataKey="time" stroke="#475569" fontSize={10} tickLine={false} />
                      <YAxis
                        stroke="#475569"
                        fontSize={10}
                        domain={[0, 100]}
                        tickLine={false}
                        tickFormatter={(v) => `${v}%`}
                      />
                      <Tooltip
                        contentStyle={tooltipStyle}
                        labelStyle={{ color: 'var(--text-secondary)' }}
                        formatter={(value) => [`${value}%`, '']}
                      />
                      <Legend wrapperStyle={{ fontSize: 11, maxHeight: 72, overflowY: 'auto' }} />
                      {chartVmSubset.map((m, i) => (
                        <Line
                          key={m.name}
                          type="monotone"
                          dataKey={chartMemKey(m.name)}
                          name={m.name}
                          stroke={VM_LINE_COLORS[i % VM_LINE_COLORS.length]}
                          strokeWidth={2}
                          dot={false}
                          connectNulls
                        />
                      ))}
                    </LineChart>
                  </ResponsiveContainer>
                ) : (
                  <div className="h-full flex items-center justify-center text-[var(--text-muted)] text-sm">
                    Collecting samples… next point in a few seconds.
                  </div>
                )}
              </div>
            </div>

            <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 p-5 min-w-0">
              <h2 className="text-sm font-semibold text-[var(--text-primary)] flex items-center gap-2 mb-3">
                <Cpu className={`w-4 h-4 ${statusToneClass('warn')}`} /> Guest CPU % (estimated)
              </h2>
              <div className="h-[260px] w-full min-w-0 isolate">
                {timeline.length > 1 ? (
                  <ResponsiveContainer width="100%" height="100%">
                    <LineChart data={timeline}>
                      <CartesianGrid strokeDasharray="3 3" stroke="var(--apple-hairline)" />
                      <XAxis dataKey="time" stroke="#475569" fontSize={10} tickLine={false} />
                      <YAxis
                        stroke="#475569"
                        fontSize={10}
                        domain={[0, 100]}
                        tickLine={false}
                        tickFormatter={(v) => `${v}%`}
                      />
                      <Tooltip
                        contentStyle={tooltipStyle}
                        labelStyle={{ color: 'var(--text-secondary)' }}
                        formatter={(value) => [`${value}%`, '']}
                      />
                      <Legend wrapperStyle={{ fontSize: 11, maxHeight: 72, overflowY: 'auto' }} />
                      {chartVmSubset.map((m, i) => (
                        <Line
                          key={m.name}
                          type="monotone"
                          dataKey={chartCpuKey(m.name)}
                          name={m.name}
                          stroke={VM_LINE_COLORS[i % VM_LINE_COLORS.length]}
                          strokeWidth={2}
                          dot={false}
                          connectNulls
                        />
                      ))}
                    </LineChart>
                  </ResponsiveContainer>
                ) : (
                  <div className="h-full flex items-center justify-center text-[var(--text-muted)] text-sm">
                    Collecting samples…
                  </div>
                )}
              </div>
            </div>

            <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 p-5 min-w-0 xl:col-span-1">
              <h2 className="text-sm font-semibold text-[var(--text-primary)] flex items-center gap-2 mb-3">
                <HardDrive className={`w-4 h-4 ${statusToneClass('ok')}`} /> Disk throughput (all VMs)
              </h2>
              <div className="h-[220px] w-full min-w-0">
                {timeline.length > 1 ? (
                  <ResponsiveContainer width="100%" height="100%">
                    <AreaChart data={timeline}>
                      <defs>
                        <linearGradient id="metricsDiskReadGrad" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="5%" stopColor="#10b981" stopOpacity={0.35} />
                          <stop offset="95%" stopColor="#10b981" stopOpacity={0} />
                        </linearGradient>
                        <linearGradient id="metricsDiskWriteGrad" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="5%" stopColor="#f59e0b" stopOpacity={0.35} />
                          <stop offset="95%" stopColor="#f59e0b" stopOpacity={0} />
                        </linearGradient>
                      </defs>
                      <CartesianGrid strokeDasharray="3 3" stroke="var(--apple-hairline)" />
                      <XAxis dataKey="time" stroke="#475569" fontSize={10} tickLine={false} />
                      <YAxis
                        stroke="#475569"
                        fontSize={10}
                        tickLine={false}
                        tickFormatter={(v: number) => formatThroughput(v)}
                      />
                      <Tooltip
                        contentStyle={tooltipStyle}
                        labelStyle={{ color: 'var(--text-secondary)' }}
                        formatter={(v) => formatThroughput(Number(v))}
                      />
                      <Legend />
                      <Area
                        type="monotone"
                        dataKey="diskRdBps"
                        name="Read"
                        stroke="#10b981"
                        strokeWidth={1.5}
                        fillOpacity={1}
                        fill="url(#metricsDiskReadGrad)"
                      />
                      <Area
                        type="monotone"
                        dataKey="diskWrBps"
                        name="Write"
                        stroke="#f59e0b"
                        strokeWidth={1.5}
                        fillOpacity={1}
                        fill="url(#metricsDiskWriteGrad)"
                      />
                    </AreaChart>
                  </ResponsiveContainer>
                ) : (
                  <div className="h-full flex items-center justify-center text-[var(--text-muted)] text-sm">
                    Collecting samples…
                  </div>
                )}
              </div>
            </div>

            <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 p-5 min-w-0 xl:col-span-1">
              <h2 className="text-sm font-semibold text-[var(--text-primary)] flex items-center gap-2 mb-3">
                <Network className="w-4 h-4 text-[var(--accent)]" /> Network throughput (all VMs)
              </h2>
              <div className="h-[220px] w-full min-w-0">
                {timeline.length > 1 ? (
                  <ResponsiveContainer width="100%" height="100%">
                    <AreaChart data={timeline}>
                      <defs>
                        <linearGradient id="metricsNetRxGrad" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="5%" stopColor="#06b6d4" stopOpacity={0.35} />
                          <stop offset="95%" stopColor="#06b6d4" stopOpacity={0} />
                        </linearGradient>
                        <linearGradient id="metricsNetTxGrad" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="5%" stopColor="#a855f7" stopOpacity={0.35} />
                          <stop offset="95%" stopColor="#a855f7" stopOpacity={0} />
                        </linearGradient>
                      </defs>
                      <CartesianGrid strokeDasharray="3 3" stroke="var(--apple-hairline)" />
                      <XAxis dataKey="time" stroke="#475569" fontSize={10} tickLine={false} />
                      <YAxis
                        stroke="#475569"
                        fontSize={10}
                        tickLine={false}
                        tickFormatter={(v: number) => formatThroughput(v)}
                      />
                      <Tooltip
                        contentStyle={tooltipStyle}
                        labelStyle={{ color: 'var(--text-secondary)' }}
                        formatter={(v) => formatThroughput(Number(v))}
                      />
                      <Legend />
                      <Area
                        type="monotone"
                        dataKey="netRxBps"
                        name="RX"
                        stroke="#06b6d4"
                        strokeWidth={1.5}
                        fillOpacity={1}
                        fill="url(#metricsNetRxGrad)"
                      />
                      <Area
                        type="monotone"
                        dataKey="netTxBps"
                        name="TX"
                        stroke="#a855f7"
                        strokeWidth={1.5}
                        fillOpacity={1}
                        fill="url(#metricsNetTxGrad)"
                      />
                    </AreaChart>
                  </ResponsiveContainer>
                ) : (
                  <div className="h-full flex items-center justify-center text-[var(--text-muted)] text-sm">
                    Collecting samples…
                  </div>
                )}
              </div>
            </div>
          </div>

          {/* Snapshot bar + detail table */}
          <div className="flex flex-col gap-6">
            <div className="tahoe-glass-card p-5 min-w-0">
              <h2 className="text-sm font-semibold text-[var(--text-primary)] mb-3">Memory snapshot</h2>
              <div
                style={{ height: Math.min(420, 48 + barData.length * 36) }}
                className="min-h-[180px]"
              >
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart data={barData} layout="vertical" margin={{ left: 4, right: 8 }}>
                    <CartesianGrid strokeDasharray="3 3" stroke="var(--apple-hairline)" horizontal={false} />
                    <XAxis type="number" domain={[0, 100]} stroke="#475569" fontSize={10} tickFormatter={(v) => `${v}%`} />
                    <YAxis type="category" dataKey="label" width={148} stroke="#64748b" fontSize={10} tickLine={false} />
                    <Tooltip
                      contentStyle={tooltipStyle}
                      formatter={(v) => [`${v}%`, 'Memory']}
                      labelFormatter={(_, p) =>
                        (p?.[0]?.payload as { fullName?: string } | undefined)?.fullName ?? ''
                      }
                    />
                    <Bar dataKey="mem" fill="#3b82f6" radius={[0, 6, 6, 0]} />
                  </BarChart>
                </ResponsiveContainer>
              </div>
            </div>

            <TahoeTableWrap className="min-w-0">
              <table className="apple-table text-sm" aria-label="VM metrics">
                <thead>
                  <tr>
                    <th scope="col">VM</th>
                    <th scope="col">Mem</th>
                    <th scope="col" className="hidden sm:table-cell">CPU</th>
                    <th scope="col" className="hidden md:table-cell">Disk R/W</th>
                    <th scope="col" className="hidden lg:table-cell">Net RX/TX</th>
                    <th scope="col" className="w-10" />
                  </tr>
                </thead>
                <tbody>
                  {filteredMetrics.length === 0 && (
                    <tr><td colSpan={6} className="text-center text-[var(--text-muted)]">No VMs match your search.</td></tr>
                  )}
                  {filteredMetrics.map((m) => {
                      const ex = vmExtras[m.name]
                      return (
                        <tr key={m.name}>
                          <td>
                            <Link
                              to={vmDetailRoute(m.name, m.libvirt_connection)}
                              className={`font-medium truncate block max-w-[220px] md:max-w-xs ${statusActionLinkClasses('info')}`}
                              title={m.name}
                            >
                              {m.name}
                            </Link>
                            <div className="text-[10px] text-[var(--text-muted)]">
                              {m.vcpus} vCPU · {m.memory_used_mb} / {m.memory_total_mb} MB
                            </div>
                          </td>
                          <td>
                            <div className="flex items-center gap-2">
                              <div className="w-20 bg-[var(--surface-hover)] rounded-full h-2 shrink-0">
                                <div
                                  role="progressbar"
                                  aria-label="Memory usage"
                                  aria-valuenow={Math.round(Math.min(100, m.memory_pct))}
                                  aria-valuemin={0}
                                  aria-valuemax={100}
                                  className={`h-2 rounded-full transition-all ${statusBgClass(utilizationTone(m.memory_pct))}`}
                                  style={{ width: `${Math.min(100, m.memory_pct)}%` }}
                                />
                              </div>
                              <span className="text-[var(--text-muted)] tabular-nums">{m.memory_pct.toFixed(0)}%</span>
                            </div>
                          </td>
                          <td className="text-[var(--text-secondary)] tabular-nums hidden sm:table-cell">
                            {ex ? `${ex.cpuPct.toFixed(0)}%` : '—'}
                          </td>
                          <td className="text-[var(--text-secondary)] text-xs hidden md:table-cell">
                            {ex ? (
                              <span>
                                {formatThroughput(ex.rdBps)}{' '}
                                <span className="text-[var(--text-muted)]">/</span> {formatThroughput(ex.wrBps)}
                              </span>
                            ) : (
                              '—'
                            )}
                          </td>
                          <td className="text-[var(--text-secondary)] text-xs hidden lg:table-cell">
                            {ex ? (
                              <span>
                                {formatThroughput(ex.rxBps)}{' '}
                                <span className="text-[var(--text-muted)]">/</span> {formatThroughput(ex.txBps)}
                              </span>
                            ) : (
                              '—'
                            )}
                          </td>
                          <td>
                            <Link
                              to={vmDetailRoute(m.name, m.libvirt_connection)}
                              className="inline-flex p-1 rounded hover:bg-[var(--surface-hover)] text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
                              aria-label={`Open ${m.name}`}
                            >
                              <ChevronRight className="w-4 h-4" />
                            </Link>
                          </td>
                        </tr>
                      )
                    })}
                </tbody>
              </table>
            </TahoeTableWrap>
          </div>
        </>
      )}
    </PageLayout>
  )
}
