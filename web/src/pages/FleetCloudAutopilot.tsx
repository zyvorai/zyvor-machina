// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import * as autopilot from '../api/autopilot'
import { executeZyraAction } from '../api/ai'
import PageLayout from '../components/PageLayout'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'

const primary = 'btn btn-primary min-h-11'
const secondary = 'btn btn-secondary min-h-11'

function usd(n: number) {
  const sign = n < 0 ? '−' : '+'
  return `${sign}$${Math.abs(n).toFixed(2)}`
}

function gib(mib: number) {
  return mib >= 1024 ? `${(mib / 1024).toFixed(mib % 1024 ? 1 : 0)} GiB` : `${mib} MiB`
}

export default function FleetCloudAutopilot() {
  const toast = useToastContext()
  const [sizing, setSizing] = useState<autopilot.Rightsizing | null>(null)
  const [plan, setPlan] = useState<autopilot.ConsolidationPlan | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)

  const load = useCallback(async () => {
    const [s, p] = await Promise.all([autopilot.getRightsizing(), autopilot.getConsolidation()])
    setSizing(s); setPlan(p); setError(null)
  }, [])

  useEffect(() => { load().catch((e) => setError(formatUserError(e))) }, [load])

  async function run(key: string, work: () => Promise<string>) {
    setBusy(key)
    try {
      toast.success(await work())
      await load()
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(null)
    }
  }

  const applyResize = (r: autopilot.Recommendation) => run(r.vm_id, async () => {
    const id = r.pending_action ?? (await autopilot.proposeResize(r.vm_id)).id
    await executeZyraAction(id)
    return `Resizing ${r.name}. It is verified when the change lands and can be undone from Actions.`
  })
  const fileResize = (r: autopilot.Recommendation) => run(r.vm_id, async () => {
    await autopilot.proposeResize(r.vm_id)
    return `Resize of ${r.name} sent for approval.`
  })
  const consolidate = (apply: boolean) => run('consolidate', async () => {
    const a = await autopilot.proposeConsolidation()
    if (!apply) return 'Consolidation sent for approval.'
    await executeZyraAction(a.id)
    return 'Consolidation started. Each live migration is prechecked first.'
  })

  const recs = sizing?.recommendations ?? []
  return (
    <PageLayout title="Autopilot" subtitle="Right-size instances from their real history and pack hosts so idle ones can power down." prepend={<FleetCloudSubNav />} error={error}>
      <div className="space-y-6">
        <section className="tahoe-glass-card space-y-3 p-5" aria-label="Rightsizing">
          <div className="flex flex-wrap items-baseline justify-between gap-3">
            <h2 className="text-lg font-semibold">Rightsizing</h2>
            {sizing && recs.length > 0 && <span className="text-sm text-[var(--text-secondary)]">{usd(sizing.monthly_delta_usd)} per month if all are applied</span>}
          </div>
          <p className="text-[var(--text-secondary)]">
            Sized from the 95th percentile of hourly peaks over the last 14 days, aiming at 60% CPU and 30% memory headroom.
            Every change goes through an approval, is checked after it lands, and can be undone.
          </p>
          {!sizing && !error && <p role="status">Loading…</p>}
          {sizing && recs.length === 0 && <p>Nothing to change. Instances need at least {sizing.min_hours} hours of history before they get a suggestion.</p>}
          <ul className="divide-y divide-[var(--apple-hairline)]">
            {recs.map((r) => (
              <li key={r.vm_id} className="flex flex-wrap items-center gap-3 py-3">
                <div className="min-w-0 flex-1">
                  <p className="font-medium">{r.name}{r.project ? <span className="text-[var(--text-secondary)]"> · {r.project}</span> : null}</p>
                  <p className="text-sm">{r.vcpus} → {r.suggested_vcpus} vCPUs · {gib(r.memory_mib)} → {gib(r.suggested_memory_mib)} · {usd(r.monthly_delta_usd)}/month</p>
                  <p className="text-xs text-[var(--text-secondary)]">{r.reasons.join('; ')} · {r.hours} hours of history</p>
                </div>
                {r.pending_action && <span className="text-sm text-[var(--text-secondary)]">Waiting for approval</span>}
                {!r.pending_action && <button className={secondary} disabled={busy !== null} onClick={() => void fileResize(r)}>Request approval</button>}
                <button className={primary} disabled={busy !== null} onClick={() => void applyResize(r)} aria-label={`Apply resize of ${r.name}`}>{busy === r.vm_id ? 'Applying…' : 'Apply'}</button>
              </li>
            ))}
          </ul>
        </section>

        <section className="tahoe-glass-card space-y-3 p-5" aria-label="Consolidation">
          <h2 className="text-lg font-semibold">Host consolidation</h2>
          <p className="text-[var(--text-secondary)]">Hosts under 30% memory use are emptied onto the others, filling each to at most 80%. VMs tied to their host stay put, and the last host is always kept.</p>
          {!plan && !error && <p role="status">Loading…</p>}
          {plan && plan.moves.length === 0 && <p>Hosts are already packed well. Nothing to move.</p>}
          {plan && plan.moves.length > 0 && (
            <>
              <p>{plan.moves.length} live {plan.moves.length === 1 ? 'migration empties' : 'migrations empty'} {plan.emptied.map((h) => h.name).join(', ')}, which can then be powered down.</p>
              <ul className="text-sm">
                {plan.moves.map((m) => <li key={m.vm_id}>{m.vm} → {m.to_name}</li>)}
              </ul>
              <div className="flex flex-wrap gap-3">
                <button className={secondary} disabled={busy !== null} onClick={() => void consolidate(false)}>Request approval</button>
                <button className={primary} disabled={busy !== null} onClick={() => void consolidate(true)}>{busy === 'consolidate' ? 'Starting…' : 'Consolidate now'}</button>
              </div>
            </>
          )}
          {plan && plan.kept.length > 0 && (
            <ul className="text-xs text-[var(--text-secondary)]" aria-label="Hosts kept">
              {plan.kept.map(([host, why]) => <li key={host}>{host}: {why}</li>)}
            </ul>
          )}
        </section>
      </div>
      <FleetCloudFooter />
    </PageLayout>
  )
}
