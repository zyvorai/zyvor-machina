// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { AlertTriangle, CheckCircle2, Loader2, Wrench } from 'lucide-react'
import {
  adoptPlatformVm,
  createVmBackup,
  setVmHa,
  vmPower,
  type ConsoleHubPlan,
  type HealthIssue,
  type PlatformVm,
} from '../../../api/platform'
import { useToastContext } from '../../../contexts/ToastContext'
import { formatUserError } from '../../../utils/apiError'
import GuestAgentSetupDialog from './GuestAgentSetupDialog'

/** What a fix does, in plain words, shown next to the button. */
const FIX_HINTS: Record<string, string> = {
  start_vm: 'Powers the machine on.',
  adopt_vm: 'Brings this machine under Machina management (no change inside the guest).',
  enable_ha: 'Restarts the VM on another host if its host fails.',
  create_backup: 'Queues a full backup now.',
  open_snapshots: 'Opens the Snapshots tab.',
  setup_agent: 'Shows how to install or start the guest agent: SSH, cloud-init or offline with GuestKit.',
}

/** Issues the controller reports without a button are mapped to something the user can act on. */
export function effectiveFix(issue: HealthIssue): { action: string; label: string } | null {
  if (issue.fix_action) {
    return { action: issue.fix_action === 'install_guest_tools' ? 'setup_agent' : issue.fix_action, label: issue.fix_label ?? 'Fix' }
  }
  if (issue.id === 'guest_agent' || issue.id === 'guest_issue') return { action: 'setup_agent', label: 'Set up guest agent' }
  return null
}

/**
 * One-click fixes for a machine's health issues, shown where the user already is (Command Center, VM overview).
 * Actions that change the guest itself open the guided setup instead of running silently.
 */
export default function VmFixItList({
  vm,
  issues,
  plan,
  onDone,
  onOpenTab,
  max = 4,
}: {
  vm: PlatformVm
  issues: HealthIssue[]
  plan?: ConsoleHubPlan | null
  onDone?: () => void
  onOpenTab?: (tab: string) => void
  max?: number
}) {
  const toast = useToastContext()
  const [busy, setBusy] = useState<string | null>(null)
  const [agentOpen, setAgentOpen] = useState(false)

  // The two guest-agent issues describe the same problem; show one.
  const seen = new Set<string>()
  const rows = issues
    .map((issue) => ({ issue, fix: effectiveFix(issue) }))
    .filter(({ issue, fix }) => {
      const key = fix?.action === 'setup_agent' ? 'setup_agent' : issue.id
      if (seen.has(key)) return false
      seen.add(key)
      return true
    })

  const run = async (id: string, action: string) => {
    setBusy(id)
    try {
      switch (action) {
        case 'start_vm': await vmPower(vm.id, 'start'); toast.success('Start queued'); break
        case 'adopt_vm': await adoptPlatformVm(vm.id); toast.success('Machine adopted'); break
        case 'enable_ha': await setVmHa(vm.id, { enabled: true }); toast.success('High availability enabled'); break
        case 'create_backup': await createVmBackup(vm.id); toast.success('Backup queued'); break
        case 'open_snapshots': onOpenTab?.('snapshots'); return
        case 'setup_agent': setAgentOpen(true); return
        default: return
      }
      onDone?.()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(null)
    }
  }

  if (rows.length === 0) {
    return (
      <p className="flex items-center gap-2 rounded-xl border border-[color-mix(in_srgb,#30d158_35%,transparent)] bg-[color-mix(in_srgb,#30d158_9%,transparent)] px-3 py-2 text-xs text-[var(--text-secondary)]" data-testid="vm-fixit-clear">
        <CheckCircle2 className="h-4 w-4 shrink-0 text-emerald-500" aria-hidden /> Nothing to fix — all checks passed.
      </p>
    )
  }

  return (
    <div className="space-y-2" data-testid="vm-fixit">
      <p className="flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--text-muted)]">
        <Wrench className="h-3 w-3" aria-hidden /> Needs a look · {rows.length}
      </p>
      <ul className="space-y-2">
        {rows.slice(0, max).map(({ issue, fix }) => (
          <li key={issue.id} className="rounded-xl border border-[color-mix(in_srgb,#ff9f0a_32%,transparent)] bg-[color-mix(in_srgb,#ff9f0a_8%,transparent)] p-3">
            <p className="flex items-start gap-2 text-sm font-medium text-[var(--text-primary)]">
              <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-500" aria-hidden />
              <span>{issue.message}</span>
            </p>
            {issue.remediation ? <p className="mt-1 pl-5 text-xs text-[var(--text-muted)]">{issue.remediation}</p> : null}
            {fix ? (
              <div className="mt-2 flex flex-wrap items-center gap-2 pl-5">
                <button type="button" className="btn-primary inline-flex items-center gap-1.5 text-xs" disabled={busy !== null} onClick={() => void run(issue.id, fix.action)}>
                  {busy === issue.id ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : null}
                  {fix.label}
                </button>
                {FIX_HINTS[fix.action] ? <span className="text-xs text-[var(--text-muted)]">{FIX_HINTS[fix.action]}</span> : null}
              </div>
            ) : null}
          </li>
        ))}
      </ul>
      <GuestAgentSetupDialog open={agentOpen} onClose={() => setAgentOpen(false)} vm={vm} osHint={plan?.os_hint} sshUser={plan?.ssh_user} />
    </div>
  )
}
