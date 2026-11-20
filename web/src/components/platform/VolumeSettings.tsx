// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import {
  setVolumeDeleteOnTermination,
  setVolumeIoLimits,
  type NativeVolume,
  type VolumeIoLimits,
} from '../../api/nativeVolumes'
import { formatUserError } from '../../utils/apiError'

const FIELDS: { key: keyof VolumeIoLimits; label: string }[] = [
  { key: 'read_iops', label: 'Read IOPS' },
  { key: 'write_iops', label: 'Write IOPS' },
  { key: 'read_bps', label: 'Read bytes/s' },
  { key: 'write_bps', label: 'Write bytes/s' },
]

/** EC2 block-device settings: delete with the instance, and per-volume I/O limits (0 = unlimited). */
export default function VolumeSettings({ volume, onChanged }: { volume: NativeVolume; onChanged?: () => void }) {
  const [dot, setDot] = useState(!!volume.delete_on_termination)
  const [vals, setVals] = useState<Record<string, string>>(
    Object.fromEntries(FIELDS.map((f) => [f.key, volume[f.key] != null ? String(volume[f.key]) : ''])),
  )
  const [msg, setMsg] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const apply = async () => {
    const limits: VolumeIoLimits = {}
    for (const f of FIELDS) {
      const raw = vals[f.key]?.trim()
      if (!raw) continue
      const n = Number(raw)
      if (!Number.isInteger(n) || n < 0) {
        setError(`${f.label} must be a whole number, 0 or more`)
        return
      }
      limits[f.key] = n
    }
    if (Object.keys(limits).length === 0) {
      setError('Enter at least one limit')
      return
    }
    setBusy(true)
    setError(null)
    try {
      await setVolumeIoLimits(volume.id, limits)
      setMsg(volume.attached_vm_id ? 'Limits applied to the running disk' : 'Limits saved')
      onChanged?.()
    } catch (e) {
      setError(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3" aria-label="Volume settings">
      <h2 className="text-sm font-medium text-[var(--text-secondary)]">Settings</h2>
      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={dot}
          onChange={async (e) => {
            const v = e.target.checked
            setDot(v)
            try {
              await setVolumeDeleteOnTermination(volume.id, v)
              setError(null)
            } catch (err) {
              setDot(!v)
              setError(formatUserError(err))
            }
          }}
        />
        Delete this volume when its instance is deleted
      </label>
      <div className="grid sm:grid-cols-4 gap-2">
        {FIELDS.map((f) => (
          <label key={f.key} className="text-xs text-[var(--text-muted)] flex flex-col gap-1">
            {f.label}
            <input
              inputMode="numeric"
              className="input-field text-sm"
              placeholder="unchanged"
              value={vals[f.key]}
              onChange={(e) => setVals({ ...vals, [f.key]: e.target.value })}
            />
          </label>
        ))}
      </div>
      <div className="flex items-center gap-3">
        <button type="button" disabled={busy} className="btn-secondary text-sm" onClick={() => void apply()}>
          Apply limits
        </button>
        {msg && <span className="text-xs text-[var(--text-muted)]">{msg}</span>}
        {error && <span role="alert" className="text-xs text-red-500">{error}</span>}
      </div>
    </section>
  )
}
