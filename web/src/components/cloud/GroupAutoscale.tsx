// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import * as cloud from '../../api/cloud'
import type { NativeLoadBalancer } from '../../api/nativeLoadBalancers'
import { formatUserError } from '../../utils/apiError'

const field = 'form-selector p-2 min-h-11 w-full sm:w-28 text-[var(--text-primary)]'
const wide = 'form-selector p-2 min-h-11 w-full sm:w-auto text-[var(--text-primary)]'

function hourLabel(h: number) {
  return new Date(h * 1000).toLocaleString(undefined, { weekday: 'short', hour: '2-digit', minute: '2-digit' })
}

/** Past 48 hours of group CPU demand, then the seasonal forecast for the next 24, as bars. */
export function ForecastChart({ data }: { data: cloud.GroupForecast }) {
  const bars = [...data.history.map((p) => ({ ...p, ahead: false })), ...data.forecast.map((p) => ({ ...p, ahead: true }))]
  if (bars.length === 0) return null
  const top = Math.max(...bars.map((b) => b.demand), data.target_cpu ?? 0, 1)
  const w = 4
  return (
    <svg role="img" aria-label="Group CPU demand: history and forecast" viewBox={`0 0 ${bars.length * w} 60`} className="h-24 w-full" preserveAspectRatio="none">
      {bars.map((b, i) => {
        const h = Math.max(1, (b.demand / top) * 58)
        return (
          <rect key={`${b.ahead ? 'f' : 'h'}${b.hour}`} x={i * w} y={60 - h} width={w - 1} height={h} fill={b.ahead ? 'var(--apple-blue, #0071e3)' : 'var(--text-muted, #86868b)'} opacity={b.ahead ? 0.55 : 0.8}>
            <title>{`${hourLabel(b.hour)} · ${b.demand.toFixed(0)}% CPU${b.needed !== undefined ? ` · ${b.needed} instances` : ''}`}</title>
          </rect>
        )
      })}
    </svg>
  )
}

type Props = {
  group: cloud.InstanceGroup
  policy: cloud.ScalingPolicy
  loadBalancers: NativeLoadBalancer[]
  busy: boolean
  onSave: (policy: cloud.ScalingPolicy) => Promise<void>
}

/** Autoscale settings, members and the demand forecast for one instance group. */
export default function GroupAutoscale({ group, policy, loadBalancers, busy, onSave }: Props) {
  const [open, setOpen] = useState(false)
  const [draft, setDraft] = useState(policy)
  const [detail, setDetail] = useState<cloud.GroupDetail | null>(null)
  const [forecast, setForecast] = useState<cloud.GroupForecast | null>(null)
  const [error, setError] = useState<string | null>(null)
  const policyJson = group.policy_json

  // `policy` is re-parsed every render; only a changed stored policy may reset the draft.
  useEffect(() => { setDraft(policy) }, [policyJson]) // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!open) return
    let alive = true
    Promise.all([cloud.getInstanceGroup(group.id), cloud.getGroupForecast(group.id)])
      .then(([d, f]) => { if (alive) { setDetail(d); setForecast(f); setError(null) } })
      .catch((e) => { if (alive) setError(formatUserError(e)) })
    return () => { alive = false }
  }, [open, group.id, policyJson])

  const set = (patch: Partial<cloud.ScalingPolicy>) => setDraft((d) => ({ ...d, ...patch }))
  const lb = draft.load_balancer ?? null
  const peak = forecast?.next_hour_peak

  return (
    <details className="w-full" open={open} onToggle={(e) => setOpen((e.target as HTMLDetailsElement).open)}>
      <summary className="cursor-pointer text-sm text-[var(--apple-blue,#0071e3)]">Autoscale {group.name}</summary>
      <form
        className="mt-3 flex flex-wrap items-end gap-3"
        aria-label={`Autoscale settings for ${group.name}`}
        onSubmit={(e) => { e.preventDefault(); void onSave(draft) }}
      >
        <label className="flex flex-col gap-1 text-xs">Min<input aria-label="Minimum instances" type="number" min={0} className={field} value={draft.min} onChange={(e) => set({ min: Number(e.target.value) })} /></label>
        <label className="flex flex-col gap-1 text-xs">Max<input aria-label="Maximum instances" type="number" min={1} className={field} value={draft.max} onChange={(e) => set({ max: Number(e.target.value) })} /></label>
        <label className="flex flex-col gap-1 text-xs">Target CPU %
          <input aria-label="Target CPU percent" type="number" min={1} max={100} placeholder="Manual" className={field} value={draft.target_cpu ?? ''} onChange={(e) => set(e.target.value === '' ? { target_cpu: null, predictive: false } : { target_cpu: Number(e.target.value) })} />
        </label>
        <label className="flex flex-col gap-1 text-xs">Cooldown (s)<input aria-label="Cooldown seconds" type="number" min={0} className={field} value={draft.cooldown_secs} onChange={(e) => set({ cooldown_secs: Number(e.target.value) })} /></label>
        <label className="flex min-h-11 items-center gap-2 text-sm">
          <input type="checkbox" aria-label="Predictive scaling" disabled={draft.target_cpu == null} checked={!!draft.predictive} onChange={(e) => set({ predictive: e.target.checked })} />
          Scale ahead of the usual daily and weekly rush
        </label>
        <label className="flex flex-col gap-1 text-xs">On scale-in
          <select aria-label="Scale-in mode" className={wide} value={draft.scale_in ?? 'stop'} onChange={(e) => set({ scale_in: e.target.value as 'stop' | 'sleep' })}>
            <option value="stop">Stop and keep disks</option>
            <option value="sleep">Sleep (memory saved, wakes in seconds)</option>
          </select>
        </label>
        <label className="flex flex-col gap-1 text-xs">Load balancer
          <select aria-label="Load balancer" className={wide} value={lb?.id ?? ''} onChange={(e) => set({ load_balancer: e.target.value ? { id: e.target.value, port: lb?.port ?? 80 } : null })}>
            <option value="">None</option>
            {loadBalancers.map((b) => <option key={b.id} value={b.id}>{b.name} · :{b.listener_port}</option>)}
          </select>
        </label>
        {lb && (
          <>
            <label className="flex flex-col gap-1 text-xs">Member port<input aria-label="Member port" type="number" min={1} max={65535} className={field} value={lb.port} onChange={(e) => set({ load_balancer: { ...lb, port: Number(e.target.value) } })} /></label>
            <label className="flex flex-col gap-1 text-xs">Drain (s)<input aria-label="Drain seconds" type="number" min={0} max={900} placeholder="30" className={field} value={draft.drain_secs ?? ''} onChange={(e) => set({ drain_secs: e.target.value === '' ? null : Number(e.target.value) })} /></label>
          </>
        )}
        <button className="btn btn-primary min-h-11" disabled={busy}>Save autoscaling</button>
      </form>
      {error && <p role="alert" className="mt-2 text-sm">{error}</p>}
      {detail && detail.members.length > 0 && (
        <ul className="mt-3 text-sm" aria-label={`Members of ${group.name}`}>
          {detail.members.map((m) => (
            <li key={m.slot} className="flex flex-wrap gap-2">
              <span>#{m.slot} {m.name ?? 'not created yet'}</span>
              <span className="text-[var(--text-secondary)]">{m.observed_state ?? '—'}{m.desired_state && m.desired_state !== m.observed_state ? ` → ${m.desired_state}` : ''}</span>
              {m.draining_since && <span className="text-[var(--text-secondary)]">draining from the load balancer</span>}
            </li>
          ))}
        </ul>
      )}
      {forecast && (
        <div className="mt-3 space-y-1" aria-label={`Demand forecast for ${group.name}`}>
          <h4 className="text-sm font-semibold">Demand</h4>
          <ForecastChart data={forecast} />
          <p className="text-xs text-[var(--text-secondary)]">
            {peak
              ? `Next hour: about ${peak.demand.toFixed(0)}% CPU across the group, ${peak.needed} instances at the target (${peak.basis} pattern).`
              : `No forecast yet: ${forecast.hours_of_history} hours of history. It needs a few days of hourly data with a repeating pattern.`}
          </p>
        </div>
      )}
    </details>
  )
}
