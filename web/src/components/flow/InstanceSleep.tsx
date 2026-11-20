// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Moon, Sun } from 'lucide-react'
import { getSleepPolicy, setSleepPolicy, sleepVm, wakeVm, type SleepPolicy } from '../../api/nativeVms'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

const CHOICES: { value: string; label: string }[] = [
  { value: 'inherit', label: 'Project default' },
  { value: '0', label: 'Never' },
  { value: '15', label: 'After 15 min idle' },
  { value: '30', label: 'After 30 min idle' },
  { value: '60', label: 'After 1 h idle' },
  { value: '240', label: 'After 4 h idle' },
  { value: '1440', label: 'After 1 day idle' },
]

function describe(p: SleepPolicy): string {
  if (p.desired_state === 'sleeping') return 'Sleeping — memory is on disk, the first packet to its address wakes it.'
  if (p.effective_minutes <= 0) return 'Auto-sleep is off.'
  const idle = p.idle_minutes == null ? '' : ` Idle for ${Math.max(0, Math.round(p.idle_minutes))} min.`
  return `Sleeps after ${p.effective_minutes} min without CPU or network activity.${idle}`
}

/** Scale-to-zero controls for one instance: sleep/wake now and the idle policy. */
export default function InstanceSleep({ vmId, onChanged }: { vmId: string; onChanged?: () => void }) {
  const toast = useToastContext()
  const [policy, setPolicy] = useState<SleepPolicy | null>(null)
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    try {
      setPolicy(await getSleepPolicy(vmId))
    } catch {
      setPolicy(null)
    }
  }, [vmId])

  useEffect(() => { void load() }, [load])

  if (!policy) return null
  const sleeping = policy.desired_state === 'sleeping'
  const running = policy.observed_state === 'running'
  const selected = policy.sleep_after_minutes == null ? 'inherit' : String(policy.sleep_after_minutes)
  const options = CHOICES.some((c) => c.value === selected)
    ? CHOICES
    : [...CHOICES, { value: selected, label: `After ${selected} min idle` }]

  const act = async (fn: (id: string) => Promise<unknown>, label: string) => {
    setBusy(true)
    try {
      await fn(vmId)
      toast.success(`${label} queued`)
      onChanged?.()
      void load()
    } catch (e: unknown) {
      toast.error(`${label} failed: ${formatUserError(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const changePolicy = async (value: string) => {
    setBusy(true)
    try {
      setPolicy(await setSleepPolicy(vmId, value === 'inherit' ? null : Number(value)))
      toast.success('Sleep policy saved')
    } catch (e: unknown) {
      toast.error(`Sleep policy: ${formatUserError(e)}`)
    } finally {
      setBusy(false)
    }
  }

  return (
    <section
      className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3"
      aria-label="Scale to zero"
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-sm font-medium text-[var(--text-secondary)] flex items-center gap-2">
            {sleeping ? <Moon className="w-4 h-4" /> : <Sun className="w-4 h-4" />} Scale to zero
          </h2>
          <p className="text-sm text-[var(--text-muted)] mt-1">{describe(policy)}</p>
          {!policy.wakeable && (
            <p className="text-xs text-[var(--text-muted)] mt-1">
              No guest address known yet, so traffic can't wake it; auto-sleep waits for one.
            </p>
          )}
        </div>
        <div className="flex items-center gap-2">
          <label className="text-xs text-[var(--text-muted)]" htmlFor={`sleep-policy-${vmId}`}>Auto-sleep</label>
          <select
            id={`sleep-policy-${vmId}`}
            className="text-sm rounded border border-[var(--apple-hairline)] bg-transparent px-2 py-1"
            value={selected}
            disabled={busy}
            onChange={(e) => void changePolicy(e.target.value)}
          >
            {options.map((c) => (
              <option key={c.value} value={c.value}>
                {c.value === 'inherit' && policy.project_default != null
                  ? `Project default (${policy.project_default ? `${policy.project_default} min` : 'never'})`
                  : c.label}
              </option>
            ))}
          </select>
          {sleeping ? (
            <button type="button" className="btn-primary text-sm inline-flex items-center gap-1" disabled={busy}
              onClick={() => void act(wakeVm, 'Wake')}>
              <Sun className="w-4 h-4" /> Wake
            </button>
          ) : (
            <button type="button" className="btn-secondary text-sm inline-flex items-center gap-1" disabled={busy || !running}
              onClick={() => void act(sleepVm, 'Sleep')}>
              <Moon className="w-4 h-4" /> Sleep now
            </button>
          )}
        </div>
      </div>
      {policy.events.length > 0 && (
        <ul className="text-xs text-[var(--text-muted)] space-y-1" aria-label="Sleep history">
          {policy.events.slice(0, 5).map((e, i) => (
            <li key={`${e.at}-${i}`}>
              <span className="font-mono">{e.at}</span> — {e.kind === 'wake' ? 'woke' : 'slept'} ({e.reason})
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
