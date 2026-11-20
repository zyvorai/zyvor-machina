// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Server, X } from 'lucide-react'
import SecurityLensLayout from '../../../components/platform/SecurityLensLayout'
import { getFirewallOverview, getFirewallServices, type AllowedService } from '../../../api/zeusFirewall'
import { formatUserError } from '../../../utils/apiError'
import { firewallRiskClass, formatAllowedFrom } from '../../../utils/firewallDisplay'
import { statusToneClass } from '../../../utils/semanticColors'
import { useExpandable } from '../../../hooks/useExpandable'
import { ExpandableToggle } from '../../../components/ui/ExpandableToggle'

type ServiceRow = AllowedService & { target: string; targetId: string; key: string }

export default function PlatformFirewallServices() {
  const [services, setServices] = useState<ServiceRow[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [filter, setFilter] = useState('')

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      const ov = await getFirewallOverview()
      // Each target's services query is independent — fire concurrently.
      const listsByTarget = await Promise.all(ov.targets.map((t) => getFirewallServices(t.id)))
      const all: ServiceRow[] = []
      const seen = new Set<string>()
      ov.targets.forEach((t, i) => {
        for (const svc of listsByTarget[i]) {
          const key = `${t.id}:${svc.protocol}:${svc.port}:${svc.name}`
          if (seen.has(key)) continue
          seen.add(key)
          all.push({ ...svc, target: t.name, targetId: t.id, key })
        }
      })
      all.sort((a, b) => {
        const ra = String(a.status).toLowerCase()
        const rb = String(b.status).toLowerCase()
        if (ra !== rb) return ra === 'critical' ? -1 : rb === 'critical' ? 1 : 0
        return a.name.localeCompare(b.name)
      })
      setServices(all)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const visible = useMemo(() => {
    const q = filter.trim().toLowerCase()
    if (!q) return services
    return services.filter(
      (s) =>
        s.name.toLowerCase().includes(q)
        || s.target.toLowerCase().includes(q)
        || String(s.port).includes(q)
        || s.protocol.toLowerCase().includes(q),
    )
  }, [services, filter])

  const criticalCount = services.filter((s) => String(s.status).toLowerCase() === 'critical').length
  const serviceList = useExpandable(visible, 30)

  return (
    <SecurityLensLayout
      testId="platform-firewall-services-page"
      title="Allowed Apps & Services"
      subtitle={`${visible.length} service${visible.length === 1 ? '' : 's'} · deduplicated rules per host`}
      icon={<Server className="w-6 h-6 text-[var(--text-muted)]" />}
      loading={loading && services.length === 0}
      error={error}
      onRefresh={() => void load()}
      stats={[
        { label: 'Services', value: String(services.length), icon: <Server className="w-4 h-4" /> },
        { label: 'Critical', value: String(criticalCount), tone: criticalCount > 0 ? 'warn' : 'default' },
        { label: 'Visible', value: String(visible.length) },
      ]}
      panelTitle="Allowed services"
      isEmpty={visible.length === 0}
      emptyTitle={services.length === 0 ? 'No allowed services yet' : 'No services match this filter'}
      emptySubtitle={services.length === 0 ? 'Firewall profiles will populate when Zeus agents sync rules.' : 'Clear the search filter to see all services.'}
    >
      <div className="flex flex-wrap gap-2 mb-4">
        <div className="relative max-w-xs flex-1">
          <input
            className={`input text-sm w-full ${filter ? 'pr-8' : 'pr-4'}`}
            aria-label="Filter firewall services"
            placeholder="Filter by name, port, or host…"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
          {filter && (
            <button type="button" aria-label="Clear filter" onClick={() => setFilter('')}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-[var(--text-muted)] hover:text-[var(--text-primary)]">
              <X className="w-4 h-4" />
            </button>
          )}
        </div>
        <span className="text-sm text-[var(--text-muted)] self-center">{visible.length} service(s)</span>
      </div>
      <div className="space-y-2" id={serviceList.listId}>
        {serviceList.shown.map((s) => (
          <article
            key={s.key}
            className="rounded-xl border border-white/[0.08] bg-[var(--apple-surface)] px-4 py-3 flex flex-wrap items-start justify-between gap-3"
          >
            <div className="min-w-0 flex-1">
              <p className="font-medium text-[var(--text-primary)] text-[15px] leading-snug">{s.name}</p>
              <p className="text-[13px] text-[var(--text-muted)] mt-1 leading-relaxed">
                <span className="text-[var(--text-secondary)]">{s.target}</span>
                {' · '}
                <span className="font-mono text-[var(--text-secondary)]">{s.protocol}/{s.port}</span>
              </p>
              <p className="text-[13px] text-[var(--text-muted)] mt-1">
                Allowed from: <span className="text-[var(--text-secondary)]">{formatAllowedFrom(s.allowed_from)}</span>
              </p>
              {s.recommendation && (
                <p className={`text-[13px] mt-2 leading-relaxed ${statusToneClass('warn')}`}>{s.recommendation}</p>
              )}
            </div>
            <span className={`text-xs px-2.5 py-1 rounded-full border shrink-0 ${firewallRiskClass(String(s.status))}`}>
              {String(s.status)}
            </span>
          </article>
        ))}
      </div>
      {serviceList.showToggle && (
        <ExpandableToggle expanded={serviceList.expanded} hidden={serviceList.hidden} listId={serviceList.listId} onToggle={serviceList.toggle} noun="services" />
      )}
    </SecurityLensLayout>
  )
}
