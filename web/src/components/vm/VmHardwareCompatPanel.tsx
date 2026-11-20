// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { CheckCircle2, CircleAlert, Loader2 } from 'lucide-react'
import type { VmDomainCapabilitiesReport, VmHardwareCompatReport } from '../../api/platform'
import HardwareApplyBadge from './HardwareApplyBadge'
import { resolveHardwareBadges } from '../../utils/hardwareApplyBadges'

type Props = {
  loading?: boolean
  report: VmHardwareCompatReport | null
  domainCaps?: VmDomainCapabilitiesReport | null
  error?: string | null
}

export default function VmHardwareCompatPanel({ loading, report, domainCaps, error }: Props) {
  if (loading) {
    return (
      <div className="rounded-lg border border-white/[0.08] bg-[var(--apple-surface)] p-3 text-xs text-[var(--text-muted)] flex items-center gap-2" data-testid="vm-hardware-compat-panel">
        <Loader2 className="w-3.5 h-3.5 animate-spin" /> Checking compatibility…
      </div>
    )
  }
  if (error) {
    return (
      <div className="rounded-lg border border-rose-500/30 bg-rose-950/20 p-3 text-xs text-rose-200" data-testid="vm-hardware-compat-panel">
        {error}
      </div>
    )
  }
  if (!report) return null

  const caps = domainCaps
  const modes = report.cpu_modes_supported ?? caps?.cpu_modes_supported ?? []

  return (
    <div className="rounded-lg border border-white/[0.08] bg-[var(--apple-surface)] p-3 space-y-2" data-testid="vm-hardware-compat-panel">
      <div className="flex items-center gap-2">
        {report.ok ? (
          <CheckCircle2 className="w-4 h-4 text-emerald-400" />
        ) : (
          <CircleAlert className="w-4 h-4 text-amber-400" />
        )}
        <p className="text-xs font-medium text-[var(--text-primary)]">
          {report.ok ? 'Compatible with this host' : 'Compatibility issues found'}
        </p>
      </div>
      {report.issues.length === 0 ? (
        <p className="text-xs text-[var(--text-muted)]">No issues detected against domain capabilities.</p>
      ) : (
        <ul className="space-y-2">
          {report.issues.map((issue, idx) => (
            <li key={`${issue.category}-${idx}`} className="text-xs text-[var(--text-secondary)]">
              <p>{issue.message}</p>
              <div className="flex flex-wrap gap-1 mt-1">
                {resolveHardwareBadges(issue.badges).map((b) => (
                  <HardwareApplyBadge key={b.id} badge={b} compact />
                ))}
              </div>
            </li>
          ))}
        </ul>
      )}
      {caps ? (
        <div className="rounded border border-white/[0.06] bg-[var(--apple-surface)] p-2 space-y-1" data-testid="vm-domain-caps-summary">
          <p className="text-[11px] text-[var(--text-muted)]">
            Host domain capabilities · {caps.arch} / {caps.virttype}
          </p>
          <p className="text-[11px] text-[var(--text-muted)]">
            TPM: {caps.tpm_supported ? 'supported' : 'not advertised'} · UEFI: {caps.uefi_supported ? 'supported' : 'not advertised'}
          </p>
          {caps.machine_types.length > 0 ? (
            <p className="text-[11px] text-[var(--text-muted)]">Machines: {caps.machine_types.slice(0, 4).join(', ')}{caps.machine_types.length > 4 ? '…' : ''}</p>
          ) : null}
        </div>
      ) : null}
      {modes.length > 0 ? (
        <p className="text-[11px] text-[var(--text-muted)] pt-1">Host CPU modes: {modes.join(', ')}</p>
      ) : null}
    </div>
  )
}
