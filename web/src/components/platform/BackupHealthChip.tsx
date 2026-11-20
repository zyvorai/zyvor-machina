// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { BackupRecord } from '../../api/platform'

/** How a backup was taken, from the agent's completion message ("… (application-consistent, verified)"). */
export function backupConsistency(message?: string | null): string | null {
  const m = /\((application-consistent|crash-consistent|offline)/.exec(message ?? '')
  return m ? m[1] : null
}

const ago = (iso: string): string => {
  const t = new Date(iso.replace(' ', 'T') + (iso.endsWith('Z') ? '' : 'Z')).getTime()
  const h = Math.max(0, Math.round((Date.now() - t) / 3_600_000))
  return h < 1 ? 'just now' : h < 48 ? `${h}h ago` : `${Math.round(h / 24)}d ago`
}

/** Whether a stored backup has been read back and found intact, and how consistent it is. */
export default function BackupHealthChip({ backup }: { backup: BackupRecord }) {
  const consistency = backupConsistency(backup.message)
  const status = backup.verify_status ?? ''
  return (
    <span className="inline-flex flex-wrap items-center gap-1.5" data-testid="backup-health" data-verify={status || 'never'}>
      {status === 'ok' ? (
        <span className="text-emerald-600" title={backup.verify_message ?? undefined}>✓ Verified{backup.verified_at ? ` ${ago(backup.verified_at)}` : ''}</span>
      ) : status === 'failed' ? (
        <span className="text-red-500" title={backup.verify_message ?? undefined}>✕ Failed its check{backup.verify_message ? ` — ${backup.verify_message}` : ''}</span>
      ) : (
        <span className="text-[var(--text-muted)]">Not verified yet</span>
      )}
      {consistency ? <span className="rounded-full bg-[var(--apple-fill-tertiary)] px-1.5 py-0.5 text-[10px] text-[var(--text-secondary)]">{consistency}</span> : null}
    </span>
  )
}
