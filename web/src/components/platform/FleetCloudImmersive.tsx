// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useId, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { hostTone, ringPositions, type MapHost } from './FleetCloudMap'

export type ImmersiveHost = MapHost & {
  address?: string
  agent_grpc_addr?: string
  memory_used_mib?: number
  memory_total_mib?: number
  last_heartbeat_at?: string | null
}

const COLOR = { ok: '#30d158', warn: '#ffd60a', error: '#ff453a', idle: '#8e8e93' }
const W = 960
const H = 560
const CX = W / 2
const CY = H / 2

export const pct = (used?: number, total?: number) => (total && total > 0 ? Math.min(100, Math.round(((used ?? 0) / total) * 100)) : 0)

/** Ring positions scaled for the large canvas. */
function layout(n: number) {
  return ringPositions(n).map((p) => ({ x: CX + (p.x - 360) * 1.25, y: CY + (p.y - 190) * 1.35 }))
}

const ARC = (r: number, frac: number) => {
  const c = 2 * Math.PI * r
  return { strokeDasharray: `${(c * Math.max(0, Math.min(1, frac))).toFixed(1)} ${c.toFixed(1)}` }
}

/** The big picture: the controller in the middle, every machine around it with live CPU and memory
 *  rings, its VMs orbiting, its IP, and packets flowing along each link. Click a machine for details. */
export default function FleetCloudImmersive({ hosts, selectedId, onSelect, attentionIds, showDetail = true }: { hosts: ImmersiveHost[]; selectedId?: string | null; onSelect?: (id: string | null) => void; /** Hosts that need a person: drawn with a pulsing red ring. */ attentionIds?: ReadonlySet<string>; /** The strip under the map; the Hosts page shows a drawer instead. */ showDetail?: boolean }) {
  const uid = useId().replace(/:/g, '')
  const [hover, setHover] = useState<string | null>(null)
  const pos = useMemo(() => layout(hosts.length), [hosts.length])
  const sel = hosts.find((h) => h.id === (selectedId ?? hover)) ?? null
  const online = hosts.filter((h) => hostTone(h) === 'ok').length
  const vms = hosts.reduce((a, h) => a + (h.vm_count || 0), 0)

  return (
    <div data-testid="fleet-cloud-immersive" className="space-y-3">
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" role="img" aria-label={`Cloud map: ${online} of ${hosts.length} machines online, ${vms} VMs`}
        style={{ display: 'block', borderRadius: 16, background: 'radial-gradient(ellipse at 50% 45%,#16264a 0%,#0a1020 62%,#05070d 100%)' }}>
        <defs>
          <radialGradient id={`${uid}hub`} cx="50%" cy="38%" r="75%"><stop offset="0%" stopColor="#6cb6ff" /><stop offset="100%" stopColor="#0a4fbf" /></radialGradient>
          <filter id={`${uid}glow`} x="-60%" y="-60%" width="220%" height="220%"><feGaussianBlur stdDeviation="6" result="b" /><feMerge><feMergeNode in="b" /><feMergeNode in="SourceGraphic" /></feMerge></filter>
          <pattern id={`${uid}grid`} width="40" height="40" patternUnits="userSpaceOnUse"><path d="M40 0H0V40" fill="none" stroke="#ffffff" strokeOpacity=".04" /></pattern>
        </defs>
        <rect width={W} height={H} fill={`url(#${uid}grid)`} />
        {Array.from({ length: 36 }, (_, i) => (
          <circle key={i} cx={(i * 197) % W} cy={(i * 131) % H} r={(i % 3) * 0.5 + 0.6} fill="#fff" opacity={0.12 + (i % 5) * 0.05} />
        ))}

        {[70, 105, 140].map((r, i) => (
          <circle key={r} cx={CX} cy={CY} r={r} fill="none" stroke="#4aa3ff" strokeOpacity={0.18 - i * 0.04} strokeDasharray="3 8" className="fci-ring" style={{ animationDuration: `${18 + i * 7}s` }} />
        ))}

        {hosts.map((h, i) => {
          const p = pos[i]
          const t = hostTone(h)
          const mx = (CX + p.x) / 2 + (p.y - CY) * 0.12
          const my = (CY + p.y) / 2 - (p.x - CX) * 0.12
          const d = `M ${CX} ${CY} Q ${mx} ${my} ${p.x} ${p.y}`
          return (
            <g key={`l${h.id}`}>
              <path d={d} fill="none" stroke={COLOR[t]} strokeOpacity={t === 'ok' ? 0.5 : 0.35} strokeWidth={1.6} strokeDasharray={t === 'ok' ? undefined : '5 6'} />
              {t === 'ok' && [0, 1, 2].map((k) => (
                <circle key={k} r={3} fill="#9fd0ff" opacity=".9">
                  <animateMotion dur={`${2.6 + (i % 3) * 0.5}s`} begin={`${k * 0.9}s`} repeatCount="indefinite" path={d} />
                </circle>
              ))}
            </g>
          )
        })}

        <g filter={`url(#${uid}glow)`}><circle cx={CX} cy={CY} r={50} fill={`url(#${uid}hub)`} /></g>
        <text x={CX} y={CY - 3} textAnchor="middle" fill="#fff" fontSize="15" fontWeight="700" fontFamily="-apple-system,system-ui,sans-serif">Controller</text>
        <text x={CX} y={CY + 15} textAnchor="middle" fill="#d6e9ff" fontSize="11" fontFamily="-apple-system,system-ui,sans-serif">{hosts.length} machine{hosts.length === 1 ? '' : 's'} · {vms} VM{vms === 1 ? '' : 's'}</text>

        {hosts.map((h, i) => {
          const p = pos[i]
          const t = hostTone(h)
          const cpu = Math.round(h.cpu_percent || 0)
          const mem = pct(h.memory_used_mib, h.memory_total_mib)
          const isSel = h.id === (selectedId ?? null)
          const dots = Math.min(h.vm_count || 0, 14)
          return (
            <g key={h.id} data-testid={`cloud-node-${h.hostname}`} style={{ cursor: 'pointer' }} tabIndex={0} role="button" aria-label={`${h.hostname} ${h.address ?? ''} ${h.state}`}
              onMouseEnter={() => setHover(h.id)} onMouseLeave={() => setHover(null)} onClick={() => onSelect?.(isSel ? null : h.id)}
              onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') onSelect?.(isSel ? null : h.id) }}>
              {attentionIds?.has(h.id) && <circle cx={p.x} cy={p.y} r={46} fill="none" stroke={COLOR.error} strokeWidth={2} className="fci-attn" data-testid={`cloud-attention-${h.hostname}`} />}
              {isSel && <circle cx={p.x} cy={p.y} r={58} fill="none" stroke="#0a84ff" strokeWidth={2} strokeDasharray="6 5" className="fci-ring" />}
              <g className="fci-orbit" style={{ transformOrigin: `${p.x}px ${p.y}px` }}>
                {Array.from({ length: dots }, (_, k) => {
                  const a = (2 * Math.PI * k) / Math.max(dots, 1)
                  return <circle key={k} cx={p.x + 50 * Math.cos(a)} cy={p.y + 50 * Math.sin(a)} r={3.2} fill="#64d2ff" opacity=".9" />
                })}
              </g>
              <circle cx={p.x} cy={p.y} r={39} fill="#0b1426" stroke="#1d2a44" strokeWidth={1} />
              <circle cx={p.x} cy={p.y} r={36} fill="none" stroke="#1d2a44" strokeWidth={5} />
              <circle cx={p.x} cy={p.y} r={36} fill="none" stroke={cpu > 85 ? COLOR.error : cpu > 65 ? COLOR.warn : '#0a84ff'} strokeWidth={5} strokeLinecap="round" {...ARC(36, cpu / 100)} transform={`rotate(-90 ${p.x} ${p.y})`} />
              <circle cx={p.x} cy={p.y} r={28} fill="none" stroke="#1d2a44" strokeWidth={4} />
              <circle cx={p.x} cy={p.y} r={28} fill="none" stroke={mem > 85 ? COLOR.error : '#bf5af2'} strokeWidth={4} strokeLinecap="round" {...ARC(28, mem / 100)} transform={`rotate(-90 ${p.x} ${p.y})`} />
              <text x={p.x} y={p.y + 7} textAnchor="middle" fill="#fff" fontSize="20" fontWeight="700" fontFamily="-apple-system,system-ui,sans-serif">{h.vm_count}</text>
              <circle cx={p.x + 31} cy={p.y - 31} r={6} fill={COLOR[t]} stroke="#0b1426" strokeWidth={2} className={t === 'ok' ? 'fci-pulse' : undefined} />
              <text x={p.x} y={p.y + 62} textAnchor="middle" fill="#fff" fontSize="13" fontWeight="600" fontFamily="-apple-system,system-ui,sans-serif">{h.hostname.slice(0, 22)}</text>
              <text x={p.x} y={p.y + 77} textAnchor="middle" fill="#8ec5ff" fontSize="11.5" fontFamily="SFMono-Regular,Menlo,Consolas,monospace">{h.address || '—'}</text>
              <text x={p.x} y={p.y + 91} textAnchor="middle" fill="#8e8e93" fontSize="10" fontFamily="-apple-system,system-ui,sans-serif">CPU {cpu}% · MEM {mem}% · {h.maintenance_mode ? 'maintenance' : h.state}</text>
            </g>
          )
        })}

        <g fontFamily="-apple-system,system-ui,sans-serif" fontSize="10.5" fill="#a1a1a6">
          <circle cx={22} cy={H - 40} r={4} fill="#0a84ff" /><text x={32} y={H - 36}>CPU ring</text>
          <circle cx={92} cy={H - 40} r={4} fill="#bf5af2" /><text x={102} y={H - 36}>memory ring</text>
          <circle cx={190} cy={H - 40} r={3} fill="#64d2ff" /><text x={198} y={H - 36}>each dot is a VM</text>
          <circle cx={22} cy={H - 20} r={4} fill={COLOR.ok} /><text x={32} y={H - 16}>online</text>
          <circle cx={82} cy={H - 20} r={4} fill={COLOR.warn} /><text x={92} y={H - 16}>maintenance</text>
          <circle cx={170} cy={H - 20} r={4} fill={COLOR.error} /><text x={180} y={H - 16}>offline</text>
        </g>
        <style>{`
          .fci-ring{transform-box:fill-box;transform-origin:center;animation:fci-rot 24s linear infinite}
          .fci-orbit{animation:fci-rot 22s linear infinite}
          .fci-pulse{animation:fci-pulse 1.6s ease-in-out infinite}
          .fci-attn{transform-box:fill-box;transform-origin:center;animation:fci-attn 1.8s ease-out infinite}
          @keyframes fci-attn{0%{opacity:.9;transform:scale(.92)}100%{opacity:0;transform:scale(1.25)}}
          @keyframes fci-rot{to{transform:rotate(360deg)}}
          @keyframes fci-pulse{50%{opacity:.35}}
          @media (prefers-reduced-motion:reduce){.fci-ring,.fci-orbit,.fci-pulse,.fci-attn{animation:none}}
        `}</style>
      </svg>

      {!showDetail ? null : sel ? (
        <div data-testid="cloud-detail" className="rounded-xl border border-white/10 bg-[var(--apple-surface)] p-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-4 text-sm">
          <div><div className="text-xs text-[var(--text-muted)]">Machine</div><div className="font-semibold text-[var(--text-primary)]">{sel.hostname}</div></div>
          <div><div className="text-xs text-[var(--text-muted)]">IP address</div><div className="font-mono text-[var(--text-primary)]">{sel.address || '—'}</div></div>
          <div><div className="text-xs text-[var(--text-muted)]">Agent</div><div className="font-mono text-[var(--text-primary)]">{sel.agent_grpc_addr || '—'}</div></div>
          <div><div className="text-xs text-[var(--text-muted)]">State</div><div className="text-[var(--text-primary)]">{sel.maintenance_mode ? 'maintenance' : sel.state}</div></div>
          <div><div className="text-xs text-[var(--text-muted)]">VMs</div><div className="text-[var(--text-primary)]">{sel.vm_count}</div></div>
          <div><div className="text-xs text-[var(--text-muted)]">CPU / memory</div><div className="text-[var(--text-primary)]">{Math.round(sel.cpu_percent || 0)}% / {pct(sel.memory_used_mib, sel.memory_total_mib)}%</div></div>
          <div><div className="text-xs text-[var(--text-muted)]">Last heartbeat</div><div className="text-[var(--text-primary)]">{sel.last_heartbeat_at ?? '—'}</div></div>
          <div className="flex items-end"><Link to={`/platform/hosts/${sel.id}`} className="btn-secondary text-xs">Open machine</Link></div>
        </div>
      ) : (
        <p className="text-xs text-[var(--text-muted)] m-0">Hover or click a machine for its details.</p>
      )}
    </div>
  )
}
