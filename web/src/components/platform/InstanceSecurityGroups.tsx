// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { ShieldCheck } from 'lucide-react'
import {
  attachInstanceSecurityGroup,
  detachInstanceSecurityGroup,
  listInstanceSecurityGroups,
  listSecurityGroups,
  type NativeSecurityGroup,
} from '../../api/securityGroups'
import { formatUserError } from '../../utils/apiError'

/** The security groups on one instance, with what the datapath says about each (advisory / enforced / auditing …). */
export default function InstanceSecurityGroups({ vmId }: { vmId: string }) {
  const [attached, setAttached] = useState<NativeSecurityGroup[] | null>(null)
  const [all, setAll] = useState<NativeSecurityGroup[]>([])
  const [pick, setPick] = useState('')
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      const [a, g] = await Promise.all([listInstanceSecurityGroups(vmId), listSecurityGroups()])
      setAttached(a)
      setAll(g)
      setError(null)
    } catch (e) {
      setError(formatUserError(e))
    }
  }, [vmId])

  useEffect(() => {
    void load()
  }, [load])

  const act = async (fn: () => Promise<void>) => {
    try {
      await fn()
      await load()
    } catch (e) {
      setError(formatUserError(e))
    }
  }

  const free = all.filter((g) => !attached?.some((a) => a.id === g.id))

  return (
    <section className="tahoe-glass-card p-4 space-y-3" aria-label="Security groups">
      <h2 className="text-sm font-semibold inline-flex items-center gap-2">
        <ShieldCheck className="w-4 h-4" /> Security groups
      </h2>
      {error && <p role="alert" className="text-xs text-red-500">{error}</p>}
      {attached && attached.length === 0 && (
        <p className="text-xs text-[var(--text-muted)]">None attached: no group filters this instance.</p>
      )}
      <ul className="space-y-1">
        {attached?.map((g) => (
          <li key={g.id} className="flex flex-wrap items-center gap-2 text-sm">
            <span className="font-medium">{g.name}</span>
            <span className="rounded-full border border-[var(--apple-hairline)] px-2 py-0.5 text-xs">
              {g.enforcement?.state ?? 'advisory'}
            </span>
            <span className="text-xs text-[var(--text-muted)]">{g.enforcement?.reason}</span>
            <button
              type="button"
              className="ml-auto px-2 py-1 text-xs rounded border border-[var(--apple-hairline)]"
              onClick={() => void act(() => detachInstanceSecurityGroup(vmId, g.id))}
            >
              Detach
            </button>
          </li>
        ))}
      </ul>
      <div className="flex gap-2 items-center">
        <select
          aria-label="Security group to attach"
          className="input-field text-sm"
          value={pick}
          onChange={(e) => setPick(e.target.value)}
        >
          <option value="">Attach a group…</option>
          {free.map((g) => (
            <option key={g.id} value={g.id}>{g.name}</option>
          ))}
        </select>
        <button
          type="button"
          disabled={!pick}
          className="px-3 py-1.5 text-sm rounded border border-[var(--apple-hairline)] disabled:opacity-50"
          onClick={() => void act(async () => { await attachInstanceSecurityGroup(vmId, pick); setPick('') })}
        >
          Attach
        </button>
      </div>
    </section>
  )
}
