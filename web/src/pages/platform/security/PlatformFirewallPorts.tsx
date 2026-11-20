// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Network } from 'lucide-react'
import { MacGlassPanel, MacListRow } from '../../../components/platform/mac/PlatformMacUi'
import SecurityLensLayout from '../../../components/platform/SecurityLensLayout'
import { explainFirewall, getFirewallOverview, getFirewallPorts, type OpenPort } from '../../../api/zeusFirewall'
import { formatUserError } from '../../../utils/apiError'
import { riskTone, statusBadgeClasses } from '../../../utils/semanticColors'
import { useExpandable } from '../../../hooks/useExpandable'
import { ExpandableToggle } from '../../../components/ui/ExpandableToggle'

export default function PlatformFirewallPorts() {
  const [ports, setPorts] = useState<Array<OpenPort & { target: string; targetId: string }>>([])
  const [filter, setFilter] = useState('all')
  const [explain, setExplain] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      const ov = await getFirewallOverview()
      // Per-target ports fetch and the explain call are both independent of each other —
      // fire concurrently instead of one after another.
      const [portsByTarget] = await Promise.all([
        Promise.all(ov.targets.map((t) => getFirewallPorts(t.id))),
        ov.targets[0]
          ? explainFirewall(ov.targets[0].id, 'Summarize open port risk').then((r) => setExplain(r.recommendation))
          : Promise.resolve(),
      ])
      const all: Array<OpenPort & { target: string; targetId: string }> = []
      ov.targets.forEach((t, i) => {
        for (const port of portsByTarget[i]) {
          all.push({ ...port, target: t.name, targetId: t.id })
        }
      })
      setPorts(all)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const filtered = ports.filter((p) => {
    if (filter === 'critical') return String(p.risk).toLowerCase() === 'critical'
    if (filter === 'warning') return String(p.risk).toLowerCase() === 'warning'
    return true
  })

  const criticalCount = ports.filter((p) => String(p.risk).toLowerCase() === 'critical').length
  const warningCount = ports.filter((p) => String(p.risk).toLowerCase() === 'warning').length
  const portList = useExpandable(filtered, 30)

  return (
    <SecurityLensLayout
      testId="platform-firewall-ports-page"
      title="Open Ports"
      subtitle={`${filtered.length} listening port${filtered.length === 1 ? '' : 's'} across the fleet`}
      icon={<Network className="w-6 h-6 text-[var(--text-muted)]" />}
      loading={loading && ports.length === 0}
      error={error}
      onRefresh={() => void load()}
      stats={[
        { label: 'Total ports', value: String(ports.length), icon: <Network className="w-4 h-4" /> },
        { label: 'Critical', value: String(criticalCount), tone: criticalCount > 0 ? 'warn' : 'default' },
        { label: 'Warning', value: String(warningCount), tone: warningCount > 0 ? 'warn' : 'default' },
      ]}
      insight={explain ? (
        <MacGlassPanel title="Zeus insight" subtitle="Exposure recommendation">
          <p className="text-sm text-[var(--text-secondary)]">{explain}</p>
        </MacGlassPanel>
      ) : undefined}
      filters={[
        { id: 'all', label: 'All' },
        { id: 'critical', label: 'Critical' },
        { id: 'warning', label: 'Warning' },
      ]}
      filterValue={filter}
      onFilterChange={setFilter}
      panelTitle={`${filtered.length} open ports`}
      isEmpty={filtered.length === 0}
      emptyTitle={ports.length === 0 ? 'No open ports detected' : 'No ports match this filter'}
      emptySubtitle={ports.length === 0 ? 'Zeus Firewall will list listening ports when agents report inventory.' : 'Try a different risk filter.'}
    >
      <div className="rounded-xl border border-white/[0.06] overflow-hidden" id={portList.listId}>
        {portList.shown.map((p) => (
          <MacListRow
            key={`${p.targetId}-${p.port}-${p.protocol}`}
            href={`/platform/zeus/security/firewall/${p.targetId}`}
            title={`${p.port}/${p.protocol} · ${p.service_name}`}
            subtitle={`${p.target} · ${p.bind_address}${p.process ? ` · ${p.process}` : ''}`}
            badge={
              <span className={`text-xs px-2 py-0.5 rounded-full ${statusBadgeClasses(riskTone(String(p.risk)))}`}>
                {String(p.risk)}
              </span>
            }
          />
        ))}
      </div>
      {portList.showToggle && (
        <ExpandableToggle expanded={portList.expanded} hidden={portList.hidden} listId={portList.listId} onToggle={portList.toggle} noun="ports" />
      )}
    </SecurityLensLayout>
  )
}
