// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Sparkles } from 'lucide-react'
import { Link } from 'react-router'
import { explainInfraObject } from '../../api/ai'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusToneClass } from '../../utils/semanticColors'

export default function MachinaExplainObjectPanel({
  kind,
  id,
  name,
  onClose,
  showOpenLink = true,
}: {
  kind: string
  id: string
  name?: string
  onClose?: () => void
  showOpenLink?: boolean
}) {
  const [data, setData] = useState<Awaited<ReturnType<typeof explainInfraObject>> | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const reqRef = useRef(0)

  const load = useCallback(async () => {
    const reqId = ++reqRef.current
    setLoading(true)
    setError(null)
    try {
      const result = await explainInfraObject(kind, id)
      if (reqRef.current !== reqId) return // a newer kind/id was selected while this was in flight
      setData(result)
    } catch (e: unknown) {
      if (reqRef.current !== reqId) return
      setError(formatUserError(e))
    } finally {
      if (reqRef.current === reqId) setLoading(false)
    }
  }, [kind, id])

  useEffect(() => { void load() }, [load])

  return (
    <div className="rounded-xl border border-orange-500/20 bg-orange-500/5 p-4 space-y-2 text-sm">
      <div className="flex items-center justify-between gap-2">
        <p className="font-medium text-orange-700 flex items-center gap-2">
          <Sparkles className="w-4 h-4" />
          {name ?? data?.name ?? id}
          <span className="text-xs text-[var(--text-muted)] font-normal">{kind}</span>
        </p>
        {onClose && (
          <button type="button" className="text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)]" onClick={onClose}>Close</button>
        )}
      </div>
      {loading && <p className="text-xs text-[var(--text-muted)]">Loading object explain…</p>}
      {error && <p className={`text-xs ${statusToneClass('error')}`}>{error}</p>}
      {data && (
        <>
          <p className="text-[var(--text-secondary)]">{data.purpose}</p>
          {data.health_score != null && (
            <p className="text-xs text-[var(--text-muted)]">Health {data.health_score}/100</p>
          )}
          {data.risks.length > 0 && (
            <ul className="text-xs text-amber-700/90 space-y-0.5">
              {data.risks.map((r) => <li key={r}>⚠ {r}</li>)}
            </ul>
          )}
          {showOpenLink && kind === 'vm' && (
            <Link to={`/platform/vms/${id}`} className={`text-xs ${hubLinkClasses()}`}>Open VM →</Link>
          )}
          {showOpenLink && kind === 'host' && (
            <Link to={`/platform/hosts/${id}`} className={`text-xs ${hubLinkClasses()}`}>Open host →</Link>
          )}
        </>
      )}
    </div>
  )
}
