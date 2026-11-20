// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Service map from the flow history (like Hubble UI): VMs and external
// endpoints, one link per talking pair, coloured by verdict, with ports and
// per-request L7 metrics (rate, denials, status classes, latency).

import { useCallback, useEffect, useMemo, useState } from 'react'
import { MacGlassPanel, MacSegmentedControl } from '../platform/mac/PlatformMacUi'
import { TahoeTableWrap } from '../platform/tahoe/TahoeListKit'
import { Empty, headRowCls, rowCls, thCls } from '../bpf/shared'
import { listFlowEdges, resetFlowEdges, type NetpolScope, type VmFlowEdge } from '../../api/vmNetpol'
import { statusPillClasses } from '../../utils/semanticColors'
import { formatUserError } from '../../utils/apiError'
import { useToastContext } from '../../contexts/ToastContext'
import { buildServiceMap, edgeTone, type MapLink, type MapNode } from '../../utils/serviceMap'

const WINDOWS = [
  { value: '1h', label: '1 hour' },
  { value: '24h', label: '24 hours' },
  { value: '7d', label: '7 days' },
] as const
type Window = (typeof WINDOWS)[number]['value']

const EDGE_ROWS = 12

const TONE_STROKE: Record<string, string> = {
  ok: '#30d158',
  warn: '#ff9f0a',
  error: '#ff453a',
}

function since(w: Window): string {
  const ms = w === '1h' ? 3600e3 : w === '24h' ? 86400e3 : 7 * 86400e3
  return new Date(Date.now() - ms).toISOString().replace(/\.\d+Z$/, 'Z')
}

function fmtCount(n: number): string {
  return n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${(n / 1e3).toFixed(1)}k` : String(n)
}

function nodeColor(n: MapNode): string {
  if (n.kind === 'vm') return '#0071e3'
  if (n.kind === 'world') return '#8e8e93'
  return '#5e5ce6'
}

export default function ServiceMap({ scope, vm }: { scope: NetpolScope; vm?: string }) {
  const toast = useToastContext()
  const [edges, setEdges] = useState<VmFlowEdge[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [win, setWin] = useState<Window>('24h')
  const [onlyDenied, setOnlyDenied] = useState(false)
  const [query, setQuery] = useState('')
  const [selected, setSelected] = useState<string | null>(null)
  const [allEdges, setAllEdges] = useState(false)
  useEffect(() => setAllEdges(false), [selected])

  const load = useCallback(async () => {
    try {
      setEdges(await listFlowEdges(scope, vm))
      setError(null)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [scope, vm])

  useEffect(() => {
    void load()
    const t = window.setInterval(() => void load(), 15000)
    return () => window.clearInterval(t)
  }, [load])

  const map = useMemo(() => {
    const cutoff = since(win)
    const q = query.trim().toLowerCase()
    const kept = edges.filter(
      (e) =>
        e.last_seen >= cutoff &&
        (!onlyDenied || edgeTone(e) !== 'ok') &&
        (!q || e.src.toLowerCase().includes(q) || e.dst.toLowerCase().includes(q) || String(e.port) === q),
    )
    return buildServiceMap(kept)
  }, [edges, win, onlyDenied, query])

  const link: MapLink | undefined = map.links.find((l) => l.id === selected)

  const clear = async () => {
    if (!window.confirm('Clear the flow history on every host in scope? Learn mode, replay and this map start over.')) return
    try {
      await resetFlowEdges(scope)
      toast.success('Flow history cleared')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const W = 960
  const H = Math.max(200, map.height + 60)

  return (
    <div className="space-y-4">
      <MacGlassPanel
        title="Service map"
        subtitle="Who talks to whom, from the 7-day flow history on each host. Green: forwarded. Amber: a policy would drop it (observe mode). Red: dropped."
        action={
          <div className="flex items-center gap-2">
            <input
              aria-label="Filter map"
              className="input text-xs py-1 w-40"
              placeholder="VM, address or port"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            <label className="text-xs flex items-center gap-1">
              <input type="checkbox" checked={onlyDenied} onChange={(e) => setOnlyDenied(e.target.checked)} /> Denied only
            </label>
            <button type="button" className="btn-secondary text-xs" onClick={() => void clear()}>Clear history</button>
          </div>
        }
      >
        <div className="mb-3">
          <MacSegmentedControl label="Seen within" options={[...WINDOWS]} value={win} onChange={setWin} />
        </div>
        {error ? (
          <Empty>{error}</Empty>
        ) : loading ? (
          <Empty>Loading flow history…</Empty>
        ) : map.nodes.length === 0 ? (
          <Empty>No flows in this window yet. The map fills in as VMs talk; flow logging is on whenever a policy is synced.</Empty>
        ) : (
          <div className="overflow-x-auto rounded-xl bg-[#0b0b0d] border border-white/[0.06]">
            <svg viewBox={`0 0 ${W} ${H}`} className="w-full min-w-[720px]" role="img" aria-label="Service map">
              <defs>
                {Object.entries(TONE_STROKE).map(([k, c]) => (
                  <marker key={k} id={`arrow-${k}`} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
                    <path d="M 0 0 L 10 5 L 0 10 z" fill={c} />
                  </marker>
                ))}
              </defs>
              {map.links.map((l) => {
                const a = map.nodes.find((n) => n.id === l.src)
                const b = map.nodes.find((n) => n.id === l.dst)
                if (!a || !b) return null
                const width = Math.min(6, 1 + Math.log10(l.count + 1))
                const active = selected === l.id
                const dir = Math.sign(b.x - a.x)
                const blocked = map.nodes.some(
                  (n) => n.id !== a.id && n.id !== b.id && n.x > Math.min(a.x, b.x) && n.x < Math.max(a.x, b.x) && Math.abs(n.y - (a.y + b.y) / 2) < 24,
                )
                // Backward links bow up so they never sit on the forward link of the same pair.
                const bow = dir < 0 ? -(blocked ? 44 : 22) : blocked ? 44 : 0
                const loop = 60 + Math.abs(a.y - b.y) / 4
                let x1: number, x2: number, d: string, lx: number
                if (dir === 0) {
                  x1 = a.x * W + 70
                  x2 = b.x * W + 70
                  d = `M ${x1} ${a.y} C ${x1 + loop} ${a.y}, ${x2 + loop} ${b.y}, ${x2} ${b.y}`
                  lx = x1 + loop * 0.75
                } else {
                  x1 = a.x * W + 70 * dir
                  x2 = b.x * W - 70 * dir
                  const dx = Math.max(40, Math.abs(x2 - x1) / 2) * dir
                  d = `M ${x1} ${a.y} C ${x1 + dx} ${a.y + bow}, ${x2 - dx} ${b.y + bow}, ${x2} ${b.y}`
                  lx = (x1 + x2) / 2
                }
                return (
                  <g key={l.id} className="cursor-pointer" onClick={() => setSelected(active ? null : l.id)}>
                    <path d={d} fill="none" stroke="transparent" strokeWidth={14} />
                    <path
                      d={d}
                      fill="none"
                      stroke={TONE_STROKE[l.tone]}
                      strokeOpacity={active ? 1 : 0.75}
                      strokeWidth={active ? width + 2 : width}
                      strokeDasharray={l.tone === 'warn' ? '6 4' : undefined}
                      markerEnd={`url(#arrow-${l.tone})`}
                    >
                      <title>{`${l.src} → ${l.dst}  ${l.ports.join(', ')}  ${l.count} flows`}</title>
                    </path>
                    <text x={lx} y={(a.y + b.y) / 2 + bow * 0.75 - 6} textAnchor="middle" className="fill-[#a1a1a6] text-[10px] font-mono">
                      {l.ports.slice(0, 3).join(' ')}{l.ports.length > 3 ? ' …' : ''}
                    </text>
                  </g>
                )
              })}
              {map.nodes.map((n) => (
                <g key={n.id} transform={`translate(${n.x * W}, ${n.y})`}>
                  <rect x={-68} y={-16} width={136} height={32} rx={10} fill="#1c1c1e" stroke={nodeColor(n)} strokeWidth={1.5} />
                  <circle cx={-54} cy={0} r={4} fill={nodeColor(n)} />
                  <text x={-44} y={4} className="fill-[#f5f5f7] text-[11px]">
                    {n.label.length > 16 ? `${n.label.slice(0, 15)}…` : n.label}
                    <title>{n.id}</title>
                  </text>
                </g>
              ))}
            </svg>
          </div>
        )}
        <div className="mt-2 text-xs text-[var(--text-muted)]">
          {map.nodes.length} endpoints · {map.links.length} links · click a link for ports, policies and L7 metrics
          {map.collapsed > 0 ? ` · ${map.collapsed} external addresses grouped as "other"` : ''}
        </div>
        {map.links.length > 0 && (
          <ul className="mt-2 flex flex-wrap gap-1.5" aria-label="Links">
            {map.links.slice(0, 40).map((l) => (
              <li key={l.id}>
                <button
                  type="button"
                  aria-pressed={selected === l.id}
                  className={`text-xs px-2 py-1 rounded-full border font-mono ${
                    selected === l.id ? 'border-[#0071e3] text-[var(--text-primary)]' : 'border-white/[0.08] text-[var(--text-muted)]'
                  }`}
                  onClick={() => setSelected(selected === l.id ? null : l.id)}
                >
                  <span style={{ color: TONE_STROKE[l.tone] }}>●</span> {l.src} → {l.dst} · {fmtCount(l.count)}
                </button>
              </li>
            ))}
          </ul>
        )}
      </MacGlassPanel>

      {link && (
        <MacGlassPanel title={`${link.src} → ${link.dst}`} subtitle={`${fmtCount(link.count)} flows · ${link.ports.join(', ')}`}>
          <TahoeTableWrap>
            <table className="w-full text-xs" aria-label="Link edges">
              <thead>
                <tr className={headRowCls}>
                  <th scope="col" className={thCls}>Port</th>
                  <th scope="col" className={thCls}>Seen at</th>
                  <th scope="col" className={thCls}>Verdict</th>
                  <th scope="col" className={thCls}>Policy</th>
                  <th scope="col" className={thCls}>Flows</th>
                  {scope === 'fleet' && <th scope="col" className={thCls}>Host</th>}
                  <th scope="col" className="py-2">Last seen</th>
                </tr>
              </thead>
              <tbody>
                {(allEdges ? link.edges : link.edges.slice(0, EDGE_ROWS)).map((e, i) => (
                  <tr key={i} className={rowCls}>
                    <td className="py-2 pr-2 font-mono">{e.proto}/{e.port}</td>
                    <td className="py-2 pr-2">{e.direction === 'egress' ? `egress of ${e.src}` : `ingress of ${e.dst}`}</td>
                    <td className="py-2 pr-2">
                      <span className={statusPillClasses(edgeTone(e) === 'ok' ? 'ok' : edgeTone(e) === 'warn' ? 'warn' : 'error')}>
                        {e.verdict}{e.drop_reason ? ` · ${e.drop_reason}` : ''}
                      </span>
                    </td>
                    <td className="py-2 pr-2 font-mono">{e.policy ?? '—'}</td>
                    <td className="py-2 pr-2">{fmtCount(e.count)}</td>
                    {scope === 'fleet' && <td className="py-2 pr-2">{e.host ?? '—'}</td>}
                    <td className="py-2 text-[var(--text-muted)]">{e.last_seen}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
          {link.edges.length > EDGE_ROWS && (
            <button type="button" className="btn-secondary text-xs mt-2" onClick={() => setAllEdges(!allEdges)}>
              {allEdges ? 'Show busiest only' : `Show all ${link.edges.length} ports`}
            </button>
          )}
          <L7Metrics link={link} />
        </MacGlassPanel>
      )}
    </div>
  )
}

function L7Metrics({ link }: { link: MapLink }) {
  if (link.l7.length === 0) {
    return (
      <p className="mt-3 text-xs text-[var(--text-muted)]">
        No L7 requests on this link. Requests are parsed where an L7 rule applies; status codes and latency come from rules that go through the bpfd proxy (TLS interception or header rewrites).
      </p>
    )
  }
  return (
    <TahoeTableWrap>
      <table className="w-full text-xs mt-3" aria-label="L7 metrics">
        <thead>
          <tr className={headRowCls}>
            <th scope="col" className={thCls}>Request</th>
            <th scope="col" className={thCls}>Count</th>
            <th scope="col" className={thCls}>Denied</th>
            <th scope="col" className={thCls}>Status</th>
            <th scope="col" className={thCls}>Avg latency</th>
            <th scope="col" className="py-2">Max latency</th>
          </tr>
        </thead>
        <tbody>
          {link.l7.map((s) => {
            const errRate = s.status ? ((s.status['5xx'] ?? 0) / Math.max(1, s.latency_n)) * 100 : 0
            return (
              <tr key={s.kind + s.request} className={rowCls}>
                <td className="py-2 pr-2 font-mono">{s.request}</td>
                <td className="py-2 pr-2">{fmtCount(s.count)}</td>
                <td className="py-2 pr-2">
                  {s.denied > 0 ? <span className={statusPillClasses('error')}>{fmtCount(s.denied)}</span> : '0'}
                </td>
                <td className="py-2 pr-2 font-mono">
                  {s.status && Object.keys(s.status).length > 0
                    ? Object.entries(s.status).map(([k, v]) => `${k}:${v}`).join(' ')
                    : '—'}
                  {errRate >= 1 && <span className={`ml-2 ${statusPillClasses('error')}`}>{errRate.toFixed(0)}% 5xx</span>}
                </td>
                <td className="py-2 pr-2">{s.latency_n ? `${Math.round(s.latency_ms_total / s.latency_n)} ms` : '—'}</td>
                <td className="py-2">{s.latency_n ? `${s.latency_ms_max} ms` : '—'}</td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </TahoeTableWrap>
  )
}
