// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Stethoscope } from 'lucide-react'
import { troubleshootVm, type DiagnosisReport } from '../../api/ai'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusToneClass } from '../../utils/semanticColors'

export default function VmOverviewTroubleshootPanel({
  vmId,
  vmName,
  onOpenDoctor,
}: {
  vmId: string
  vmName: string
  onOpenDoctor?: () => void
}) {
  const [report, setReport] = useState<DiagnosisReport | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const run = async () => {
    setLoading(true)
    setError(null)
    try {
      setReport(await troubleshootVm({ vm_id: vmId, vm_name: vmName, symptom: 'unreachable' }))
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }

  return (
    <MacGlassPanel
      title="AI troubleshoot"
      subtitle="Autonomous diagnosis when the VM is not running normally"
      action={
        <button type="button" className="btn-secondary text-xs" disabled={loading} onClick={() => void run()}>
          {loading ? 'Analyzing…' : report ? 'Re-run' : 'Run troubleshoot'}
        </button>
      }
    >
      <p className="text-sm text-[var(--text-muted)] flex items-center gap-2">
        <Stethoscope className="w-4 h-4 text-orange-400" />
        One-click RCA for stopped, missing, or unhealthy guests.
      </p>
      {error && <p className={`text-sm mt-2 ${statusToneClass('error')}`}>{error}</p>}
      {report && (
        <div className="mt-3 space-y-2 text-sm">
          <p className="text-[var(--text-secondary)]">
            Severity: <span className={statusToneClass(report.severity === 'critical' ? 'error' : 'warn')}>{report.severity}</span>
          </p>
          <ul className="text-xs text-[var(--text-muted)] space-y-1">
            {report.findings.slice(0, 3).map((f) => (
              <li key={`${f.domain}-${f.message}`}>[{f.domain}] {f.message}</li>
            ))}
          </ul>
          {report.recommended_actions.slice(0, 2).map((a) => (
            <p key={a} className={`text-xs ${hubLinkClasses()}`}>→ {a}</p>
          ))}
        </div>
      )}
      {onOpenDoctor && (
        <button type="button" className={`text-xs mt-3 ${hubLinkClasses()}`} onClick={onOpenDoctor}>
          Full doctor & troubleshooter →
        </button>
      )}
    </MacGlassPanel>
  )
}
