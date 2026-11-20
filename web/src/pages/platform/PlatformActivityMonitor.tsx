// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { usePlatformTabState } from '../../hooks/usePlatformTabState'
import { Link } from 'react-router'
import { Activity } from 'lucide-react'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import DetailTabs from '../../components/platform/DetailTabs'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import { getFleetActivity, type FleetActivityOverview } from '../../api/platform'
import { formatUserError } from '../../utils/apiError'
import { statusToneClass, utilizationBarClass, hubLinkClasses } from '../../utils/semanticColors'
import { useExpandable } from '../../hooks/useExpandable'
import { ExpandableToggle } from '../../components/ui/ExpandableToggle'

const ACTIVITY_TABS = ['vms', 'hosts'] as const

function bar(label: string, value: number, tone: 'cpu' | 'mem' | 'io' | 'thermal') {
  // Thermal is a temperature in °C, not a percentage — its own thresholds and
  // unit. (It was using the IO/PSI thresholds, so a normal 30°C read as red.)
  const thresholds =
    tone === 'thermal' ? { warn: 70, error: 85 } : tone === 'io' ? { warn: 20, error: 50 } : { warn: 60, error: 85 }
  const unit = tone === 'thermal' ? '°C' : '%'
  return (
    <div className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
      <span className="w-14 shrink-0">{label}</span>
      <div className="flex-1 h-1.5 rounded-full bg-[var(--apple-fill-tertiary)] overflow-hidden">
        <div className={`h-full rounded-full ${utilizationBarClass(value, thresholds)}`} style={{ width: `${Math.min(100, Math.max(0, value))}%` }} />
      </div>
      <span className="w-10 text-right">{value.toFixed(0)}{unit}</span>
    </div>
  )
}

export default function PlatformActivityMonitor() {
  const [tab, setTab] = usePlatformTabState(ACTIVITY_TABS, { defaultTab: 'vms' })
  const [data, setData] = useState<FleetActivityOverview | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const vmList = useExpandable(data?.top_vms ?? [], 20)
  const hostList = useExpandable(data?.hosts ?? [], 20)

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      setData(await getFleetActivity())
    } catch (e: unknown) {
      setError(formatUserError(e))
      setData(null)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/operations" label="Operations" />}
      title="Activity Monitor"
      subtitle="Fleet-wide CPU, memory, and Linux PSI — macOS Activity Monitor for your hypervisors."
      icon={<Activity className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={<PlatformRefreshButton onClick={() => void load()} />}
      contentLoading={loading && !data}
      contentClassName="space-y-4"
    >
      <div className="flex flex-wrap gap-3 text-xs">
        <Link to="/platform/placement" className={hubLinkClasses()}>HA status & fence events →</Link>
        <Link to="/platform/developer" className={hubLinkClasses()}>Developer SDK →</Link>
        <Link to="/platform/reports?tab=runbooks" className={hubLinkClasses()}>Ops runbooks →</Link>
        <Link to="/platform/settings?section=integrations" className={hubLinkClasses()}>Classic tools →</Link>
      </div>
      {data && <p className="text-sm text-[var(--text-muted)]">{data.summary}</p>}

      <DetailTabs
        primary={[
          { id: 'vms', label: 'VMs' },
          { id: 'hosts', label: 'Hosts' },
        ]}
        active={tab}
        onChange={setTab}
      />

      {!loading && tab === 'vms' && (
        data?.top_vms.length ? (
          <ul className="space-y-2" id={vmList.listId}>
            {vmList.shown.map((vm) => {
              // Per-VM utilization (used / this VM's own allocation) — matches
              // the "used / total MiB" text beside the bar. Previously divided by
              // the fleet's peak VM, so the bar contradicted its own label.
              const memPct = Math.min(100, (vm.memory_used_mib / Math.max(1, vm.memory_mib)) * 100)
              return (
                <li key={vm.vm_id} className="platform-mac-stat rounded-xl border border-white/[0.06] bg-[var(--apple-surface)] p-4">
                  <div className="flex justify-between items-center mb-2">
                    <Link to={`/platform/vms/${vm.vm_id}`} className="font-medium text-[var(--text-primary)] hover:text-[var(--accent)]">{vm.vm_name}</Link>
                    <span className="text-xs text-[var(--text-muted)]">{vm.memory_used_mib} / {vm.memory_mib} MiB</span>
                  </div>
                  <div className="space-y-1.5">
                    {bar('Memory', memPct, 'mem')}
                    {bar('CPU', vm.cpu_percent, 'cpu')}
                  </div>
                </li>
              )
            })}
          </ul>
        ) : (
          <PlatformEmptyState title="No running VMs" subtitle="Start a VM to see live CPU and memory usage." />
        )
      )}
      {!loading && tab === 'vms' && vmList.showToggle && (
        <ExpandableToggle expanded={vmList.expanded} hidden={vmList.hidden} listId={vmList.listId} onToggle={vmList.toggle} noun="VMs" />
      )}

      {!loading && tab === 'hosts' && (
        data?.hosts.length ? (
          <MacGlassPanel title="Hypervisor hosts" subtitle="Inventory CPU/memory + Linux PSI when agent online">
            <div id={hostList.listId}>
            {hostList.shown.map((h) => (
              <div key={h.host_id} className="py-3 border-b border-white/[0.04] last:border-0">
                <MacListRow
                  title={h.hostname}
                  subtitle={`${h.vm_count} VM(s) · ${h.state} · ${h.status}`}
                  badge={
                    h.status !== 'ok' && h.status !== 'offline' ? (
                      <span className={`text-xs ${statusToneClass('warn')}`}>{h.status}</span>
                    ) : undefined
                  }
                  href={`/platform/hosts/${h.host_id}`}
                />
                <div className="mt-2 space-y-1 pl-1 max-w-md">
                  {bar('CPU', h.cpu_percent, 'cpu')}
                  {bar('Memory', h.memory_percent, 'mem')}
                  {h.cpu_pressure_pct > 0 && bar('CPU PSI', h.cpu_pressure_pct, 'io')}
                  {h.memory_pressure_pct > 0 && bar('Mem PSI', h.memory_pressure_pct, 'io')}
                  {h.io_pressure_pct > 0 && bar('IO PSI', h.io_pressure_pct, 'io')}
                  {h.thermal_max_c > 0 && bar('Thermal', Math.min(100, h.thermal_max_c), 'thermal')}
                </div>
              </div>
            ))}
            </div>
            {hostList.showToggle && (
              <div className="pt-3">
                <ExpandableToggle expanded={hostList.expanded} hidden={hostList.hidden} listId={hostList.listId} onToggle={hostList.toggle} noun="hosts" />
              </div>
            )}
          </MacGlassPanel>
        ) : (
          <PlatformEmptyState title="No hosts" subtitle="Enroll hypervisors to monitor fleet activity." />
        )
      )}
    </PlatformPageChrome>
  )
}
