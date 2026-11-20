// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { VmDoctorReport } from '../../api/ai'
import {
  adoptPlatformVm,
  createVmBackup,
  installGuestTools,
  setVmHa,
  vmPower,
} from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { statusToneClass } from '../../utils/semanticColors'

interface MachinaDoctorPanelProps {
  vmId: string
  report: VmDoctorReport | null
  loading: boolean
  onRefresh: () => void
  onTab?: (tab: string) => void
}

export default function MachinaDoctorPanel({ vmId, report, loading, onRefresh, onTab }: MachinaDoctorPanelProps) {
  const toast = useToastContext()

  const fix = async (issue: VmDoctorReport['issues'][0]) => {
    try {
      switch (issue.fix_action) {
        case 'start_vm':
          await vmPower(vmId, 'start')
          toast.success('Start queued')
          break
        case 'adopt_vm':
          await adoptPlatformVm(vmId)
          toast.success('VM adopted')
          break
        case 'enable_ha':
          await setVmHa(vmId, { enabled: true })
          toast.success('HA enabled')
          break
        case 'create_backup':
          await createVmBackup(vmId)
          toast.success('Backup queued')
          break
        case 'open_snapshots':
          onTab?.('snapshots')
          break
        case 'install_guest_tools':
          await installGuestTools(vmId)
          toast.success('Guest tools install queued')
          break
        default:
          return
      }
      onRefresh()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const pct = report?.score_numeric ?? 0

  return (
    <div className="platform-mac-stat rounded-2xl border border-white/[0.06] bg-[var(--apple-surface)] p-5 space-y-4">
      <div className="flex items-center justify-between gap-2">
        <h3 className="font-semibold">Zyra SRE</h3>
        <button type="button" className="btn-secondary text-xs" disabled={loading} onClick={onRefresh}>
          {loading ? 'Scanning…' : 'Run scan'}
        </button>
      </div>
      {report && (
        <>
          <div className="flex items-center gap-4">
            <div className="relative w-16 h-16 shrink-0">
              <svg className="w-16 h-16 -rotate-90" viewBox="0 0 36 36">
                <circle cx="18" cy="18" r="15" fill="none" stroke="currentColor" strokeWidth="3" className="text-[var(--mark-track)]" />
                <circle cx="18" cy="18" r="15" fill="none" stroke="currentColor" strokeWidth="3" strokeDasharray={`${pct} 100`} className={statusToneClass(pct >= 80 ? 'ok' : pct >= 50 ? 'warn' : 'error')} />
              </svg>
              <span className="absolute inset-0 flex items-center justify-center text-sm font-bold">{pct}</span>
            </div>
            <div>
              <p className="text-lg font-semibold text-[var(--text-primary)]">{report.score_numeric}/100</p>
              <p className={`text-sm capitalize ${statusToneClass(report.healthy ? 'ok' : 'warn')}`}>{report.score_label}</p>
              <p className="text-xs text-[var(--text-muted)]">{report.checks_passed}/{report.checks_total} checks passed</p>
            </div>
          </div>
          {report.issues.length === 0 ? (
            <p className="text-sm text-[var(--text-muted)]">All checks passed.</p>
          ) : (
            <ul className="space-y-3 text-sm">
              {report.issues.map((issue) => (
                <li key={issue.message} className="rounded-xl border border-white/[0.06] p-3">
                  <p className={`font-medium capitalize ${statusToneClass(issue.severity === 'critical' ? 'error' : 'warn')}`}>{issue.message}</p>
                  {issue.remediation && <p className="text-xs text-[var(--text-muted)] mt-1">{issue.remediation}</p>}
                  {issue.fix_action && issue.fix_label && (
                    <button type="button" className="btn-primary text-xs mt-2" onClick={() => void fix(issue)}>{issue.fix_label}</button>
                  )}
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </div>
  )
}
