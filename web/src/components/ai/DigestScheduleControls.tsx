// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { getDigestSchedule, sendDigestNow, setDigestSchedule, type DigestSchedule } from '../../api/ai'
import { formatUserError } from '../../utils/apiError'

// .input is width: 100% and outranks Tailwind's layered utilities, so w-auto does not apply.
const AUTO_WIDTH = { width: 'auto' } as const

const DAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday']

/**
 * Send the fleet digest on a schedule. Off by default. It goes to the webhooks and notification channels that
 * subscribe to `zyra.digest` (or `zyra.*` / `*`). Changing it and sending now need the admin role.
 */
export default function DigestScheduleControls() {
  const [saved, setSaved] = useState<DigestSchedule | null>(null)
  const [enabled, setEnabled] = useState(false)
  const [frequency, setFrequency] = useState<'daily' | 'weekly'>('daily')
  const [hour, setHour] = useState(8)
  const [weekday, setWeekday] = useState(1)
  const [busy, setBusy] = useState(false)
  const [note, setNote] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const apply = (s: DigestSchedule) => {
    setSaved(s)
    setEnabled(s.enabled)
    setFrequency(s.frequency)
    setHour(s.hour_utc)
    setWeekday(s.weekday)
  }

  useEffect(() => {
    let live = true
    getDigestSchedule()
      .then((s) => { if (live) apply(s) })
      .catch((e: unknown) => { if (live) setError(formatUserError(e)) })
    return () => { live = false }
  }, [])

  const run = async (label: string, fn: () => Promise<unknown>) => {
    setBusy(true)
    setError(null)
    setNote(null)
    try {
      await fn()
      setNote(label)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const save = () =>
    run('Schedule saved', async () => apply(await setDigestSchedule({ enabled, frequency, hour_utc: hour, weekday })))

  return (
    <div className="mt-4 pt-3 border-t border-white/10 text-xs" data-testid="digest-schedule">
      <label className="flex items-center gap-2 font-medium text-[var(--text-primary)]">
        <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
        Send this digest automatically
      </label>
      <div className="flex flex-wrap items-center gap-2 mt-2">
        <select aria-label="Digest frequency" className="input text-xs" style={AUTO_WIDTH} value={frequency}
          onChange={(e) => setFrequency(e.target.value as 'daily' | 'weekly')}>
          <option value="daily">Every day</option>
          <option value="weekly">Every week</option>
        </select>
        {frequency === 'weekly' && (
          <select aria-label="Digest weekday" className="input text-xs" style={AUTO_WIDTH} value={weekday}
            onChange={(e) => setWeekday(Number(e.target.value))}>
            {DAYS.map((d, i) => <option key={d} value={i + 1}>{d}</option>)}
          </select>
        )}
        <select aria-label="Digest hour (UTC)" className="input text-xs" style={AUTO_WIDTH} value={hour}
          onChange={(e) => setHour(Number(e.target.value))}>
          {Array.from({ length: 24 }, (_, h) => (
            <option key={h} value={h}>{String(h).padStart(2, '0')}:00 UTC</option>
          ))}
        </select>
        <button type="button" className="btn-primary text-xs" disabled={busy || !saved} onClick={() => void save()}>
          Save
        </button>
        <button type="button" className="btn-secondary text-xs" disabled={busy || !saved}
          onClick={() => void run('Digest sent to the subscribed channels', sendDigestNow)}>
          Send now
        </button>
      </div>
      <p className="mt-2 text-[var(--text-muted)]">
        Goes to notification channels and webhooks subscribed to <code>zyra.digest</code>, <code>zyra.*</code> or <code>*</code>.
        A new channel only receives <code>alert.*</code> until you add one of those.
        {saved?.last_sent_at ? ` Last sent ${saved.last_sent_at} UTC.` : ' Not sent yet.'}
      </p>
      {note && <p className="mt-1 text-emerald-600">{note}</p>}
      {error && <p className="mt-1 text-red-500">{error}</p>}
    </div>
  )
}
