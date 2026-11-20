// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, type ReactElement } from 'react'
import { ArrowUpCircle, X } from 'lucide-react'
import { getUpdateCheck, type UpdateCheck } from '../../api/platform'

const dismissKey = (v: string) => `machina-update-dismissed-${v}`

/**
 * "A new version is available". The controller only checks when an operator sets MACHINA_UPDATE_CHECK_URL, so on an
 * air-gapped site (or any failure) this renders nothing at all.
 */
export default function UpdateBanner(): ReactElement | null {
  const [info, setInfo] = useState<UpdateCheck | null>(null)
  const [hidden, setHidden] = useState(false)

  useEffect(() => {
    let alive = true
    getUpdateCheck()
      .then((u) => {
        if (!alive || !u?.enabled || !u.available || !u.latest) return
        let dismissed = false
        try { dismissed = localStorage.getItem(dismissKey(u.latest)) === '1' } catch { /* private mode */ }
        if (!dismissed) setInfo(u)
      })
      .catch(() => { /* no banner when the answer is not available */ })
    return () => { alive = false }
  }, [])

  if (!info || hidden) return null
  return (
    <div role="status" data-testid="update-banner" className="flex flex-wrap items-center gap-3 rounded-xl border border-[var(--apple-separator,rgba(0,0,0,0.1))] px-4 py-2.5 text-sm">
      <ArrowUpCircle className="h-4 w-4 shrink-0 text-[#0071e3]" aria-hidden />
      <span className="min-w-0 flex-1">
        Machina {info.latest} is available (this controller runs {info.current}).{' '}
        {info.notes_url && /^https?:\/\//.test(info.notes_url) && (
          <a href={info.notes_url} target="_blank" rel="noreferrer" className="text-[#0071e3] underline">Release notes</a>
        )}
        {' '}Upgrade with <code>sudo machinactl upgrade</code> after <code>machinactl backup all</code>.
      </span>
      <button
        type="button"
        aria-label="Dismiss the update notice"
        className="rounded-full p-1 text-[var(--text-muted)] hover:bg-[var(--apple-fill-tertiary)]"
        onClick={() => {
          try { localStorage.setItem(dismissKey(info.latest ?? ''), '1') } catch { /* private mode */ }
          setHidden(true)
        }}
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  )
}
