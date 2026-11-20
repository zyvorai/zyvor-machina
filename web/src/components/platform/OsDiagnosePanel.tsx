// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { Sparkles } from 'lucide-react'
import AskZyraButton from '../ai/AskZyraButton'
import { MacGlassPanel } from './mac/PlatformMacUi'
import {
  enqueueValidateHost,
  syncHost,
  installGuestTools,
  runVmHealthCheck,
  type HostOsDiagnoseReport,
  type VmOsDiagnoseReport,
  type OsDiagnoseAction,
} from '../../api/platform'
import { troubleshootVm, type DiagnosisReport } from '../../api/ai'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses } from '../../utils/semanticColors'

type OsDiagnoseReport = HostOsDiagnoseReport | VmOsDiagnoseReport

interface OsDiagnosePanelProps {
  resourceId: string
  resourceKind: 'host' | 'vm'
  defaultQuery?: string
  loading: boolean
  report: OsDiagnoseReport | null
  onRun: (query?: string) => void
  onAskCopilot?: () => void
}

export default function OsDiagnosePanel({
  resourceId,
  resourceKind,
  defaultQuery,
  loading,
  report,
  onRun,
  onAskCopilot,
}: OsDiagnosePanelProps) {
  const toast = useToastContext()
  const navigate = useNavigate()
  const [query, setQuery] = useState(defaultQuery ?? '')
  const [troubleshoot, setTroubleshoot] = useState<DiagnosisReport | null>(null)
  const [troubleshootLoading, setTroubleshootLoading] = useState(false)

  const runFullTroubleshoot = async () => {
    if (resourceKind !== 'vm') return
    setTroubleshootLoading(true)
    try {
      setTroubleshoot(await troubleshootVm({
        vm_id: resourceId,
        symptom: query.toLowerCase().includes('disk') ? 'disk'
          : query.toLowerCase().includes('network') ? 'network'
          : query.toLowerCase().includes('unreachable') ? 'unreachable'
          : 'slow',
      }))
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setTroubleshootLoading(false)
    }
  }

  const runFix = async (fix: OsDiagnoseAction) => {
    try {
      switch (fix.action) {
        case 'host.sync':
          await syncHost(resourceId)
          toast.success('Host sync queued')
          break
        case 'host.validate':
          await enqueueValidateHost(resourceId)
          toast.success('Validation queued')
          break
        case 'firewall.open':
          navigate(`/platform/zeus/security/firewall/${resourceId}`)
          break
        case 'nav.storage':
          navigate('/platform/storage')
          break
        case 'vm.guest_tools':
          await installGuestTools(resourceId)
          toast.success('Guest tools install queued')
          break
        case 'vm.health_check':
          await runVmHealthCheck(resourceId)
          toast.success('Health check queued')
          break
        default:
          if (fix.detail.startsWith('/')) navigate(fix.detail)
          return
      }
      onRun(query || undefined)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  return (
    <MacGlassPanel
      title="Machina OS Diagnose"
      subtitle={resourceKind === 'host' ? 'Hypervisor Linux + fleet correlation' : 'Guest health + ports context'}
      action={
        onAskCopilot ? (
          <AskZyraButton variant="secondary" className="text-xs" onClick={onAskCopilot} />
        ) : undefined
      }
    >
      <div className="flex flex-wrap gap-2 mb-3">
        <input
          aria-label="Diagnosis query"
          className="input text-sm flex-1 min-w-[12rem]"
          placeholder={resourceKind === 'host' ? 'Why is this host under pressure?' : 'Guest health and exposed ports'}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <button type="button" className="btn-primary text-sm flex items-center gap-1" disabled={loading} onClick={() => onRun(query || undefined)}>
          <Sparkles className="w-4 h-4" />
          {loading ? 'Diagnosing…' : 'Diagnose'}
        </button>
        {resourceKind === 'vm' && (
          <button type="button" className="btn-secondary text-sm" disabled={troubleshootLoading} onClick={() => void runFullTroubleshoot()}>
            {troubleshootLoading ? 'Running…' : 'Full troubleshoot'}
          </button>
        )}
      </div>
      {report && (
        <div className="space-y-3 text-sm">
          <p className="text-[var(--text-secondary)]">{report.summary}</p>
          {report.hypotheses.map((h) => (
            <div key={h.title} className="rounded-xl border border-white/[0.06] bg-[var(--apple-surface)] p-3">
              <div className="flex justify-between gap-2 mb-1">
                <span className="font-medium text-[var(--text-primary)]">{h.title}</span>
                <span className="text-xs text-[var(--link)]">{Math.round(h.confidence * 100)}%</span>
              </div>
              <p className="text-xs text-[var(--text-muted)]">{h.evidence}</p>
              <p className="text-xs text-[var(--text-muted)] mt-1">{h.action}</p>
            </div>
          ))}
          {report.fix_actions.length > 0 && (
            <div className="flex flex-wrap gap-2 pt-1">
              {report.fix_actions.map((fix) => (
                <button key={fix.action} type="button" className="btn-secondary text-xs" onClick={() => void runFix(fix)}>
                  {fix.label}
                </button>
              ))}
            </div>
          )}
          {resourceKind === 'host' && (
            <Link to={`/platform/hosts/${resourceId}`} className={`text-xs ${hubLinkClasses()}`}>Host settings →</Link>
          )}
        </div>
      )}
      {troubleshoot && (
        <div className="mt-4 rounded-xl border border-orange-500/20 bg-orange-500/5 p-3 text-xs space-y-2">
          <p className="font-medium text-orange-700">Full troubleshoot — {troubleshoot.severity}</p>
          <div className="grid gap-2 sm:grid-cols-2">
            {troubleshoot.checks.map((c) => (
              <div key={c.domain} className="text-[var(--text-muted)]">
                <span className="text-[var(--text-secondary)] capitalize">{c.domain}</span>: {c.detail}
              </div>
            ))}
          </div>
          {troubleshoot.findings.slice(0, 3).map((f) => (
            <p key={f.message} className="text-[var(--text-muted)]">[{f.domain}] {f.message}</p>
          ))}
        </div>
      )}
    </MacGlassPanel>
  )
}
