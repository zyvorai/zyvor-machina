// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { KeyRound } from 'lucide-react'
import PlatformEmptyState from '../PlatformEmptyState'
import CopyButton from '../../CopyButton'
import type { EnrollmentTokenRow } from '../../../api/platform'
import { statusActionLinkClasses, statusPillClasses } from '../../../utils/semanticColors'
import { tokenState, type TokenState } from './enrollCommand'

const TONE: Record<TokenState, 'ok' | 'neutral' | 'warn'> = { open: 'ok', used: 'neutral', expired: 'warn' }
const LABEL: Record<TokenState, string> = { open: 'Open', used: 'Used', expired: 'Expired' }

export default function EnrollTokensTable({
  rows,
  loading,
  onRevoke,
  now = Date.now(),
}: {
  rows: EnrollmentTokenRow[]
  loading?: boolean
  onRevoke: (token: string) => void
  now?: number
}) {
  if (rows.length === 0 && !loading) {
    return (
      <PlatformEmptyState
        icon={KeyRound}
        title="No enrollment tokens yet"
        subtitle="Each token works for one machine, once. Used and expired tokens stay listed here; revoking an open token deletes it."
      />
    )
  }
  return (
    <div className="overflow-x-auto" data-testid="enroll-tokens-table">
      <table className="apple-table w-full text-sm">
        <thead>
          <tr className="text-left text-[var(--text-muted)]">
            <th className="py-2 pr-3 font-medium">Token</th>
            <th className="py-2 pr-3 font-medium">Created</th>
            <th className="py-2 pr-3 font-medium">Expires</th>
            <th className="py-2 pr-3 font-medium">Status</th>
            <th className="py-2 text-right font-medium">Actions</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((t) => {
            const state = tokenState(t, now)
            return (
              <tr key={t.token} data-token-state={state} className="border-t border-[var(--apple-hairline)]">
                <td className="py-2 pr-3"><code className="text-xs">{t.token.slice(0, 14)}…</code></td>
                <td className="py-2 pr-3 text-[var(--text-secondary)] tabular-nums">{t.created_at}</td>
                <td className="py-2 pr-3 text-[var(--text-secondary)] tabular-nums">{t.expires_at ?? '—'}</td>
                <td className="py-2 pr-3"><span className={`text-xs ${statusPillClasses(TONE[state])}`}>{LABEL[state]}</span></td>
                <td className="py-2 text-right">
                  <span className="inline-flex items-center gap-3">
                    {state === 'open' && <CopyButton text={t.token} label="Copy token" />}
                    {state === 'open' && (
                      <button type="button" className={statusActionLinkClasses('error')} onClick={() => onRevoke(t.token)}>Revoke</button>
                    )}
                  </span>
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}
