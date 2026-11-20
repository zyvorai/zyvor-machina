// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useId } from 'react'
import { Link } from 'react-router'
import type { PlatformHost } from '../../api/platform'

export type MapHost = Pick<PlatformHost, 'id' | 'hostname' | 'state' | 'vm_count' | 'cpu_percent'> & { maintenance_mode?: boolean; address?: string }

type Props = {
  hosts: MapHost[]
  /** A host that is joining right now: drawn as a pulsing node. */
  joining?: { label: string; failed?: boolean } | null
  /** Highlight this host id (e.g. the one that just joined). */
  highlightId?: string | null
  height?: number
}

export function hostTone(h: Pick<PlatformHost, 'state'> & { maintenance_mode?: boolean }): 'ok' | 'warn' | 'error' | 'idle' {
  if (h.maintenance_mode) return 'warn'
  if (h.state === 'online') return 'ok'
  if (h.state === 'offline' || h.state === 'error') return 'error'
  return 'idle'
}

const COLOR = { ok: '#30d158', warn: '#ffd60a', error: '#ff453a', idle: '#8e8e93' }
const W = 720
const H = 380
const CX = W / 2
const CY = H / 2

/** Positions on an ellipse around the controller, starting at the top and going clockwise. */
export function ringPositions(n: number): { x: number; y: number }[] {
  const rx = 275
  const ry = 135
  return Array.from({ length: n }, (_, i) => {
    const a = -Math.PI / 2 + (2 * Math.PI * i) / Math.max(n, 1)
    return { x: Math.round(CX + rx * Math.cos(a)), y: Math.round(CY + ry * Math.sin(a)) }
  })
}

/** The machines that make up the cloud: the controller in the middle, every host around it. */
export default function FleetCloudMap({ hosts, joining, highlightId, height = 340 }: Props) {
  const uid = useId().replace(/:/g, '')
  const slots = hosts.length + (joining ? 1 : 0)
  const pos = ringPositions(slots)
  const compact = slots > 8
  const online = hosts.filter((h) => hostTone(h) === 'ok').length
  const vms = hosts.reduce((a, h) => a + (h.vm_count || 0), 0)

  return (
    <figure data-testid="fleet-cloud-map" className="m-0" aria-label={`Fleet map: ${online} of ${hosts.length} hosts online, ${vms} VMs`}>
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" style={{ height, display: 'block', borderRadius: 12, background: 'radial-gradient(ellipse at center,#14213a 0%,#0a0f1c 70%)' }} role="img">
        <defs>
          <radialGradient id={`${uid}hub`} cx="50%" cy="40%" r="70%">
            <stop offset="0%" stopColor="#4aa3ff" />
            <stop offset="100%" stopColor="#0a4fbf" />
          </radialGradient>
          <filter id={`${uid}glow`} x="-50%" y="-50%" width="200%" height="200%">
            <feGaussianBlur stdDeviation="4" result="b" />
            <feMerge><feMergeNode in="b" /><feMergeNode in="SourceGraphic" /></feMerge>
          </filter>
        </defs>

        {pos.map((p, i) => {
          const isJoin = joining && i === hosts.length
          const h = isJoin ? null : hosts[i]
          const tone = isJoin ? (joining?.failed ? 'error' : 'warn') : hostTone(h!)
          return (
            <line key={`l${i}`} x1={CX} y1={CY} x2={p.x} y2={p.y} stroke={COLOR[tone]} strokeOpacity={tone === 'ok' ? 0.55 : 0.4} strokeWidth={1.5}
              strokeDasharray={tone === 'ok' ? '2 7' : '5 6'} className={tone === 'ok' || isJoin ? 'fcm-flow' : undefined} />
          )
        })}

        <g filter={`url(#${uid}glow)`}>
          <circle cx={CX} cy={CY} r={44} fill={`url(#${uid}hub)`} />
        </g>
        <text x={CX} y={CY - 2} textAnchor="middle" fill="#fff" fontSize="13" fontWeight="600" fontFamily="-apple-system,system-ui,sans-serif">Controller</text>
        <text x={CX} y={CY + 14} textAnchor="middle" fill="#cfe4ff" fontSize="10.5" fontFamily="-apple-system,system-ui,sans-serif">{hosts.length} host{hosts.length === 1 ? '' : 's'} · {vms} VM{vms === 1 ? '' : 's'}</text>

        {hosts.map((h, i) => {
          const p = pos[i]
          const tone = hostTone(h)
          const hl = highlightId === h.id
          const body = compact ? (
            <g>
              <circle cx={p.x} cy={p.y} r={15} fill="#0d1526" stroke={COLOR[tone]} strokeWidth={hl ? 3 : 2} />
              <text x={p.x} y={p.y + 4} textAnchor="middle" fill="#fff" fontSize="11" fontFamily="-apple-system,system-ui,sans-serif">{h.vm_count}</text>
              <text x={p.x} y={p.y + 31} textAnchor="middle" fill="#d1d1d6" fontSize="10.5" fontFamily="-apple-system,system-ui,sans-serif">{h.hostname.slice(0, 16)}</text>
              {h.address && <text x={p.x} y={p.y + 43} textAnchor="middle" fill="#8ec5ff" fontSize="9.5" fontFamily="SFMono-Regular,Menlo,Consolas,monospace">{h.address}</text>}
            </g>
          ) : (
            <g>
              <rect x={p.x - 78} y={p.y - 28} width={156} height={56} rx={12} fill="#0d1526" stroke={COLOR[tone]} strokeWidth={hl ? 3 : 1.5} />
              <circle cx={p.x - 62} cy={p.y - 11} r={4.5} fill={COLOR[tone]} className={tone === 'ok' ? 'fcm-pulse' : undefined} />
              <text x={p.x - 52} y={p.y - 7} fill="#fff" fontSize="12.5" fontWeight="600" fontFamily="-apple-system,system-ui,sans-serif">{h.hostname.slice(0, 18)}</text>
              <text x={p.x - 66} y={p.y + 8} fill="#8ec5ff" fontSize="10.5" fontFamily="SFMono-Regular,Menlo,Consolas,monospace">{h.address || h.state} · {h.vm_count} VM{h.vm_count === 1 ? '' : 's'}</text>
              <rect x={p.x - 66} y={p.y + 17} width={132} height={3} rx={1.5} fill="#2c2c2e" />
              <rect x={p.x - 66} y={p.y + 17} width={Math.max(2, Math.min(100, h.cpu_percent || 0)) * 1.32} height={3} rx={1.5} fill={COLOR[tone]} />
            </g>
          )
          return (
            <Link key={h.id} to={`/platform/hosts/${h.id}`} aria-label={`Host ${h.hostname}, ${h.state}`}>
              <g data-testid={`fleet-map-host-${h.hostname}`} className={hl ? 'fcm-arrive' : undefined} style={{ cursor: 'pointer' }}>{body}</g>
            </Link>
          )
        })}

        {joining && (() => {
          const p = pos[hosts.length]
          const c = joining.failed ? COLOR.error : COLOR.warn
          return (
            <g data-testid="fleet-map-joining">
              <circle cx={p.x} cy={p.y} r={22} fill="none" stroke={c} strokeWidth={2} strokeDasharray="4 5" className="fcm-spin" />
              <circle cx={p.x} cy={p.y} r={7} fill={c} className="fcm-pulse" />
              <text x={p.x} y={p.y + 40} textAnchor="middle" fill={c} fontSize="11" fontFamily="-apple-system,system-ui,sans-serif">{joining.label.slice(0, 28)}</text>
            </g>
          )
        })()}
      </svg>
      <style>{`
        .fcm-flow{animation:fcm-dash 1.4s linear infinite}
        .fcm-pulse{animation:fcm-pulse 1.6s ease-in-out infinite}
        .fcm-spin{transform-box:fill-box;transform-origin:center;animation:fcm-rot 5s linear infinite}
        .fcm-arrive{animation:fcm-pop .6s ease-out}
        @keyframes fcm-dash{to{stroke-dashoffset:-18}}
        @keyframes fcm-pulse{50%{opacity:.35}}
        @keyframes fcm-rot{to{transform:rotate(360deg)}}
        @keyframes fcm-pop{from{opacity:0;transform:scale(.85)}}
        @media (prefers-reduced-motion:reduce){.fcm-flow,.fcm-pulse,.fcm-spin,.fcm-arrive{animation:none}}
      `}</style>
    </figure>
  )
}
