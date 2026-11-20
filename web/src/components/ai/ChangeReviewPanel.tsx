// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { ChevronDown, ChevronRight, ShieldQuestion } from 'lucide-react'
import { getZyraChangeReview, type ZyraChangeReview } from '../../api/ai'
import { formatUserError } from '../../utils/apiError'

const VERDICT: Record<ZyraChangeReview['verdict'], { label: string; cls: string }> = {
  approve: { label: 'Safe to approve', cls: 'text-emerald-500' },
  approve_with_care: { label: 'Approve with care', cls: 'text-amber-500' },
  hold: { label: 'Hold for review', cls: 'text-red-500' },
}

/** "What happens if I approve?" — loaded on demand so the queue stays fast. */
export default function ChangeReviewPanel({ actionId }: { actionId: string }) {
  const [open, setOpen] = useState(false)
  const [review, setReview] = useState<ZyraChangeReview | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  const toggle = async () => {
    const next = !open
    setOpen(next)
    if (next && !review && !loading) {
      setLoading(true)
      setError(null)
      try {
        setReview(await getZyraChangeReview(actionId))
      } catch (e: unknown) {
        setError(formatUserError(e))
      } finally {
        setLoading(false)
      }
    }
  }

  const v = review ? VERDICT[review.verdict] : null
  return (
    <div className="mt-1 ml-1 text-xs">
      <button
        type="button"
        onClick={() => void toggle()}
        aria-expanded={open}
        className="inline-flex items-center gap-1 text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
      >
        {open ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
        <ShieldQuestion className="w-3 h-3" />
        What happens if I approve?
      </button>
      {open && (
        <div className="mt-2 rounded-lg border border-[var(--border-subtle,rgba(128,128,128,.25))] p-3 space-y-2" data-testid="change-review">
          {loading && <p className="text-[var(--text-muted)]">Reviewing…</p>}
          {error && <p className="text-red-500">{error}</p>}
          {review && v && (
            <>
              <p>
                <span className={`font-semibold ${v.cls}`}>{v.label}</span>
                <span className="text-[var(--text-muted)]"> · risk {review.risk} · {review.reversible ? 'reversible' : 'not reversible'}</span>
              </p>
              <p className="text-[var(--text-secondary)]">{review.summary}</p>
              {review.blast_radius.length > 0 && (
                <ul className="list-disc ml-4 text-[var(--text-muted)]">
                  {review.blast_radius.map((b) => <li key={b}>{b}</li>)}
                </ul>
              )}
              <p><span className="font-medium">Rollback:</span> <span className="text-[var(--text-muted)]">{review.rollback}</span></p>
              {review.checks.length > 0 && (
                <ul className="list-disc ml-4 text-[var(--text-muted)]">
                  {review.checks.map((c) => <li key={c}>{c}</li>)}
                </ul>
              )}
            </>
          )}
        </div>
      )}
    </div>
  )
}
