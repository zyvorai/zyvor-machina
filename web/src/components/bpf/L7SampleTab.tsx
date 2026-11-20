// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Sampled plaintext L7: Redis, PostgreSQL, MySQL, Kafka and HTTP/2 (gRPC)
// operations seen on this host's sockets. Only operation names are kept.

import { useCallback, useState } from 'react'
import { MacGlassPanel, MacToggle } from '../platform/mac/PlatformMacUi'
import {
  getBpfL7,
  getBpfL7Sample,
  setBpfL7Sample,
  workloadLabel,
  type BpfL7SampleConfig,
  type BpfL7SampleProtocol,
} from '../../api/bpf'
import { statusPillClasses } from '../../utils/semanticColors'
import { Empty, Field, Metrics, headRowCls, rowCls, thCls, useBpfAction, useBpfLoad } from './shared'

const PROTOCOLS: BpfL7SampleProtocol[] = ['redis', 'postgres', 'mysql', 'kafka', 'http2']
const DEFAULT_PORTS: BpfL7SampleConfig['ports'] = [
  { port: 6379, protocol: 'redis' },
  { port: 5432, protocol: 'postgres' },
  { port: 3306, protocol: 'mysql' },
  { port: 9092, protocol: 'kafka' },
  { port: 50051, protocol: 'http2' },
]

export default function L7SampleTab() {
  const action = useBpfAction()
  const { data: st, setData, error, reload: reloadStatus } = useBpfLoad(useCallback(() => getBpfL7Sample(), []))
  const [proto, setProto] = useState<string>('')
  const [newPort, setNewPort] = useState('')
  const [newProto, setNewProto] = useState<BpfL7SampleProtocol>('redis')
  const { data: records, reload } = useBpfLoad(
    useCallback(() => getBpfL7(300, proto || undefined), [proto]),
  )

  if (error) return <MacGlassPanel title="L7 sampling"><Empty>{error}</Empty></MacGlassPanel>
  if (!st) return null
  const ports = st.config.ports.length ? st.config.ports : DEFAULT_PORTS
  const save = (patch: Partial<BpfL7SampleConfig>) =>
    void action(setBpfL7Sample({ ...st.config, ...patch }), 'L7 sampling updated').then((r) => { if (r) setData(r) })
  const addPort = () => {
    const port = Number(newPort)
    if (!Number.isInteger(port) || port < 1 || port > 65535) return
    save({ ports: [...ports.filter((p) => p.port !== port), { port, protocol: newProto }] })
    setNewPort('')
  }
  const sampled = (records ?? []).filter((r) => PROTOCOLS.includes(r.protocol as BpfL7SampleProtocol))

  return (
    <>
      <Metrics
        items={[
          { label: 'Eligible segments', value: st.eligible.toLocaleString() },
          { label: 'Sampled', value: st.emitted.toLocaleString() },
          { label: 'Rate limited', value: st.rate_limited.toLocaleString() },
          { label: 'Undecoded', value: st.undecoded.toLocaleString() },
          { label: 'Dropped', value: (st.ringbuf_full + st.load_fail).toLocaleString() },
        ]}
      />
      <MacGlassPanel
        title="L7 sampling"
        subtitle="cgroup_skb programs sample one segment per flow and direction to known service ports. Only the operation name is kept — never keys, values or query parameters. Observe only."
        action={<span className={statusPillClasses(st.attached ? 'ok' : 'neutral')}>{st.attached ? `Sampling ${st.attached}` : 'Off'}</span>}
      >
        <div className="space-y-3">
          <div className="flex flex-wrap items-end gap-4">
            <MacToggle label="Enabled" checked={st.config.enabled} onChange={(enabled) => save({ enabled })} />
            <Field label="Flow gap (ms)" htmlFor="l7s-gap">
              <input
                id="l7s-gap"
                type="number"
                min={0}
                className="input text-sm w-28"
                defaultValue={st.config.flow_gap_ms}
                onBlur={(e) => { const v = Number(e.target.value); if (v !== st.config.flow_gap_ms && v >= 0) save({ flow_gap_ms: v }) }}
              />
            </Field>
            <Field label="Samples / s" htmlFor="l7s-rate">
              <input
                id="l7s-rate"
                type="number"
                min={1}
                max={10000}
                className="input text-sm w-28"
                defaultValue={st.config.rate}
                onBlur={(e) => { const v = Number(e.target.value); if (v !== st.config.rate && v >= 1) save({ rate: v }) }}
              />
            </Field>
          </div>
          <div className="flex flex-wrap gap-2">
            {ports.map((p) => (
              <span key={p.port} className="inline-flex items-center gap-1 rounded-full border border-[var(--border-subtle)] px-2 py-0.5 text-xs font-mono">
                {p.port} · {p.protocol}
                <button
                  type="button"
                  aria-label={`Remove port ${p.port}`}
                  className="text-[var(--text-muted)] hover:text-[var(--text-primary)]"
                  onClick={() => save({ ports: ports.filter((x) => x.port !== p.port) })}
                >
                  ×
                </button>
              </span>
            ))}
          </div>
          <div className="flex flex-wrap items-end gap-2">
            <Field label="Port" htmlFor="l7s-port">
              <input id="l7s-port" className="input text-sm font-mono w-24" value={newPort} onChange={(e) => setNewPort(e.target.value.trim())} />
            </Field>
            <Field label="Protocol" htmlFor="l7s-proto">
              <select id="l7s-proto" className="input text-sm" value={newProto} onChange={(e) => setNewProto(e.target.value as BpfL7SampleProtocol)}>
                {PROTOCOLS.map((p) => <option key={p} value={p}>{p}</option>)}
              </select>
            </Field>
            <button type="button" className="btn-secondary text-sm" onClick={addPort}>Add port</button>
          </div>
          {st.notes.map((n) => <p key={n} className="text-xs text-[var(--text-muted)]">{n}</p>)}
        </div>
      </MacGlassPanel>
      <MacGlassPanel
        title="Top operations"
        action={<button type="button" className="btn-secondary text-sm" onClick={() => void reloadStatus()}>Refresh</button>}
      >
        {!st.top.length ? (
          <Empty>No operations sampled yet.</Empty>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className={headRowCls}>
                  <th className={thCls}>Protocol</th>
                  <th className={thCls}>Operation</th>
                  <th className={thCls}>Samples</th>
                </tr>
              </thead>
              <tbody>
                {st.top.map((o) => (
                  <tr key={`${o.protocol}-${o.op}`} className={rowCls}>
                    <td className="py-1.5 pr-2">{o.protocol}</td>
                    <td className="py-1.5 pr-2 font-mono">{o.op}</td>
                    <td className="py-1.5 pr-2 tabular-nums">{o.count.toLocaleString()}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </MacGlassPanel>
      <MacGlassPanel
        title="Recent samples"
        action={
          <div className="flex items-end gap-2">
            <Field label="Protocol" htmlFor="l7s-filter">
              <select id="l7s-filter" className="input text-sm" value={proto} onChange={(e) => setProto(e.target.value)}>
                <option value="">All</option>
                {PROTOCOLS.map((p) => <option key={p} value={p}>{p}</option>)}
              </select>
            </Field>
            <button type="button" className="btn-secondary text-sm" onClick={() => void reload()}>Refresh</button>
          </div>
        }
      >
        {!sampled.length ? (
          <Empty>No samples recorded yet.</Empty>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className={headRowCls}>
                  <th className={thCls}>Time</th>
                  <th className={thCls}>Protocol</th>
                  <th className={thCls}>Operation</th>
                  <th className={thCls}>Client</th>
                  <th className={thCls}>Server</th>
                  <th className={thCls}>Workload</th>
                </tr>
              </thead>
              <tbody>
                {sampled.map((r, i) => (
                  <tr key={`${r.ts}-${i}`} className={rowCls}>
                    <td className="py-1.5 pr-2 whitespace-nowrap">{new Date(r.ts).toLocaleTimeString()}</td>
                    <td className="py-1.5 pr-2">{r.protocol}</td>
                    <td className="py-1.5 pr-2 font-mono">{r.method}{r.path ? ` ${r.path}` : ''}</td>
                    <td className="py-1.5 pr-2 font-mono">{r.client}:{r.client_port}</td>
                    <td className="py-1.5 pr-2 font-mono">{r.server}:{r.server_port}</td>
                    <td className="py-1.5 pr-2">{workloadLabel(r.workload) || r.direction}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </MacGlassPanel>
    </>
  )
}
