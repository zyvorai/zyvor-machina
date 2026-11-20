// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useState } from 'react'
import { Stethoscope } from 'lucide-react'
import { troubleshootVm, type DiagnosisReport } from '../../api/ai'
import { formatUserError } from '../../utils/apiError'
import { statusBadgeClasses, statusToneClass, riskTone } from '../../utils/semanticColors'

export default function MachinaVmTroubleshootPanel({
  vmId,
  vmName,
  symptom = 'slow',
}: {
  vmId: string
  vmName?: string
  symptom?: string
}) {
  const [report, setReport] = useState<DiagnosisReport | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [activeSymptom, setActiveSymptom] = useState(symptom)

  const run = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setReport(await troubleshootVm({ vm_id: vmId, vm_name: vmName, symptom: activeSymptom }))
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [vmId, vmName, activeSymptom])

  return (
    <div className="platform-mac-stat rounded-2xl border border-white/[0.06] bg-[var(--apple-surface)] p-5 space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="font-semibold flex items-center gap-2">
          <Stethoscope className="w-4 h-4 text-orange-400" />
          Autonomous troubleshooter
        </h3>
        <div className="flex gap-2">
          <select aria-label="Troubleshoot symptom" className="input text-xs" value={activeSymptom} onChange={(e) => setActiveSymptom(e.target.value)}>
            <option value="slow">VM is slow</option>
            <option value="unreachable">Unreachable</option>
            <option value="disk">Disk issues</option>
            <option value="network">Network</option>
          </select>
          <button type="button" className="btn-secondary text-xs" disabled={loading} onClick={() => void run()}>
            {loading ? 'Running…' : report ? 'Re-run' : 'Run'}
          </button>
        </div>
      </div>
      {error && <p className={`text-sm ${statusToneClass('error')}`}>{error}</p>}
      {report && (
        <>
          <p className="text-sm text-[var(--text-secondary)]">
            {report.vm_name} · severity{' '}
            <span className={`text-[10px] uppercase px-2 py-0.5 rounded-full ${statusBadgeClasses(riskTone(report.severity))}`}>
              {report.severity}
            </span>
          </p>
          <div className="grid gap-2 sm:grid-cols-2 text-xs">
            {report.checks.map((c) => (
              <div key={c.domain} className="rounded-lg border border-white/[0.06] p-2">
                <p className="font-medium text-[var(--text-primary)] capitalize">{c.domain}</p>
                <p className={`${statusToneClass(c.status === 'ok' ? 'ok' : c.status === 'warn' ? 'warn' : 'error')}`}>{c.status}</p>
                <p className="text-[var(--text-muted)] mt-0.5">{c.detail}</p>
              </div>
            ))}
          </div>
          {report.findings.length > 0 && (
            <ul className="text-sm space-y-1">
              {report.findings.map((f) => (
                <li key={`${f.domain}-${f.message}`} className="text-[var(--text-muted)]">
                  [{f.domain}] {f.message}
                </li>
              ))}
            </ul>
          )}
          {report.recommended_actions.length > 0 && (
            <ul className="text-xs text-[var(--text-muted)] space-y-0.5">
              {report.recommended_actions.map((a) => (
                <li key={a}>→ {a}</li>
              ))}
            </ul>
          )}
        </>
      )}
    </div>
  )
}
