// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// VM-aware sched_ext scheduler: listed VMs' vCPU threads run on the
// machina_scx BPF scheduler under a lease; everything else stays on CFS.

import { useCallback, useState } from 'react'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import { getBpfScx, setBpfScx } from '../../api/bpf'
import { statusPillClasses } from '../../utils/semanticColors'
import { Empty, Field, Metrics, headRowCls, rowCls, thCls, useBpfAction, useBpfLoad } from './shared'

export default function SchedulerTab() {
  const action = useBpfAction()
  const { data: st, setData, error, reload } = useBpfLoad(useCallback(() => getBpfScx(), []))
  const [vms, setVms] = useState('')
  const [lease, setLease] = useState(600)
  const [target, setTarget] = useState(500)

  if (error) return <MacGlassPanel title="Scheduler"><Empty>{error}</Empty></MacGlassPanel>
  if (!st) return null
  const apply = (p: Promise<typeof st>, msg: string) => void action(p, msg).then((r) => { if (r) setData(r) })
  const pill = st.running
    ? { tone: 'warn' as const, text: `Running · ${st.lease_remaining_secs ?? 0}s left` }
    : st.supported
      ? { tone: 'neutral' as const, text: 'Off' }
      : { tone: 'error' as const, text: 'Unsupported' }

  return (
    <>
      <Metrics
        items={[
          { label: 'Kernel state', value: st.kernel_state },
          { label: 'Scheduler', value: st.ops ?? '—' },
          { label: 'VMs', value: st.vms.length.toLocaleString() },
          { label: 'Rejected tasks', value: st.nr_rejected.toLocaleString() },
        ]}
      />
      <MacGlassPanel
        title="VM-aware scheduler (sched_ext)"
        subtitle="Moves only the listed VMs' vCPU threads onto a BPF scheduler with per-VM weighting and dispatch-latency accounting. It always runs under a lease; when the lease ends, the helper exits or you stop it, the threads go back to CFS."
        action={<span className={statusPillClasses(pill.tone)}>{pill.text}</span>}
      >
        <div className="space-y-3">
          {!st.supported && <p className="text-sm text-[var(--text-secondary)]">This kernel was built without CONFIG_SCHED_CLASS_EXT.</p>}
          {st.running ? (
            <div className="flex gap-2">
              <button type="button" className="btn-secondary text-sm" onClick={() => void reload()}>Refresh</button>
              <button type="button" className="btn-secondary text-sm" onClick={() => apply(setBpfScx({ enabled: false }), 'Scheduler stopped')}>
                Stop
              </button>
            </div>
          ) : (
            <div className="flex flex-wrap items-end gap-2">
              <Field label="VMs" htmlFor="scx-vms">
                <input id="scx-vms" className="input text-sm w-64" placeholder="vm-a, vm-b" value={vms} onChange={(e) => setVms(e.target.value)} />
              </Field>
              <Field label="Lease (s)" htmlFor="scx-lease">
                <input id="scx-lease" type="number" min={1} max={3600} className="input text-sm w-24" value={lease} onChange={(e) => setLease(Number(e.target.value))} />
              </Field>
              <Field label="Latency target (µs)" htmlFor="scx-target">
                <input id="scx-target" type="number" min={0} className="input text-sm w-28" value={target} onChange={(e) => setTarget(Number(e.target.value))} />
              </Field>
              <button
                type="button"
                className="btn-primary text-sm"
                disabled={!st.supported || !st.helper || !vms.trim()}
                onClick={() =>
                  apply(
                    setBpfScx({ enabled: true, lease_secs: lease, latency_target_us: target || null, vms: vms.split(/[\s,]+/).filter(Boolean) }),
                    `Scheduler running for ${lease}s`,
                  )
                }
              >
                Start under lease
              </button>
            </div>
          )}
          {st.last_exit && !st.running && <p className="text-xs text-[var(--text-muted)]">Last run ended: {st.last_exit}</p>}
          {st.notes.map((n) => <p key={n} className="text-xs text-[var(--text-muted)]">{n}</p>)}
          {st.vms.length === 0 ? (
            <Empty>No VMs scheduled.</Empty>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className={headRowCls}>
                    <th className={thCls}>VM</th>
                    <th className={thCls}>vCPUs</th>
                    <th className={thCls}>Dispatches</th>
                    <th className={thCls}>Avg / max queue delay</th>
                    <th className={thCls}>Runtime</th>
                    <th className={thCls}>Over target</th>
                  </tr>
                </thead>
                <tbody>
                  {st.vms.map((v) => (
                    <tr key={v.name} className={rowCls}>
                      <td className="py-1.5 pr-2">{v.name}</td>
                      <td className="py-1.5 pr-2">{v.vcpus}</td>
                      <td className="py-1.5 pr-2">{v.dispatches.toLocaleString()}</td>
                      <td className="py-1.5 pr-2">{v.avg_queue_delay_us.toFixed(1)} / {v.max_queue_delay_us.toFixed(0)} µs</td>
                      <td className="py-1.5 pr-2">{(v.runtime_ms / 1000).toFixed(1)} s</td>
                      <td className="py-1.5 pr-2">{v.latency_violations.toLocaleString()}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </MacGlassPanel>
    </>
  )
}
