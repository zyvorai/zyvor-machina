// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link } from 'react-router'
import { asArray, asRecord } from './JsonInspector'
import { hubLinkClasses, statusBadgeClasses, statusSurfaceClasses, statusToneClass } from '../../utils/semanticColors'

export function ComplianceReportSummary({ report }: { report: Record<string, unknown> }) {
  const summary = typeof report.summary === 'string' ? report.summary : null
  const findings = asArray(report.findings ?? report.items ?? report.violations ?? report.results)
  const critical = report.critical_count ?? report.critical ?? findings.filter((f) => {
    const r = asRecord(f)
    return r?.severity === 'critical' || r?.risk === 'critical'
  }).length

  return (
    <div className="space-y-3">
      {summary && <p className="text-sm text-[var(--text-secondary)]">{summary}</p>}
      <div className="flex flex-wrap gap-3 text-xs">
        <span className="px-2 py-1 rounded bg-[var(--apple-fill-tertiary)] text-[var(--text-secondary)]">{findings.length} finding(s)</span>
        {critical != null && Number(critical) > 0 && (
          <span className={`px-2 py-1 rounded ${statusBadgeClasses('error')}`}>{Number(critical)} critical</span>
        )}
      </div>
      {findings.length > 0 && (
        <ul className="space-y-2 max-h-64 overflow-y-auto">
          {findings.slice(0, 20).map((item, i) => {
            const row = asRecord(item) ?? { detail: String(item) }
            const title = String(row.title ?? row.rule ?? row.policy ?? row.name ?? `Finding ${i + 1}`)
            const detail = String(row.detail ?? row.message ?? row.description ?? '')
            const target = row.target_id ?? row.host_id
            return (
              <li key={title} className="rounded-lg border border-white/[0.06] bg-[var(--apple-surface)] px-3 py-2 text-sm">
                <p className="text-[var(--text-primary)] font-medium">{title}</p>
                {detail && <p className="text-xs text-[var(--text-muted)] mt-0.5">{detail}</p>}
                {target != null && target !== '' && (
                  <Link to={`/platform/zeus/security/firewall/${String(target)}`} className={`text-xs mt-1 inline-block ${hubLinkClasses()}`}>
                    Open firewall target →
                  </Link>
                )}
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}

export function BpfAnomalySummary({ data }: { data: Record<string, unknown> }) {
  const summary = typeof data.summary === 'string' ? data.summary : null
  const anomalies = asArray(data.anomalies ?? data.items ?? data.results)

  return (
    <div className="space-y-3">
      {summary && <p className="text-sm text-[var(--text-secondary)]">{summary}</p>}
      {anomalies.length === 0 ? (
        <p className="text-sm text-[var(--text-muted)]">No anomalies reported by machina-bpfd.</p>
      ) : (
        <ul className="space-y-2 max-h-64 overflow-y-auto">
          {anomalies.slice(0, 15).map((item, i) => {
            const row = asRecord(item) ?? { detail: String(item) }
            return (
              <li key={String(row.id ?? i)} className={`rounded-lg px-3 py-2 text-sm ${statusSurfaceClasses('warn')}`}>
                <p className={statusToneClass('warn')}>
                  {String(row.kind ?? row.type ?? row.title ?? `Anomaly ${i + 1}`)}
                  {row.severity ? ` · ${String(row.severity)}` : ''}
                  {row.hostname ? ` · ${String(row.hostname)}` : ''}
                </p>
                <p className={`text-xs mt-0.5 opacity-70 ${statusToneClass('warn')}`}>{String(row.summary ?? row.detail ?? row.message ?? row.description ?? '')}</p>
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}

export function SupportBundleSummary({ bundle }: { bundle: Record<string, unknown> }) {
  const rows = [
    { label: 'Generated', value: bundle.generated_at ?? bundle.timestamp },
    { label: 'Controller version', value: bundle.version ?? bundle.controller_version },
    { label: 'Hosts', value: bundle.host_count ?? (asArray(bundle.hosts).length || undefined) },
    { label: 'VMs', value: bundle.vm_count ?? (asArray(bundle.vms).length || undefined) },
    { label: 'Failed tasks (24h)', value: bundle.failed_tasks_24h ?? bundle.failed_tasks },
    { label: 'Cluster health', value: bundle.cluster_status ?? bundle.health },
  ].filter((r) => r.value != null && r.value !== '')

  return (
    <ul className="grid gap-2 sm:grid-cols-2 text-sm">
      {rows.map((r) => (
        <li key={r.label} className="rounded-lg border border-white/[0.06] bg-[var(--apple-surface)] px-3 py-2">
          <span className="text-xs text-[var(--text-muted)] block">{r.label}</span>
          <span className="text-[var(--text-primary)]">{String(r.value)}</span>
        </li>
      ))}
    </ul>
  )
}
