// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { GitCompareArrows, Loader2 } from 'lucide-react'
import { listPlatformVms, type PlatformVm } from '../../../api/platform'
import { driftGuestDisk, guestkitCapabilities, type GuestRepairReport } from '../../../api/guestRepair'
import { formatUserError } from '../../../utils/apiError'

const OFF = (s: string) => /^(shutoff|shut off|stopped|off)$/i.test(s.trim())

/**
 * "How far has this machine moved from my golden image?" Compares this powered-off machine's disk with another
 * powered-off machine's using GuestKit. Read-only: neither disk is written.
 */
export default function GuestDriftCard({ vm }: { vm: PlatformVm }) {
  const [available, setAvailable] = useState(false)
  const [others, setOthers] = useState<PlatformVm[]>([])
  const [baseline, setBaseline] = useState('')
  const [busy, setBusy] = useState(false)
  const [report, setReport] = useState<GuestRepairReport | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let alive = true
    void (async () => {
      const caps = await guestkitCapabilities()
      if (!alive || !caps?.cli_found) return
      const res = await listPlatformVms().catch(() => null)
      const rows = Array.isArray(res) ? res : ((res as { items?: PlatformVm[] } | null)?.items ?? [])
      if (!alive) return
      setOthers(rows.filter((v) => v.id !== vm.id && OFF(v.observed_state) && v.inventory_source !== 'kubevirt'))
      setAvailable(true)
    })()
    return () => { alive = false }
  }, [vm.id])

  // Needs GuestKit, this machine off, and at least one other powered-off machine to compare against.
  if (!available || !OFF(vm.observed_state) || others.length === 0) return null

  const run = async () => {
    setBusy(true)
    setError(null)
    setReport(null)
    try {
      setReport(await driftGuestDisk(vm.name, baseline))
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4" data-testid="guest-drift">
      <p className="flex items-center gap-2 font-medium text-[var(--text-primary)]"><GitCompareArrows className="h-4 w-4 text-[#0071e3]" /> Compare with a golden image</p>
      <p className="mt-1 text-xs text-[var(--text-secondary)]">
        See how far {vm.name} has moved from another machine (packages, config, users). Both disks are only read — nothing is changed.
      </p>
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <select className="input text-sm" aria-label="Machine to compare against" value={baseline} onChange={(e) => setBaseline(e.target.value)}>
          <option value="">Choose a powered-off machine…</option>
          {others.map((o) => <option key={o.id} value={o.name}>{o.name}</option>)}
        </select>
        <button type="button" className="btn-secondary text-sm" disabled={!baseline || busy} onClick={() => void run()}>
          {busy ? <Loader2 className="h-4 w-4 animate-spin" /> : 'Compare'}
        </button>
      </div>
      {report ? <pre className="mt-3 max-h-72 overflow-auto whitespace-pre-wrap rounded-xl bg-[var(--apple-fill-tertiary)]/50 p-3 font-mono text-[11px] text-[var(--text-secondary)]">{report.output || 'No differences found.'}</pre> : null}
      {error ? <p className="mt-3 text-xs text-red-500" role="alert">{error}</p> : null}
    </section>
  )
}
