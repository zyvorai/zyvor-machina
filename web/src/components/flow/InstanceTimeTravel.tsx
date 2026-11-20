// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import { GitBranch, History, Rewind, Unlink } from 'lucide-react'
import {
  createRestorePoint,
  detachFork,
  forkVm,
  getTimeTravel,
  rewindVm,
  setRestorePointPolicy,
  type RestorePoint,
  type TimeTravel,
} from '../../api/nativeVms'
import ConfirmDialog from '../ConfirmDialog'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

const SCHEDULES: { value: number; label: string }[] = [
  { value: 0, label: 'Off' },
  { value: 15, label: 'Every 15 min' },
  { value: 30, label: 'Every 30 min' },
  { value: 60, label: 'Every hour' },
  { value: 360, label: 'Every 6 h' },
  { value: 1440, label: 'Every day' },
]
const KEEPS = [6, 12, 24, 48, 168]

function when(p: RestorePoint): string {
  const d = new Date(p.created_at)
  return Number.isNaN(d.getTime()) ? p.created_at : d.toLocaleString()
}

/**
 * Restore points on a timeline: rewind the instance to any point, or fork a
 * new instance from a point (or from right now, optionally with its memory).
 */
export default function InstanceTimeTravel({
  vmId,
  vmName,
  running,
  onChanged,
}: {
  vmId: string
  vmName: string
  running: boolean
  onChanged?: () => void
}) {
  const toast = useToastContext()
  const [tt, setTt] = useState<TimeTravel | null>(null)
  const [pos, setPos] = useState(0)
  const [busy, setBusy] = useState(false)
  const [confirmRewind, setConfirmRewind] = useState(false)
  const [forkOpen, setForkOpen] = useState(false)
  const [forkName, setForkName] = useState('')
  const [memory, setMemory] = useState(false)
  const [isolate, setIsolate] = useState(false)
  const [reseed, setReseed] = useState(true)
  const selectedRef = useRef<string | null>(null)
  selectedRef.current = tt?.points[pos]?.id ?? null

  const load = useCallback(async () => {
    try {
      const v = await getTimeTravel(vmId)
      const selected = selectedRef.current
      const keep = selected ? v.points.findIndex((p) => p.id === selected) : -1
      setTt(v)
      setPos(keep >= 0 ? keep : v.points.length)
    } catch {
      setTt(null)
    }
  }, [vmId])

  useEffect(() => { void load() }, [load])

  if (!tt) return null
  const atNow = pos >= tt.points.length
  const point = atNow ? null : tt.points[pos]
  const schedule = tt.every_minutes ?? 0
  const scheduleOptions = SCHEDULES.some((s) => s.value === schedule)
    ? SCHEDULES
    : [...SCHEDULES, { value: schedule, label: `Every ${schedule} min` }]

  const run = async (fn: () => Promise<unknown>, label: string) => {
    setBusy(true)
    try {
      await fn()
      toast.success(`${label} queued`)
      onChanged?.()
      window.setTimeout(() => void load(), 1500)
    } catch (e: unknown) {
      toast.error(`${label} failed: ${formatUserError(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const savePolicy = async (every: number, keep?: number) => {
    setBusy(true)
    try {
      const v = await setRestorePointPolicy(vmId, every, keep)
      setTt(v)
      toast.success(every ? 'Restore point schedule saved' : 'Scheduled restore points off')
    } catch (e: unknown) {
      toast.error(`Restore points: ${formatUserError(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const submitFork = () => {
    const name = forkName.trim()
    if (!name) return
    void run(
      () =>
        forkVm(vmId, {
          name,
          restore_point_id: point?.id,
          memory: atNow && memory,
          isolate: isolate || (atNow && memory),
          reseed,
        }),
      `Fork ${name}`,
    ).then(() => {
      setForkOpen(false)
      setForkName('')
      setMemory(false)
    })
  }

  return (
    <section
      className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3"
      aria-label="Time travel"
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-sm font-medium text-[var(--text-secondary)] flex items-center gap-2">
            <History className="w-4 h-4" /> Time travel
          </h2>
          <p className="text-sm text-[var(--text-muted)] mt-1">
            {tt.points.length === 0
              ? 'No restore points yet. Take one before a risky change, or schedule them.'
              : `${tt.points.length} restore point${tt.points.length === 1 ? '' : 's'}; rewinding or forking copies no data.`}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <label className="text-xs text-[var(--text-muted)]" htmlFor={`rp-schedule-${vmId}`}>Restore points</label>
          <select
            id={`rp-schedule-${vmId}`}
            className="text-sm rounded border border-[var(--apple-hairline)] bg-transparent px-2 py-1"
            value={schedule}
            disabled={busy}
            onChange={(e) => void savePolicy(Number(e.target.value))}
          >
            {scheduleOptions.map((s) => <option key={s.value} value={s.value}>{s.label}</option>)}
          </select>
          {schedule > 0 && (
            <>
              <label className="text-xs text-[var(--text-muted)]" htmlFor={`rp-keep-${vmId}`}>Keep</label>
              <select
                id={`rp-keep-${vmId}`}
                className="text-sm rounded border border-[var(--apple-hairline)] bg-transparent px-2 py-1"
                value={tt.keep}
                disabled={busy}
                onChange={(e) => void savePolicy(schedule, Number(e.target.value))}
              >
                {(KEEPS.includes(tt.keep) ? KEEPS : [...KEEPS, tt.keep].sort((a, b) => a - b)).map((k) => (
                  <option key={k} value={k}>{k}</option>
                ))}
              </select>
            </>
          )}
          <button
            type="button"
            className="btn-secondary text-sm"
            disabled={busy}
            aria-label="Create restore point"
            onClick={() => void run(() => createRestorePoint(vmId), 'Restore point')}
          >
            Restore point now
          </button>
        </div>
      </div>

      <div className="space-y-2">
        <input
          type="range"
          className="w-full"
          min={0}
          max={tt.points.length}
          step={1}
          value={pos}
          aria-label="Point in time"
          aria-valuetext={point ? when(point) : 'Now'}
          onChange={(e) => setPos(Number(e.target.value))}
        />
        <div className="flex justify-between text-[11px] text-[var(--text-muted)]">
          <span>{tt.points[0] ? when(tt.points[0]) : ''}</span>
          <span>Now</span>
        </div>
        <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
          <div aria-live="polite">
            {point ? (
              <>
                <span className="font-medium">{when(point)}</span>
                <span className="text-[var(--text-muted)]">
                  {' '}— {point.kind}{point.note ? `: ${point.note}` : ''}{point.quiesced ? ' · filesystems quiesced' : ''}
                  {point.forks.length > 0 ? ` · forks: ${point.forks.join(', ')}` : ''}
                </span>
              </>
            ) : (
              <span className="font-medium">Now</span>
            )}
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              className="btn-secondary text-sm inline-flex items-center gap-1"
              disabled={busy || atNow}
              onClick={() => setConfirmRewind(true)}
            >
              <Rewind className="w-4 h-4" /> Rewind here
            </button>
            <button
              type="button"
              className="btn-primary text-sm inline-flex items-center gap-1"
              disabled={busy}
              onClick={() => {
                setForkOpen((o) => !o)
                if (!forkName) setForkName(`${vmName}-fork`)
              }}
            >
              <GitBranch className="w-4 h-4" /> Fork {atNow ? 'now' : 'here'}
            </button>
          </div>
        </div>
      </div>

      {forkOpen && (
        <form
          className="flex flex-wrap items-end gap-3 rounded-xl border border-[var(--apple-hairline)] p-3"
          aria-label="Fork instance"
          onSubmit={(e) => {
            e.preventDefault()
            submitFork()
          }}
        >
          <label className="text-xs text-[var(--text-muted)] flex flex-col gap-1">
            Fork name
            <input
              className="text-sm rounded border border-[var(--apple-hairline)] bg-transparent px-2 py-1"
              value={forkName}
              onChange={(e) => setForkName(e.target.value)}
              aria-label="Fork name"
            />
          </label>
          {atNow && running && (
            <label className="text-xs flex items-center gap-1">
              <input type="checkbox" checked={memory} onChange={(e) => setMemory(e.target.checked)} />
              Copy memory (isolated network, same addresses)
            </label>
          )}
          {!(atNow && memory) && (
            <>
              <label className="text-xs flex items-center gap-1">
                <input type="checkbox" checked={isolate} onChange={(e) => setIsolate(e.target.checked)} />
                Isolated network
              </label>
              <label className="text-xs flex items-center gap-1">
                <input type="checkbox" checked={reseed} onChange={(e) => setReseed(e.target.checked)} />
                New cloud-init identity
              </label>
            </>
          )}
          <button type="submit" className="btn-primary text-sm" disabled={busy || !forkName.trim()}>
            Fork
          </button>
        </form>
      )}

      {tt.fork_of && (
        <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
          <span>
            Forked from{' '}
            <Link className="underline" to={`/fleet-cloud/instances/${encodeURIComponent(tt.fork_of.vm_id)}`}>
              {tt.fork_of.name ?? tt.fork_of.vm_id}
            </Link>
            {tt.fork_of.memory ? ' with its memory' : ''}{tt.fork_of.isolated ? ' · isolated network' : ''}
          </span>
          <button
            type="button"
            className="btn-secondary text-sm inline-flex items-center gap-1"
            disabled={busy}
            onClick={() => void run(() => detachFork(vmId), 'Detach')}
          >
            <Unlink className="w-4 h-4" /> Detach from source
          </button>
        </div>
      )}
      {tt.forks.length > 0 && (
        <ul className="text-xs text-[var(--text-muted)] space-y-1" aria-label="Forks">
          {tt.forks.map((f) => (
            <li key={f.vm_id}>
              <Link className="underline" to={`/fleet-cloud/instances/${encodeURIComponent(f.vm_id)}`}>
                {f.name ?? f.vm_id}
              </Link>
              {f.memory ? ' — memory fork' : ''}{f.isolated ? ' · isolated' : ''}
            </li>
          ))}
          <li>While forks depend on this instance its restore points are not merged.</li>
        </ul>
      )}

      <ConfirmDialog
        open={confirmRewind}
        variant="danger"
        title="Rewind instance"
        message={point
          ? `Rewind '${vmName}' to ${when(point)}? Everything written after that is discarded${running ? ' and the instance restarts' : ''}.`
          : ''}
        confirmLabel="Rewind"
        onCancel={() => setConfirmRewind(false)}
        onConfirm={() => {
          setConfirmRewind(false)
          if (point) void run(() => rewindVm(vmId, point.id), 'Rewind')
        }}
      />
    </section>
  )
}
