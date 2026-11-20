// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router'
import { PieChart } from 'lucide-react'
import ErrorBanner from '../../components/ErrorBanner'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import DetailTabs from '../../components/platform/DetailTabs'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import RunbookExecutionSheet, { parseRunbookStepsJson, type RunbookExecutionResult } from '../../components/platform/RunbookExecutionSheet'
import PlatformPageChrome, { PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import { usePlatformTabState } from '../../hooks/usePlatformTabState'
import {
  getCapacityReport,
  getFinOpsReport,
  getOperationsOverview,
  getOpsShowback,
  listOpsRunbookExecutions,
  listOpsRunbooks,
  executeOpsRunbook,
  listProjects,
  type CapacityReport,
  type FinOpsReport,
  type OpsRunbookCatalogItem,
  type OpsRunbookExecution,
  type OpsShowbackOverview,
  type OperationsOverview,
  type ProjectRow,
} from '../../api/platform'
import {
  downloadAiCapacityExport,
  downloadAiComplianceExport,
  downloadAiCompliancePdf,
  downloadAiCostExport,
  downloadCostAttributionExport,
  getAiCapacity,
  getAiCompliance,
  getAiCost,
  getAiSecurity,
  getAutopilotHistory,
  getCostAttribution,
  getCostBudget,
  migrationReadinessReport,
  runAutopilotSafe,
  type AutopilotHistoryEntry,
  type CapacityPlan,
  type CostAnalysis,
  type CostAttributionReport,
  type ComplianceReport,
  type CostBudgetReport,
  type MigrationReadinessReport,
  type SecurityReport,
} from '../../api/ai'
import { getFirewallExposureFinOps, downloadFirewallExposureFinOpsExport, type ExposureFinOpsReport } from '../../api/zeusFirewall'
import { formatUserError } from '../../utils/apiError'
import { installStateTone } from '../../components/platform/GuestAgentDiagnosticsPanel'
import { statusPillClasses, statusToneClass } from '../../utils/semanticColors'
import { useToastContext } from '../../contexts/ToastContext'

type TabId = 'reports' | 'runbooks' | 'showback'

const REPORT_TABS = [
  { id: 'reports' as const, label: 'Reports' },
  { id: 'runbooks' as const, label: 'Runbooks' },
  { id: 'showback' as const, label: 'Showback' },
]

export default function PlatformReports({ embedded }: { embedded?: boolean } = {}) {
  const toast = useToastContext()
  const [tab, setTab] = usePlatformTabState<TabId>(REPORT_TABS.map((t) => t.id), { defaultTab: 'reports' })

  const [projects, setProjects] = useState<ProjectRow[]>([])
  const [cap, setCap] = useState<CapacityReport | null>(null)
  const [finops, setFinops] = useState<FinOpsReport | null>(null)
  const [cost, setCost] = useState<CostAnalysis | null>(null)
  const [aiCap, setAiCap] = useState<CapacityPlan | null>(null)
  const [security, setSecurity] = useState<SecurityReport | null>(null)
  const [compliance, setCompliance] = useState<ComplianceReport | null>(null)
  const [autopilotHistory, setAutopilotHistory] = useState<AutopilotHistoryEntry[]>([])
  const [attribution, setAttribution] = useState<CostAttributionReport | null>(null)
  const [budget, setBudget] = useState<CostBudgetReport | null>(null)
  const [exposureFinops, setExposureFinops] = useState<ExposureFinOpsReport | null>(null)
  const [opsOverview, setOpsOverview] = useState<OperationsOverview | null>(null)
  const [runbooks, setRunbooks] = useState<OpsRunbookCatalogItem[]>([])
  const [executions, setExecutions] = useState<OpsRunbookExecution[]>([])
  const [showback, setShowback] = useState<OpsShowbackOverview | null>(null)
  const [runbookBusy, setRunbookBusy] = useState<string | null>(null)
  const [runbookError, setRunbookError] = useState<{ label: string; message: string } | null>(null)
  const [runbookResult, setRunbookResult] = useState<RunbookExecutionResult | null>(null)
  const [runbookSheetOpen, setRunbookSheetOpen] = useState(false)
  const [expandedExecution, setExpandedExecution] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [migrationReport, setMigrationReport] = useState<MigrationReadinessReport | null>(null)
  const [migrationBusy, setMigrationBusy] = useState(false)
  const [autopilotBusy, setAutopilotBusy] = useState(false)

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      const [p, c, f, costR, capR, secR, compR, hist, attrR, budgetR, expR] = await Promise.all([
        listProjects(),
        getCapacityReport(),
        getFinOpsReport(),
        getAiCost().catch(() => null),
        getAiCapacity().catch(() => null),
        getAiSecurity().catch(() => null),
        getAiCompliance().catch(() => null),
        getAutopilotHistory(10).catch(() => []),
        getCostAttribution().catch(() => null),
        getCostBudget().catch(() => null),
        getFirewallExposureFinOps().catch(() => null),
      ])
      setProjects(p)
      setCap(c)
      setFinops(f)
      setCost(costR)
      setAiCap(capR)
      setSecurity(secR)
      setCompliance(compR)
      setAutopilotHistory(hist)
      setAttribution(attrR)
      setBudget(budgetR)
      setExposureFinops(expR)
      if (tab === 'runbooks' || tab === 'showback') {
        const [ov, rb, ex, sb] = await Promise.all([
          getOperationsOverview().catch(() => null),
          listOpsRunbooks().catch(() => []),
          listOpsRunbookExecutions(15).catch(() => []),
          getOpsShowback().catch(() => null),
        ])
        setOpsOverview(ov)
        setRunbooks(rb)
        setExecutions(ex)
        setShowback(sb)
      }
    } catch (e: unknown) { setError(formatUserError(e)) }
    finally { setLoading(false) }
  }, [tab])

  useEffect(() => { void load() }, [load])

  const runRunbook = async (incident: string, title: string, context: Record<string, unknown> = {}) => {
    setRunbookBusy(incident)
    setRunbookError(null)
    try {
      const r = await executeOpsRunbook(incident, context)
      setRunbookResult({
        title: r.title || title,
        summary: r.summary,
        steps: r.steps ?? [],
        commands: r.commands ?? [],
      })
      setRunbookSheetOpen(true)
      toast.success(r.summary)
      await load()
    } catch (e: unknown) {
      const message = formatUserError(e)
      setRunbookError({ label: title, message })
      toast.error(message)
    } finally {
      setRunbookBusy(null)
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      hideHeader={embedded}
      compact={embedded}
      error={error}
      onErrorRetry={() => void load()}
      loading={loading}
      title={embedded ? undefined : 'Reports'}
      subtitle={embedded ? undefined : 'Cost Guardian, FinOps, operations runbooks, and compliance showback.'}
      icon={embedded ? undefined : <PieChart className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={embedded ? undefined : <PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-4 xl:space-y-0 xl:columns-2 xl:gap-4 [&>*]:break-inside-avoid xl:[&>*]:mb-4 xl:[&>*:first-child]:[column-span:all]"
    >
      <DetailTabs primary={REPORT_TABS} active={tab} onChange={setTab} />

      {!loading && tab === 'runbooks' && (
        <>
          {runbookError && (
            <ErrorBanner
              title={`${runbookError.label} failed`}
              headline={runbookError.message}
              hints={[
                'Verify the controller is reachable and you have operator permissions.',
                'Open Operations hub to review failed tasks from the same window.',
              ]}
              onDismiss={() => setRunbookError(null)}
            />
          )}
          {opsOverview && (
            <div className="apple-metric-band">
              {[
                { label: 'Runbooks', value: String(opsOverview.runbook_count) },
                { label: '24h executions', value: String(opsOverview.executions_24h) },
                { label: 'Fleet grade', value: opsOverview.compliance_grade },
              ].map((s) => (
                <div key={s.label} className="min-w-0">
                  <div className="apple-metric-value">{s.value}</div>
                  <div className="apple-metric-label">{s.label}</div>
                </div>
              ))}
            </div>
          )}
          <MacGlassPanel title="Runbook catalog" subtitle="Execute incident playbooks — records steps in execution history.">
            <ul className="space-y-2 text-sm">
              {runbooks.length === 0 ? (
                <PlatformEmptyState
                  title="No runbooks"
                  subtitle="Runbook catalog loads from the controller — check connectivity and retry."
                  action={
                    <button type="button" className="btn-secondary text-xs" onClick={() => void load()}>
                      Retry catalog
                    </button>
                  }
                />
              ) : runbooks.map((rb) => (
                <li key={rb.id} className="flex flex-wrap items-center justify-between gap-2 border-b border-white/[0.04] pb-2">
                  <div>
                    <p className="text-[var(--text-primary)]">{rb.title}</p>
                    <p className="text-xs text-[var(--text-muted)]">{rb.category} · {rb.severity}{rb.auto_trigger ? ` · trigger: ${rb.auto_trigger}` : ''}</p>
                  </div>
                  <button type="button" className="btn-secondary text-xs shrink-0" disabled={runbookBusy === rb.incident} onClick={() => void runRunbook(rb.incident, rb.title)}>
                    {runbookBusy === rb.incident ? 'Running…' : 'Execute'}
                  </button>
                </li>
              ))}
            </ul>
          </MacGlassPanel>
          <MacGlassPanel title="Recent executions" subtitle="Operator runbook history.">
            {executions.length === 0 ? (
              <PlatformEmptyState
                title="No runbook executions yet"
                subtitle="Execute a catalog playbook to record steps and commands here."
              />
            ) : (
              <ul className="text-xs space-y-2 text-[var(--text-muted)]">
                {executions.map((ex) => {
                  const steps = parseRunbookStepsJson(ex.steps_json)
                  const expanded = expandedExecution === ex.id
                  return (
                    <li key={ex.id} className="border-b border-white/[0.04] pb-2">
                      <button
                        type="button"
                        className="text-left w-full"
                        onClick={() => setExpandedExecution(expanded ? null : ex.id)}
                      >
                        <span className="text-[var(--text-secondary)]">{ex.incident}</span> — {ex.summary}
                        <span className="text-[var(--text-faint)] block">{new Date(ex.created_at).toLocaleString()}</span>
                      </button>
                      {expanded && steps.length > 0 && (
                        <ol className="mt-2 list-decimal list-inside text-[var(--text-muted)] space-y-1">
                          {steps.map((s, i) => <li key={`${ex.id}-${i}`}>{s}</li>)}
                        </ol>
                      )}
                    </li>
                  )
                })}
              </ul>
            )}
          </MacGlassPanel>
          <RunbookExecutionSheet
            open={runbookSheetOpen}
            onClose={() => setRunbookSheetOpen(false)}
            result={runbookResult}
          />
        </>
      )}

      {!loading && tab === 'showback' && !showback && (
        <PlatformEmptyState
          title="Showback unavailable"
          subtitle="Compliance showback loads from the controller — check connectivity and retry."
          action={
            <button type="button" className="btn-secondary text-xs" onClick={() => void load()}>
              Retry showback
            </button>
          }
        />
      )}

      {!loading && tab === 'showback' && showback && (
        <>
          <MacGlassPanel title="Compliance showback" subtitle={showback.summary}>
            <p className={`text-2xl font-bold -mt-2 ${statusToneClass('ok')}`}>
              ${showback.total_cost_usd.toFixed(0)}<span className="text-sm font-normal text-[var(--text-muted)]"> / mo</span>
            </p>
            <p className="text-sm text-[var(--text-muted)] mt-1">Fleet compliance grade: {showback.fleet_grade}</p>
            {compliance && (
              <div className="flex flex-wrap gap-2 mt-3">
                <button type="button" className="btn-secondary text-xs" onClick={() => void downloadAiComplianceExport().catch((e: unknown) => toast.error(formatUserError(e)))} data-testid="reports-compliance-export-url">
                  Compliance export (HTML)
                </button>
                <button type="button" className="btn-secondary text-xs" onClick={() => void downloadAiCompliancePdf().catch((e: unknown) => toast.error(formatUserError(e)))} data-testid="reports-compliance-pdf-url">
                  Compliance PDF
                </button>
                <button
                  type="button"
                  className="btn-secondary text-xs"
                  onClick={() => void downloadAiComplianceExport().catch((e: unknown) => toast.error(formatUserError(e)))}
                >
                  Open compliance report
                </button>
                <button
                  type="button"
                  className="btn-secondary text-xs"
                  onClick={() => void downloadAiCompliancePdf().catch((e: unknown) => toast.error(formatUserError(e)))}
                >
                  Download compliance PDF
                </button>
              </div>
            )}
            <div className="overflow-x-auto mt-4">
              <table className="w-full text-sm" aria-label="Project costs">
                <thead>
                  <tr className="text-left text-[var(--text-muted)] border-b border-white/[0.06]">
                    <th scope="col" className="py-2 pr-2">Project</th>
                    <th scope="col" className="py-2 pr-2">Cost</th>
                    <th scope="col" className="py-2 pr-2">Grade</th>
                    <th scope="col" className="py-2 pr-2">VMs</th>
                  </tr>
                </thead>
                <tbody>
                  {showback.lines.map((line) => (
                    <tr key={line.project_name} className="border-b border-white/[0.04] text-[var(--text-primary)]">
                      <td className="py-2 pr-2">{line.project_name}</td>
                      <td className="py-2 pr-2">${line.cost_usd.toFixed(0)}/mo</td>
                      <td className="py-2 pr-2">{line.compliance_grade}</td>
                      <td className="py-2 pr-2">{line.vm_count}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </MacGlassPanel>
        </>
      )}

      {!loading && tab === 'reports' && !error && !cap && (
        <PlatformEmptyState
          title="Reports unavailable"
          subtitle="Capacity and FinOps reports load from the controller — check connectivity and retry."
          action={
            <button type="button" className="btn-secondary text-xs" onClick={() => void load()}>
              Retry
            </button>
          }
        />
      )}

      {!loading && tab === 'reports' && cap && (
        <>
          <MacGlassPanel
            title="AI migration readiness"
            subtitle="Live QGA for running VMs; GuestKit offline disk scan when stopped (GUESTKIT_ENABLED)"
            action={
              <div className="flex flex-wrap gap-2">
                <button
                  type="button"
                  className="btn-secondary text-xs"
                  disabled={migrationBusy}
                  onClick={() => {
                    setMigrationBusy(true)
                    void migrationReadinessReport()
                      .then(setMigrationReport)
                      .catch((e: unknown) => toast.error(formatUserError(e)))
                      .finally(() => setMigrationBusy(false))
                  }}
                >
                  {migrationBusy ? 'Generating…' : 'Generate report'}
                </button>
                {migrationReport && (migrationReport.rows?.length ?? 0) > 0 && (
                  <button
                    type="button"
                    className="btn-secondary text-xs"
                    onClick={() => {
                      const header = 'vm_id,vm_name,readiness_percent,install_state,qga_gaps,remediation'
                      const lines = migrationReport.rows.map((row) =>
                        [
                          row.vm_id,
                          row.vm_name,
                          row.readiness_percent,
                          row.install_state,
                          (row.qga_gaps ?? []).join('; '),
                          (row.remediation ?? []).join('; '),
                        ]
                          .map((c) => `"${String(c).replace(/"/g, '""')}"`)
                          .join(','),
                      )
                      const blob = new Blob([[header, ...lines].join('\n')], { type: 'text/csv' })
                      const url = URL.createObjectURL(blob)
                      const a = document.createElement('a')
                      a.href = url
                      a.download = 'migration-readiness.csv'
                      a.click()
                      URL.revokeObjectURL(url)
                    }}
                  >
                    Export CSV
                  </button>
                )}
              </div>
            }
          >
            {migrationReport ? (
              <div className="space-y-3 text-sm">
                <p className="text-[var(--text-primary)]">{migrationReport.executive_summary}</p>
                {migrationReport.prioritized_remediation.length > 0 && (
                  <ul className="text-xs text-[var(--text-muted)] list-disc pl-4">
                    {migrationReport.prioritized_remediation.slice(0, 6).map((r) => (
                      <li key={r}>{r}</li>
                    ))}
                  </ul>
                )}
                <div className="overflow-x-auto">
                  <table className="w-full text-xs" aria-label="VM migration readiness">
                    <thead>
                      <tr className="text-left text-[var(--text-muted)]">
                        <th scope="col" className="py-1 pr-2">VM</th>
                        <th scope="col" className="py-1 pr-2">Ready</th>
                        <th scope="col" className="py-1 pr-2">Assurance</th>
                        <th scope="col" className="py-1">Gaps</th>
                      </tr>
                    </thead>
                    <tbody>
                      {migrationReport.rows.map((row) => (
                        <tr key={row.vm_id} className="border-t border-white/[0.04]">
                          <td className="py-1 pr-2">
                            <Link
                              to={`/platform/vms/${row.vm_id}?tab=guestHealth`}
                              className="text-[var(--link)]/90 hover:underline"
                            >
                              {row.vm_name}
                            </Link>
                          </td>
                          <td className="py-1 pr-2">{row.readiness_percent}%</td>
                          <td className="py-1 pr-2">
                            <span
                              className={statusPillClasses(
                                row.assurance_mode === 'offline_guestkit' ? 'warn' : installStateTone(row.install_state),
                              )}
                              title={row.guestkit_summary}
                            >
                              {row.assurance_mode === 'offline_guestkit'
                                ? 'GuestKit'
                                : row.assurance_mode === 'live_qga'
                                  ? 'QGA'
                                  : row.install_state}
                            </span>
                          </td>
                          <td className="py-1 text-[var(--text-muted)]" title={row.guestkit_summary ?? row.remediation.join(' · ')}>
                            {row.qga_gaps.join('; ') || '—'}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : (
              <p className="text-sm text-[var(--text-muted)]">
                One-click report for hypervisor migration: guest OS inventory, QGA gaps, and prioritized remediation.
              </p>
            )}
          </MacGlassPanel>
          <div className="apple-metric-band">
            {[
              { label: 'Online hosts', value: String(cap.hosts_online) },
              { label: 'Running VMs', value: String(cap.running_vms) },
              { label: 'Memory headroom', value: `${cap.memory_headroom_mib} MiB` },
              { label: 'Avg CPU', value: `${cap.avg_cpu_percent.toFixed(0)}%` },
            ].map((s) => (
              <div key={s.label} className="min-w-0">
                <div className="apple-metric-value">{s.value}</div>
                <div className="apple-metric-label">{s.label}</div>
              </div>
            ))}
          </div>
          {(cap.storage_capacity_gib != null && cap.storage_capacity_gib > 0) ||
          (cap.estimated_small_vms_addable != null && cap.estimated_small_vms_addable > 0) ||
          (cap.planner_recommendations?.length ?? 0) > 0 ? (
            <MacGlassPanel title="Fleet capacity planner" subtitle="Controller capacity report merged with AI planner forecasts.">
              <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4 text-sm -mt-2">
                {cap.storage_capacity_gib != null && cap.storage_capacity_gib > 0 && (
                  <p>
                    Storage:{' '}
                    <span className="text-[var(--text-primary)]">
                      {cap.storage_used_gib ?? 0} / {cap.storage_capacity_gib} GiB
                    </span>
                  </p>
                )}
                {cap.estimated_small_vms_addable != null && (
                  <p>
                    Small VMs addable: <span className="text-[var(--text-primary)]">~{cap.estimated_small_vms_addable}</span>
                  </p>
                )}
                {cap.forecast_30d_vms != null && (
                  <p>
                    30d VM forecast: <span className="text-[var(--text-primary)]">{cap.forecast_30d_vms}</span>
                  </p>
                )}
                <p>
                  Memory total: <span className="text-[var(--text-primary)]">{cap.memory_total_mib} MiB</span>
                </p>
              </div>
              {cap.planner_recommendations && cap.planner_recommendations.length > 0 && (
                <ul className="mt-3 text-xs text-[var(--text-muted)] space-y-1">
                  {cap.planner_recommendations.map((r) => (
                    <li key={r}>• {r}</li>
                  ))}
                </ul>
              )}
            </MacGlassPanel>
          ) : null}
        </>
      )}
      {!loading && tab === 'reports' && compliance && (
        <MacGlassPanel title="Machina Compliance" subtitle={`Grade ${compliance.grade} · ${compliance.score}/100`}>
          <p className="text-2xl font-bold text-[var(--text-primary)] -mt-2">{compliance.score}/100</p>
          <p className="text-sm text-[var(--text-muted)] mt-1">{compliance.summary}</p>
          <ul className="mt-3 text-xs space-y-1">
            {(compliance.checks ?? []).map((c) => (
              <li key={c.id} className={statusToneClass(c.passed ? 'ok' : 'warn')}>
                {c.passed ? '✓' : '○'} {c.name} — {c.detail}
              </li>
            ))}
          </ul>
          <div className="flex flex-wrap gap-2 mt-3">
            <button
              type="button"
              className="btn-secondary text-xs"
              onClick={() => {
                const blob = new Blob([compliance.markdown], { type: 'text/markdown' })
                const url = URL.createObjectURL(blob)
                const a = document.createElement('a')
                a.href = url
                a.download = 'machina-compliance-report.md'
                a.click()
                URL.revokeObjectURL(url)
              }}
            >
              Download Markdown
            </button>
            <button type="button" className="btn-secondary text-xs" onClick={() => void downloadAiComplianceExport().catch((e: unknown) => toast.error(formatUserError(e)))} data-testid="reports-compliance-export-url">
              Export HTML
            </button>
            <button type="button" className="btn-secondary text-xs" onClick={() => void downloadAiCompliancePdf().catch((e: unknown) => toast.error(formatUserError(e)))} data-testid="reports-compliance-pdf-url">
              Export PDF
            </button>
            <button
              type="button"
              className="btn-secondary text-xs"
              onClick={() => void downloadAiComplianceExport().catch((e: unknown) => toast.error(formatUserError(e)))}
            >
              Open print / PDF view
            </button>
            <button
              type="button"
              className="btn-secondary text-xs"
              onClick={() => void downloadAiCompliancePdf().catch((e: unknown) => toast.error(formatUserError(e)))}
            >
              Download PDF
            </button>
          </div>
        </MacGlassPanel>
      )}
      {!loading && tab === 'reports' && budget && (
        <MacGlassPanel title="FinOps budget guard" subtitle={budget.summary}>
          <p className="text-2xl font-bold text-[var(--text-primary)] -mt-2">
            ${budget.current_spend_usd.toFixed(0)}
            <span className="text-sm font-normal text-[var(--text-muted)]"> / ${budget.monthly_budget_usd.toFixed(0)} budget ({budget.utilization_pct.toFixed(0)}%)</span>
          </p>
          <p className="text-sm text-[var(--text-muted)] mt-1">Status: <span className={statusToneClass(budget.status === 'over_budget' ? 'error' : budget.status === 'watch' ? 'warn' : 'ok')}>{budget.status}</span></p>
          {budget.alerts.length > 0 && (
            <ul className="mt-3 text-xs space-y-1">
              {budget.alerts.map((a) => (
                <li key={a.id} className={statusToneClass(a.severity === 'critical' ? 'error' : a.severity === 'warning' ? 'warn' : 'neutral')}>
                  {a.message}
                </li>
              ))}
            </ul>
          )}
        </MacGlassPanel>
      )}
      {!loading && tab === 'reports' && exposureFinops && (
        <MacGlassPanel title="FinOps × Zeus Firewall" subtitle={exposureFinops.summary}>
          <div className="apple-metric-band -mt-2">
            {[
              { label: 'Fleet exposure', value: `$${exposureFinops.fleet_exposure_monthly_usd.toFixed(0)}/mo` },
              { label: 'Idle port waste', value: `$${exposureFinops.idle_port_waste_usd.toFixed(0)}/mo` },
              { label: 'Cloud SG', value: `$${exposureFinops.cloud_sg_monthly_usd.toFixed(0)}/mo` },
              { label: 'GPU / storage', value: `$${(exposureFinops.gpu_exposure_usd + exposureFinops.storage_exposure_usd).toFixed(0)}/mo` },
            ].map((s) => (
              <div key={s.label} className="min-w-0">
                <div className="apple-metric-value">{s.value}</div>
                <div className="apple-metric-label">{s.label}</div>
              </div>
            ))}
          </div>
          {exposureFinops.vm_idle_ranking.length > 0 && (
            <ul className="mt-4 text-xs space-y-1">
              {exposureFinops.vm_idle_ranking.slice(0, 5).map((v) => (
                <li key={v.vm_id} className="text-[var(--text-muted)]">
                  #{v.rank} {v.vm_name} — ${v.waste_usd.toFixed(0)}/mo ({v.idle_ports} idle ports)
                </li>
              ))}
            </ul>
          )}
          <div className="flex flex-wrap gap-2 mt-3">
            <button type="button" className="btn-secondary text-xs" onClick={() => void downloadFirewallExposureFinOpsExport().catch((e: unknown) => toast.error(formatUserError(e)))}>
              Export exposure CSV
            </button>
          </div>
        </MacGlassPanel>
      )}
      {!loading && tab === 'reports' && cost && (
        <MacGlassPanel title="Machina Cost Guardian" subtitle="Idle, oversized, and snapshot-heavy VMs.">
          <p className={`text-2xl font-bold -mt-2 ${statusToneClass('ok')}`}>${cost.estimated_monthly_usd.toFixed(0)}<span className="text-sm font-normal text-[var(--text-muted)]"> est. / month</span></p>
          {cost.predicted_next_month_usd != null && (
            <p className="text-sm text-[var(--text-muted)] mt-1">
              Predicted next month: <span className={`font-medium ${statusToneClass('ok')}`}>${cost.predicted_next_month_usd.toFixed(0)}</span>
            </p>
          )}
          <div className="grid gap-2 sm:grid-cols-3 text-sm text-[var(--text-muted)] mt-3">
            <p>{cost.idle_vm_count} idle VMs</p>
            <p>{cost.oversized_vm_count} oversized</p>
            <p>{cost.snapshot_heavy_count} snapshot-heavy</p>
          </div>
          {cost.suggestions.length > 0 && (
            <ul className="mt-3 text-xs text-[var(--text-muted)] space-y-1">{cost.suggestions.map((s) => <li key={s}>• {s}</li>)}</ul>
          )}
          <div className="flex flex-wrap gap-2 mt-3">
            <button type="button" className="btn-secondary text-xs" onClick={() => void downloadAiCostExport().catch((e: unknown) => toast.error(formatUserError(e)))} data-testid="reports-cost-export-url">
              Export cost CSV
            </button>
            <button
              type="button"
              className="btn-secondary text-xs"
              onClick={() => void downloadAiCostExport().catch((e: unknown) => toast.error(formatUserError(e)))}
            >
              Download CFO CSV
            </button>
          </div>
        </MacGlassPanel>
      )}
      {!loading && tab === 'reports' && attribution && attribution.teams.length > 0 && (
        <MacGlassPanel title="Team cost attribution" subtitle={attribution.summary}>
          <ul className="text-xs space-y-2 text-[var(--text-muted)] mt-2">
            {attribution.teams.slice(0, 8).map((t) => (
              <li key={t.team} className="flex justify-between gap-2">
                <span>{t.team} ({t.vm_count} VMs)</span>
                <span className={statusToneClass('ok')}>${t.estimated_monthly_usd.toFixed(0)}/mo · {t.share_pct.toFixed(0)}%</span>
              </li>
            ))}
          </ul>
          <div className="flex flex-wrap gap-2 mt-3">
            <button type="button" className="btn-secondary text-xs" onClick={() => void downloadCostAttributionExport().catch((e: unknown) => toast.error(formatUserError(e)))} data-testid="reports-attribution-export-url">
              Attribution CSV
            </button>
            <button
              type="button"
              className="btn-secondary text-xs"
              onClick={() => void downloadCostAttributionExport().catch((e: unknown) => toast.error(formatUserError(e)))}
            >
              Download chargeback CSV
            </button>
          </div>
        </MacGlassPanel>
      )}
      {!loading && tab === 'reports' && (
        <MacGlassPanel title="Autopilot" subtitle="POST /api/v1/ai/autopilot/run — safe auto-fix with audit trail">
          <div className="flex flex-wrap items-center gap-2 mb-3">
            <button
              type="button"
              className="btn-secondary text-xs"
              disabled={autopilotBusy}
              onClick={() => {
                setAutopilotBusy(true)
                void runAutopilotSafe(undefined, 3)
                  .then((r) => {
                    toast.success(`Autopilot: ${r.executed_count} executed, ${r.skipped_count} skipped`)
                    return load()
                  })
                  .catch((e: unknown) => toast.error(formatUserError(e)))
                  .finally(() => setAutopilotBusy(false))
              }}
            >
              {autopilotBusy ? 'Running…' : 'Run safe autopilot'}
            </button>
          </div>
          {autopilotHistory.length > 0 ? (
            <ul className="text-xs space-y-2">
              {autopilotHistory.map((h) => (
                <li key={h.id} className="flex justify-between gap-2 border-b border-white/[0.04] pb-2">
                  <span className="text-[var(--text-secondary)]">{h.action.replace('ai.autopilot.', '')}</span>
                  <span className="text-[var(--text-muted)] shrink-0">{new Date(h.created_at).toLocaleString()}</span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-sm text-[var(--text-muted)]">No autopilot runs recorded yet.</p>
          )}
        </MacGlassPanel>
      )}
      {!loading && tab === 'reports' && aiCap && (
        <MacGlassPanel title="Machina Capacity Planner" subtitle="Headroom and simple onboarding projections.">
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4 text-sm -mt-2">
            <p>CPU headroom: <span className="text-[var(--text-primary)]">{aiCap.cpu_headroom_percent}%</span></p>
            <p>Memory headroom: <span className="text-[var(--text-primary)]">{aiCap.memory_headroom_mib} MiB</span></p>
            <p>Small VMs addable: <span className="text-[var(--text-primary)]">~{aiCap.estimated_small_vms_addable}</span></p>
            {aiCap.storage_runway_days != null && <p>Storage runway: <span className="text-[var(--text-primary)]">{aiCap.storage_runway_days} days</span></p>}
          </div>
          {aiCap.recommendations.length > 0 && (
            <ul className="mt-3 text-xs text-[var(--text-muted)] space-y-1">{aiCap.recommendations.map((r) => <li key={r}>• {r}</li>)}</ul>
          )}
          <div className="flex flex-wrap gap-2 mt-3">
            <button type="button" className="btn-secondary text-xs" onClick={() => void downloadAiCapacityExport().catch((e: unknown) => toast.error(formatUserError(e)))} data-testid="reports-capacity-export-url">
              Capacity CSV
            </button>
            <button
              type="button"
              className="btn-secondary text-xs"
              onClick={() => void downloadAiCapacityExport().catch((e: unknown) => toast.error(formatUserError(e)))}
            >
              Download capacity CSV
            </button>
          </div>
        </MacGlassPanel>
      )}
      {!loading && tab === 'reports' && security && security.findings.length > 0 && (
        <MacGlassPanel title="Machina Security Sentinel" subtitle={`Risk level: ${security.risk_level ?? 'unknown'}`}>
          <ul className="text-sm space-y-2 -mt-2">
            {security.findings.slice(0, 8).map((f) => (
              <li key={f.id} className="border-b border-white/[0.04] pb-2">
                <span className={statusToneClass(f.severity === 'critical' ? 'error' : 'warn')}>{f.title}</span>
                <p className="text-xs text-[var(--text-muted)] mt-0.5">{f.detail}</p>
              </li>
            ))}
          </ul>
        </MacGlassPanel>
      )}
      {!loading && tab === 'reports' && finops && (
        <MacGlassPanel title="FinOps estimate" subtitle="Rough monthly cost from vCPU and memory rates.">
          <p className={`text-3xl font-bold -mt-2 ${statusToneClass('ok')}`}>${finops.estimated_monthly_usd.toFixed(2)}<span className="text-sm font-normal text-[var(--text-muted)]"> / month</span></p>
          <div className="grid gap-3 sm:grid-cols-3 text-sm text-[var(--text-muted)] mt-3">
            <p>{finops.total_vcpu} vCPU @ ${finops.vcpu_hour_usd}/hr</p>
            <p>{finops.total_memory_gib.toFixed(1)} GiB @ ${finops.gib_hour_usd}/hr</p>
            <p>{finops.vm_count} VMs ({finops.running_vms} running)</p>
          </div>
        </MacGlassPanel>
      )}
      {!loading && tab === 'reports' && (
      <MacGlassPanel title="Workspaces" subtitle="Project quotas and VM counts.">
        <ul className="text-sm space-y-2 -mt-2">{projects.map((p) => (
          <li key={p.name} className="flex justify-between border-b border-white/[0.04] pb-2"><span className="text-[var(--text-primary)]">{p.name}</span><span className="text-[var(--text-muted)]">{p.vm_count} VMs</span></li>
        ))}</ul>
      </MacGlassPanel>
      )}
    </PlatformPageChrome>
  )
}
