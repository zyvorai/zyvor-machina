// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// VM runtime intelligence: KVM exits, vCPU scheduling, block / fault /
// reclaim latency, boot time and per-CPU interrupt load, per VM.

import { useCallback, useState } from 'react'
import { MacGlassPanel, MacToggle } from '../platform/mac/PlatformMacUi'
import {
  getBpfVmIntel,
  getBpfVmIntelVm,
  setBpfVmIntel,
  type BpfVmIntelFeature,
  type BpfVmIntelHist,
} from '../../api/bpf'
import { statusPillClasses } from '../../utils/semanticColors'
import { Empty, Metrics, headRowCls, rowCls, thCls, useBpfAction, useBpfLoad } from './shared'

const FEATURES: { id: BpfVmIntelFeature; label: string }[] = [
  { id: 'flight', label: 'KVM exits & vCPU scheduling' },
  { id: 'io', label: 'Block & vhost I/O' },
  { id: 'mem', label: 'Faults, reclaim & boot' },
  { id: 'topology', label: 'Per-CPU interrupts' },
]

function fmtNs(ns: number): string {
  if (ns >= 1e9) return `${(ns / 1e9).toFixed(2)} s`
  if (ns >= 1e6) return `${(ns / 1e6).toFixed(1)} ms`
  if (ns >= 1e3) return `${(ns / 1e3).toFixed(1)} µs`
  return `${ns} ns`
}

function Hist({ title, h }: { title: string; h: BpfVmIntelHist }) {
  const max = Math.max(1, ...h.buckets.map((b) => b.count))
  return (
    <div className="space-y-1">
      <div className="flex items-baseline justify-between text-sm">
        <span className="font-medium text-[var(--text-primary)]">{title}</span>
        <span className="text-xs text-[var(--text-muted)]">
          {h.count ? `${h.count.toLocaleString()} · p50 < ${fmtNs(h.p50_ns)} · p99 < ${fmtNs(h.p99_ns)}` : 'no samples'}
        </span>
      </div>
      {h.buckets.map((b) => (
        <div key={b.le_ns} className="flex items-center gap-2 text-xs">
          <span className="w-20 text-right font-mono text-[var(--text-muted)]">&lt; {fmtNs(b.le_ns)}</span>
          <div className="h-2 flex-1 rounded bg-[var(--border-subtle)]">
            <div className="h-2 rounded bg-[#0071e3]" style={{ width: `${(b.count / max) * 100}%` }} />
          </div>
          <span className="w-16 tabular-nums">{b.count.toLocaleString()}</span>
        </div>
      ))}
    </div>
  )
}

function VmReport({ name }: { name: string }) {
  const { data: r, error, reload } = useBpfLoad(useCallback(() => getBpfVmIntelVm(name), [name]))
  if (error) return <Empty>{error}</Empty>
  if (!r) return null
  const resTotal = r.residency.reduce((s, x) => s + x.ns, 0) || 1
  return (
    <MacGlassPanel
      title={`Runtime · ${name}`}
      action={<button type="button" className="btn-secondary text-sm" onClick={() => void reload()}>Refresh</button>}
    >
      <div className="space-y-4">
        <Metrics
          items={[
            { label: 'KVM exits', value: r.exits.reduce((s, e) => s + e.count, 0).toLocaleString() },
            { label: 'vCPU migrations', value: r.migrations.toLocaleString() },
            { label: 'vhost kicks / work', value: `${r.vhost_kicks.toLocaleString()} / ${r.vhost_work.toLocaleString()}` },
            { label: 'Boot → first KVM entry', value: r.boot_to_first_entry_ms != null ? `${r.boot_to_first_entry_ms.toFixed(0)} ms` : '—' },
          ]}
        />
        <div className="grid gap-4 md:grid-cols-2">
          <Hist title="vCPU run-queue latency" h={r.runq} />
          <Hist title="Block I/O latency" h={r.block} />
          <Hist title="Page-fault latency" h={r.fault} />
          <Hist title="Direct-reclaim stalls" h={r.reclaim} />
        </div>
        <div className="grid gap-4 md:grid-cols-2">
          <div>
            <p className="mb-1 text-sm font-medium text-[var(--text-primary)]">Exit reasons</p>
            {!r.exits.length ? <Empty>No exits recorded.</Empty> : (
              <table className="w-full text-sm">
                <tbody>
                  {r.exits.slice(0, 12).map((e) => (
                    <tr key={e.reason} className={rowCls}>
                      <td className="py-1 pr-2 font-mono">{e.name}</td>
                      <td className="py-1 pr-2 text-right tabular-nums">{e.count.toLocaleString()}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
          <div>
            <p className="mb-1 text-sm font-medium text-[var(--text-primary)]">vCPU residency by physical CPU</p>
            {!r.residency.length ? <Empty>No residency data.</Empty> : r.residency.map((x) => (
              <div key={x.cpu} className="flex items-center gap-2 text-xs">
                <span className="w-12 font-mono text-[var(--text-muted)]">cpu{x.cpu}</span>
                <div className="h-2 flex-1 rounded bg-[var(--border-subtle)]">
                  <div className="h-2 rounded bg-[#0071e3]" style={{ width: `${(x.ns / resTotal) * 100}%` }} />
                </div>
                <span className="w-20 tabular-nums">{fmtNs(x.ns)}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </MacGlassPanel>
  )
}

export default function VmRuntimeTab() {
  const action = useBpfAction()
  const { data: st, setData, error, reload } = useBpfLoad(useCallback(() => getBpfVmIntel(), []))
  const [selected, setSelected] = useState<string | null>(null)

  if (error) return <MacGlassPanel title="VM runtime"><Empty>{error}</Empty></MacGlassPanel>
  if (!st) return null
  const features = st.config.features.length ? st.config.features : FEATURES.map((f) => f.id)
  const save = (patch: Partial<typeof st.config>) =>
    void action(setBpfVmIntel({ ...st.config, ...patch }), 'VM runtime intelligence updated').then((r) => { if (r) setData(r) })
  const maxIrq = Math.max(1, ...st.cpus.map((c) => c.irq_ns + c.softirq_ns))

  return (
    <>
      <MacGlassPanel
        title="VM runtime intelligence"
        subtitle="Tracepoints and kprobes scoped to QEMU processes. Observe only; scheduler hooks run on a hot path, so this stays off until enabled."
        action={<span className={statusPillClasses(st.config.enabled ? 'ok' : 'neutral')}>{st.config.enabled ? `${st.hooks.length} hooks` : 'Off'}</span>}
      >
        <div className="space-y-3">
          <MacToggle label="Enabled" checked={st.config.enabled} onChange={(enabled) => save({ enabled })} />
          <div className="flex flex-wrap gap-4">
            {FEATURES.map((f) => (
              <MacToggle
                key={f.id}
                label={f.label}
                checked={features.includes(f.id)}
                onChange={(on) => save({ features: on ? [...features, f.id] : features.filter((x) => x !== f.id) })}
              />
            ))}
          </div>
          {st.notes.map((n) => <p key={n} className="text-xs text-[var(--text-muted)]">{n}</p>)}
        </div>
      </MacGlassPanel>
      <MacGlassPanel
        title="Tracked VMs"
        action={<button type="button" className="btn-secondary text-sm" onClick={() => void reload()}>Refresh</button>}
      >
        {!st.vms.length ? (
          <Empty>{st.config.enabled ? 'No running VMs found.' : 'Enable to track running VMs.'}</Empty>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className={headRowCls}>
                  <th className={thCls}>VM</th>
                  <th className={thCls}>vCPUs</th>
                  <th className={thCls}>Threads</th>
                  <th className={thCls}>Cgroup</th>
                </tr>
              </thead>
              <tbody>
                {st.vms.map((v) => (
                  <tr
                    key={v.name}
                    className={`${rowCls} cursor-pointer ${selected === v.name ? 'bg-[var(--border-subtle)]' : ''}`}
                    onClick={() => setSelected(v.name)}
                  >
                    <td className="py-1.5 pr-2 font-medium text-[#0071e3]">{v.name}</td>
                    <td className="py-1.5 pr-2 tabular-nums">{v.vcpus}</td>
                    <td className="py-1.5 pr-2 tabular-nums">{v.threads}</td>
                    <td className="py-1.5 pr-2 font-mono text-xs">{v.cgroup}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </MacGlassPanel>
      {selected && <VmReport name={selected} />}
      {st.cpus.length > 0 && (
        <MacGlassPanel title="Interrupt load by CPU" subtitle="Hardirq + softirq time since enabled.">
          <div className="space-y-1">
            {st.cpus.map((c) => (
              <div key={c.cpu} className="flex items-center gap-2 text-xs">
                <span className="w-12 font-mono text-[var(--text-muted)]">cpu{c.cpu}</span>
                <div className="flex h-2 flex-1 overflow-hidden rounded bg-[var(--border-subtle)]">
                  <div className="h-2 bg-[#0071e3]" style={{ width: `${(c.irq_ns / maxIrq) * 100}%` }} />
                  <div className="h-2 bg-[#2997ff] opacity-60" style={{ width: `${(c.softirq_ns / maxIrq) * 100}%` }} />
                </div>
                <span className="w-32 tabular-nums">{fmtNs(c.irq_ns)} / {fmtNs(c.softirq_ns)}</span>
              </div>
            ))}
          </div>
        </MacGlassPanel>
      )}
    </>
  )
}
