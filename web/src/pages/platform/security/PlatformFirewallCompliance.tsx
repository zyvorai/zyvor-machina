// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router'
import { ArrowLeft, CheckCircle2, RefreshCw, Shield } from 'lucide-react'
import { MacGlassPanel } from '../../../components/platform/mac/PlatformMacUi'
import PageLayout from '../../../components/PageLayout'
import {
  getFirewallCompliance,
  exportFirewallSiem,
  getBpfAnomalies,
  listFirewallApprovals,
  approveFirewallChange,
  rejectFirewallChange,
  exportFirewallGitOps,
  syncFirewallGitOps,
  createFirewallTemporaryRule,
  downloadFirewallCompliancePdf,
  type FirewallApproval,
  type FirewallApprovalApplyResult,
} from '../../../api/zeusFirewall'
import JsonInspector, { asRecord } from '../../../components/platform/JsonInspector'
import { ComplianceReportSummary, BpfAnomalySummary } from '../../../components/platform/FirewallComplianceViews'
import { formatUserError } from '../../../utils/apiError'
import { hubLinkClasses, statusBadgeClasses, statusToneClass } from '../../../utils/semanticColors'
import { useToastContext } from '../../../contexts/ToastContext'

const REPORTS = [
  'production',
  'public-exposure',
  'ssh-exposure',
  'database-exposure',
  'drift',
  'temporary-access',
]

export default function PlatformFirewallCompliance() {
  const toast = useToastContext()
  const [report, setReport] = useState<Record<string, unknown> | null>(null)
  const [kind, setKind] = useState('production')
  const [error, setError] = useState<string | null>(null)
  const [approvals, setApprovals] = useState<FirewallApproval[]>([])
  const [approvalsLoading, setApprovalsLoading] = useState(false)
  const [anomalies, setAnomalies] = useState<Record<string, unknown> | null>(null)
  const [anomaliesLoading, setAnomaliesLoading] = useState(false)
  const [tempPort, setTempPort] = useState('22')
  const [tempProtocol, setTempProtocol] = useState('tcp')
  const [tempHours, setTempHours] = useState('4')
  const [tempReason, setTempReason] = useState('Emergency access')

  const load = useCallback(async () => {
    setError(null)
    try {
      const r = await getFirewallCompliance(kind)
      setReport(r)
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }, [kind])

  const loadApprovals = useCallback(async () => {
    setApprovalsLoading(true)
    try {
      const rows = await listFirewallApprovals('pending')
      setApprovals(rows)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setApprovalsLoading(false)
    }
  }, [])

  const loadAnomalies = useCallback(async () => {
    setAnomaliesLoading(true)
    try {
      setAnomalies(await getBpfAnomalies())
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setAnomaliesLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])
  useEffect(() => { void loadApprovals() }, [loadApprovals])
  useEffect(() => { void loadAnomalies() }, [loadAnomalies])

  return (
    <PageLayout
      compact
      error={error}
      prepend={
        <Link to="/platform/zeus/security/firewall" className={`text-sm inline-flex items-center gap-1 min-h-9 ${hubLinkClasses()}`}>
          <ArrowLeft className="w-4 h-4" /> Firewall
        </Link>
      }
      title="Firewall Compliance"
      subtitle="Production exposure, approvals, eBPF anomalies, and GitOps policy sync"
      icon={<CheckCircle2 className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={
        <button type="button" className="btn-secondary text-xs" onClick={() => void load()} aria-label="Refresh">
          <RefreshCw className="w-4 h-4" />
        </button>
      }
      contentClassName="space-y-4"
    >
      <div className="flex flex-wrap gap-3 text-sm">
        <Link to="/platform/placement" className={hubLinkClasses()}>HA & fence events →</Link>
      </div>
      <MacGlassPanel title="Pending approvals" action={
        <button type="button" className={`text-xs ${hubLinkClasses()}`} onClick={() => void loadApprovals()}>
          Refresh
        </button>
      }>
        {approvalsLoading ? (
          <p className="text-sm text-[var(--text-muted)]">Loading…</p>
        ) : approvals.length === 0 ? (
          <p className="text-sm text-[var(--text-muted)]">No pending firewall change approvals.</p>
        ) : (
          <ul className="space-y-3">
            {approvals.map((a) => (
              <li key={a.id} className="flex flex-wrap items-center justify-between gap-2 border border-[var(--apple-hairline)] rounded-lg p-3">
                <div>
                  <p className="text-sm text-[var(--text-primary)]">{a.profile ?? 'custom'} on host {a.target_id.slice(0, 8)}…</p>
                  <p className="text-xs text-[var(--text-muted)]">Requested by {a.requested_by} · {a.created_at}</p>
                </div>
                <div className="flex gap-2">
                  <button
                    type="button"
                    className={`text-xs px-2 py-1 rounded ${statusBadgeClasses('ok')}`}
                    onClick={() => void approveFirewallChange(a.id).then((r: FirewallApprovalApplyResult) => {
                      toast.success(r.message)
                      return loadApprovals()
                    }).catch((e: unknown) => toast.error(formatUserError(e)))}
                  >
                    Approve
                  </button>
                  <button
                    type="button"
                    className="text-xs px-2 py-1 rounded bg-[var(--surface-hover)] text-[var(--text-primary)]"
                    onClick={() => void rejectFirewallChange(a.id).then(() => {
                      toast.success('Firewall change rejected')
                      return loadApprovals()
                    }).catch((e: unknown) => toast.error(formatUserError(e)))}
                  >
                    Reject
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </MacGlassPanel>
      <MacGlassPanel title="eBPF anomalies" action={
        <button type="button" className={`text-xs ${hubLinkClasses()}`} onClick={() => void loadAnomalies()}>
          Refresh
        </button>
      }>
        {anomaliesLoading ? (
          <p className="text-sm text-[var(--text-muted)]">Loading…</p>
        ) : anomalies ? (
          <JsonInspector data={anomalies} emptyMessage="No anomaly data.">
            {asRecord(anomalies) && <BpfAnomalySummary data={asRecord(anomalies)!} />}
          </JsonInspector>
        ) : (
          <p className="text-sm text-[var(--text-muted)]">No anomalies reported — machina-bpfd may not be running on any host.</p>
        )}
      </MacGlassPanel>
      <MacGlassPanel title="Global temporary rule">
        <p className="text-sm text-[var(--text-muted)] mb-3">
          Records a fleet-wide time-boxed allow request for audit purposes only — this does{' '}
          <strong>not</strong> change any host firewall. No rule is actually opened and nothing
          auto-expires; treat this as a paper trail, not a live access grant.
        </p>
        <div className="grid gap-3 md:grid-cols-4 max-w-2xl">
          <input aria-label="Port" className="input text-sm" value={tempPort} onChange={(e) => setTempPort(e.target.value)} placeholder="Port" />
          <select aria-label="Protocol" className="input text-sm" value={tempProtocol} onChange={(e) => setTempProtocol(e.target.value)}>
            <option value="tcp">tcp</option>
            <option value="udp">udp</option>
          </select>
          <input aria-label="Duration hours" className="input text-sm" value={tempHours} onChange={(e) => setTempHours(e.target.value)} placeholder="Hours" />
          <button
            type="button"
            className="btn-primary text-sm"
            onClick={async () => {
              try {
                await createFirewallTemporaryRule({
                  port: Number(tempPort),
                  protocol: tempProtocol,
                  duration_hours: Number(tempHours),
                  reason: tempReason,
                })
                toast.success('Temporary rule recorded (audit only — not applied to any host firewall)')
              } catch (e: unknown) {
                toast.error(formatUserError(e))
              }
            }}
          >
            Create rule
          </button>
        </div>
        <input aria-label="Reason" className="input text-sm mt-3 w-full max-w-2xl" value={tempReason} onChange={(e) => setTempReason(e.target.value)} placeholder="Reason" />
      </MacGlassPanel>
      <div className="flex flex-wrap gap-2">
        {REPORTS.map((r) => (
          <button
            key={r}
            type="button"
            onClick={() => setKind(r)}
            className={`text-xs px-3 py-1.5 rounded-full ${kind === r ? 'bg-[var(--accent)] text-white' : 'bg-white/10 text-[var(--text-secondary)]'}`}
          >
            {r}
          </button>
        ))}
      </div>
      {report && (
        <MacGlassPanel title={`Report: ${kind}`} action={
          <div className="flex gap-3">
            <button type="button" className={`text-xs ${hubLinkClasses()}`} onClick={() => void exportFirewallGitOps().then((r) => {
              const blob = new Blob([JSON.stringify(r, null, 2)], { type: 'application/json' })
              const url = URL.createObjectURL(blob)
              const a = document.createElement('a')
              a.href = url
              a.download = 'machine-firewall-policies.json'
              a.click()
              URL.revokeObjectURL(url)
            })}>
              Export GitOps
            </button>
            <button type="button" className={`text-xs ${statusToneClass('ok')}`} onClick={() => void exportFirewallGitOps().then((r) =>
              syncFirewallGitOps(r.policies ?? [], false).then((s) => toast.success(`Synced ${s.upserted} policies`))
            ).catch((e: unknown) => toast.error(formatUserError(e)))}>
              Sync GitOps
            </button>
            <button
              type="button"
              className={`text-xs ${hubLinkClasses()}`}
              onClick={() => void downloadFirewallCompliancePdf(kind).catch((e: unknown) => toast.error(formatUserError(e)))}
            >
              Export PDF
            </button>
            <button type="button" className={`text-xs ${hubLinkClasses()}`} onClick={() => void exportFirewallSiem(168).then((r) => {
              const blob = new Blob([JSON.stringify(r, null, 2)], { type: 'application/json' })
              const url = URL.createObjectURL(blob)
              const a = document.createElement('a')
              a.href = url
              a.download = 'zeus-firewall-siem-export.json'
              a.click()
              URL.revokeObjectURL(url)
            })}>
              Export SIEM JSON
            </button>
          </div>
        }>
          <JsonInspector data={report}>
            {asRecord(report) && <ComplianceReportSummary report={asRecord(report)!} />}
          </JsonInspector>
        </MacGlassPanel>
      )}
    </PageLayout>
  )
}
