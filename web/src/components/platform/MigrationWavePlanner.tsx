// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Link } from 'react-router'
import { Loader2, Route } from 'lucide-react'
import { guestkitVmMigratePlan } from '../../api/guestkit'
import { listPlatformVms } from '../../api/platform'
import { blockers, planWaves, type ScoredMachine } from '../../utils/migrationWaves'

const CONCURRENCY = 3
const MAX_MACHINES = 30

/**
 * Migration copilot: score every machine for a move to KVM with GuestKit, group them into cutover waves, and say
 * what blocks each one. It plans only — nothing is changed. Fixes happen where they belong (Boot Doctor for boot
 * problems) and each wave is scheduled by a person.
 */
export default function MigrationWavePlanner() {
  const [busy, setBusy] = useState(false)
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null)
  const [rows, setRows] = useState<ScoredMachine[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  const run = async () => {
    setBusy(true)
    setError(null)
    setRows(null)
    try {
      const res = await listPlatformVms()
      const all = (Array.isArray(res) ? res : ((res as { items?: { id: string; name: string; inventory_source?: string }[] })?.items ?? []))
        .filter((v) => v.inventory_source !== 'kubevirt')
        .slice(0, MAX_MACHINES)
      setProgress({ done: 0, total: all.length })
      const out: ScoredMachine[] = []
      let next = 0
      const worker = async () => {
        while (next < all.length) {
          const v = all[next++]
          try {
            out.push({ id: v.id, name: v.name, plan: await guestkitVmMigratePlan(v.id) })
          } catch (e) {
            out.push({ id: v.id, name: v.name, plan: null, error: e instanceof Error ? e.message : 'could not be scored' })
          }
          setProgress((p) => (p ? { ...p, done: p.done + 1 } : p))
        }
      }
      await Promise.all(Array.from({ length: CONCURRENCY }, worker))
      setRows(out)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not list machines')
    } finally {
      setBusy(false)
    }
  }

  const result = rows ? planWaves(rows) : null

  return (
    <section className="space-y-4" data-testid="migration-wave-planner">
      <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4">
        <p className="flex items-center gap-2 font-medium text-[var(--text-primary)]"><Route className="h-4 w-4 text-[#0071e3]" /> Plan migration waves</p>
        <p className="mt-1 text-xs text-[var(--text-secondary)]">
          GuestKit scores each machine’s disk for a move to KVM, then Machina groups them into waves — easy wins first, blocked machines last.
          This only plans; nothing is changed.
        </p>
        <button type="button" className="btn-primary mt-3 text-sm" disabled={busy} onClick={() => void run()}>
          {busy ? <span className="inline-flex items-center gap-2"><Loader2 className="h-4 w-4 animate-spin" /> Scoring {progress ? `${progress.done}/${progress.total}` : '…'}</span> : 'Score my machines'}
        </button>
        {error ? <p className="mt-2 text-xs text-red-500" role="alert">{error}</p> : null}
      </div>

      {result?.waves.map((w) => (
        <div key={w.id} className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4" data-wave={w.id}>
          <div className="flex flex-wrap items-baseline justify-between gap-2">
            <p className="font-medium text-[var(--text-primary)]">{w.title} <span className="text-xs font-normal text-[var(--text-muted)]">· {w.machines.length} machine{w.machines.length === 1 ? '' : 's'}</span></p>
            {w.machines.length > 0 ? <p className="text-xs text-[var(--text-muted)]">about {w.downtimeMinutes} min of downtime in total</p> : null}
          </div>
          <p className="text-xs text-[var(--text-secondary)]">{w.blurb}</p>
          {w.machines.length === 0 ? <p className="mt-2 text-xs text-[var(--text-muted)]">Nothing here.</p> : (
            <ul className="mt-2 divide-y divide-[var(--apple-hairline)]">
              {w.machines.map((m) => {
                const p = m.plan!
                const notes = [...blockers(p), ...p.required_changes]
                return (
                  <li key={m.id} className="py-2.5 text-sm" data-machine={m.name}>
                    <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                      <Link to={`/platform/vms/${m.id}`} className="font-medium text-[#0071e3] hover:underline">{m.name}</Link>
                      <span className="rounded-full bg-[var(--apple-fill-tertiary)] px-2 py-0.5 text-xs tabular-nums">{Math.round(p.migration_score)}% ready</span>
                      <span className="text-xs text-[var(--text-muted)]">~{p.estimated_downtime_minutes} min downtime</span>
                      {p.boot_score < 60 ? <Link to={`/platform/vms/${m.id}?action=bootdoctor`} className="text-xs text-[#0071e3] hover:underline">Open Boot Doctor</Link> : null}
                    </div>
                    {notes.length > 0 ? <ul className="mt-1 list-disc pl-5 text-xs text-[var(--text-secondary)]">{notes.map((n) => <li key={n}>{n}</li>)}</ul> : null}
                  </li>
                )
              })}
            </ul>
          )}
        </div>
      ))}

      {result && result.unscored.length > 0 ? (
        <p className="text-xs text-[var(--text-muted)]">
          {result.unscored.length} machine{result.unscored.length === 1 ? ' was' : 's were'} not scored (usually because the disk is on another host): {result.unscored.map((u) => u.name).join(', ')}.
        </p>
      ) : null}
    </section>
  )
}
