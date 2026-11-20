// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { ShieldCheck } from 'lucide-react'
import { listZyraTrust, setZyraTrust, type ZyraTrustClass } from '../../api/ai'
import { formatUserError } from '../../utils/apiError'

const NAMES: Record<string, { name: string; what: string }> = {
  create_backup: { name: 'Back up a machine', what: 'Takes a backup. Nothing on the machine changes.' },
  enable_ha: { name: 'Turn on high availability', what: 'Restarts a machine on another host if its host fails.' },
  install_guest_tools: { name: 'Install guest tools', what: 'Adds the in-guest agent so health and metrics work.' },
  start_vm: { name: 'Start a stopped machine', what: 'Powers a machine on. It can be switched off again.' },
}

/**
 * Trust ladder. Every action class starts at "ask me". After a run of approvals Zyra offers to do that class by
 * itself — never on its own, and only in autopilot mode, a few per run, all audited and undoable.
 */
export default function ZyraTrustLadder({ canEdit = true }: { canEdit?: boolean }) {
  const [rows, setRows] = useState<ZyraTrustClass[] | null | undefined>(undefined)
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(() => listZyraTrust().then((r) => setRows(Array.isArray(r) ? r : null)).catch(() => setRows(null)), [])
  useEffect(() => { void load() }, [load])

  const change = async (c: ZyraTrustClass, level: 'ask' | 'auto') => {
    setBusy(c.action_type)
    setError(null)
    try {
      const next = await setZyraTrust(c.action_type, level, c.max_per_run)
      if (Array.isArray(next)) setRows(next)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setBusy(null)
    }
  }

  // Older controller (no trust API) or still loading: stay out of the way.
  if (!Array.isArray(rows)) return null

  return (
    <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4" data-testid="zyra-trust-ladder">
      <p className="flex items-center gap-2 text-sm font-semibold text-[var(--text-primary)]"><ShieldCheck className="h-4 w-4 text-[#0071e3]" /> What Zyra may do on its own</p>
      <p className="mt-1 text-xs text-[var(--text-secondary)]">
        Everything starts at “ask me”. Switch a kind of action to automatic and Zyra will approve those proposals itself when autopilot is on
        (a few per run, always audited and undoable). Network policy, temporary access and deletions can never be automatic.
      </p>
      <ul className="mt-3 divide-y divide-[var(--apple-hairline)]">
        {rows.map((c) => {
          const meta = NAMES[c.action_type] ?? { name: c.action_type, what: '' }
          return (
            <li key={c.action_type} className="flex flex-wrap items-center gap-3 py-2.5" data-trust={c.action_type} data-level={c.level}>
              <div className="min-w-0 flex-1">
                <p className="text-sm font-medium text-[var(--text-primary)]">{meta.name}</p>
                <p className="text-xs text-[var(--text-muted)]">
                  {meta.what} {c.total_approved > 0 ? `You approved ${c.approved_streak} in a row (${c.total_approved} total).` : 'No history yet.'}
                </p>
                {c.offer ? <p className="mt-0.5 text-xs font-medium text-[#0071e3]">You’ve approved this {c.approved_streak} times in a row — want Zyra to just do it?</p> : null}
              </div>
              <div role="group" aria-label={`${meta.name} mode`} className="inline-flex overflow-hidden rounded-full border border-[var(--apple-hairline)] text-xs">
                {(['ask', 'auto'] as const).map((lv) => (
                  <button
                    key={lv}
                    type="button"
                    disabled={!canEdit || busy === c.action_type}
                    aria-pressed={c.level === lv}
                    onClick={() => c.level !== lv && void change(c, lv)}
                    className={`min-h-9 px-3 ${c.level === lv ? 'bg-[#0071e3] text-white' : 'text-[var(--text-secondary)] hover:bg-[var(--apple-fill-tertiary)]'}`}
                  >
                    {lv === 'ask' ? 'Ask me' : 'Automatic'}
                  </button>
                ))}
              </div>
            </li>
          )
        })}
      </ul>
      {error ? <p className="mt-2 text-xs text-red-500" role="alert">{error}</p> : null}
    </section>
  )
}
