// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Loader2, Snowflake } from 'lucide-react'
import { getGuestFsFreezeStatus } from '../../api/platform'
import { statusSurfaceClasses, statusToneClass } from '../../utils/semanticColors'

type Props = {
  vmId: string
  /** Poll guest-agent fs-freeze while quiesced snapshots are in progress. */
  poll?: boolean
  className?: string
}

export default function GuestFsFreezeBanner({ vmId, poll = false, className = '' }: Props) {
  const [frozen, setFrozen] = useState<boolean | null>(null)
  const [detail, setDetail] = useState('')
  const [loading, setLoading] = useState(false)

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      const r = await getGuestFsFreezeStatus(vmId)
      const fz = r.fs_freeze
      setFrozen(Boolean(fz?.frozen))
      setDetail(fz?.detail ?? r.message ?? '')
    } catch {
      setFrozen(null)
      setDetail('')
    } finally {
      setLoading(false)
    }
  }, [vmId])

  useEffect(() => {
    void refresh()
  }, [refresh])

  useEffect(() => {
    if (!poll) return
    const id = window.setInterval(() => void refresh(), 5000)
    return () => window.clearInterval(id)
  }, [poll, refresh])

  if (frozen === null && !loading && !detail) return null

  return (
    <div
      className={`rounded-lg border px-3 py-2 text-xs flex items-start gap-2 ${
        frozen ? statusSurfaceClasses('warn') : statusSurfaceClasses('neutral')
      } ${className}`}
    >
      {loading ? (
        <Loader2 className="w-3.5 h-3.5 animate-spin shrink-0 mt-0.5" aria-hidden="true" />
      ) : (
        <Snowflake className={`w-3.5 h-3.5 shrink-0 mt-0.5 ${frozen ? statusToneClass('warn') : 'text-[var(--text-muted)]'}`} />
      )}
      <div className="min-w-0">
        <p className="font-medium text-[var(--text-primary)]">
          {frozen ? 'Guest filesystems frozen (quiesce)' : 'Guest filesystems not frozen'}
        </p>
        {detail && <p className="text-[var(--text-muted)] mt-0.5">{detail}</p>}
        {poll && (
          <p className="text-[var(--text-faint)] mt-1">Polling every 5s while quiesce snapshot is enabled.</p>
        )}
      </div>
    </div>
  )
}
