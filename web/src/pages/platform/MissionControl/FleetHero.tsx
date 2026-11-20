// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { MissionControlFleetState } from './useMissionControlFleet'

const MAX_HOSTS = 5
const W = 960

/** Seconds a dot takes to cross a link: busier fleets move faster (same idea as Netra's datapath hero). */
function flowSeconds(activity: number): number {
  return Math.max(1.2, 6 - Math.log10(activity + 1) * 1.1)
}

/**
 * Signature visual: the controller on the left, each host as a node, every VM as a dot on its host.
 * Dots travel along the controller-to-host links; offline hosts get a dashed red link and no traffic.
 */
export default function FleetHero({ state }: { state: MissionControlFleetState }) {
  const { hosts, vms, running, loading, error } = state
  if (loading || error || hosts.length === 0) return null

  const shown = hosts.slice(0, MAX_HOSTS)
  const extra = hosts.length - shown.length
  const H = Math.max(200, 64 + shown.length * 74 + (extra > 0 ? 18 : 0))
  const rowH = (H - 40 - (extra > 0 ? 18 : 0)) / Math.max(shown.length, 1)
  const cx = 130
  const hx = 480
  const vmCols = Math.min(9, Math.max(1, ...shown.map((h) => vms.filter((v) => v.host_id === h.id).length)))
  const left = cx - 70
  const right = hx + 100 + (vmCols - 1) * 20 + 6 + (extra > 0 || vms.length > 18 ? 34 : 0)
  const ox = (W - (right - left)) / 2 - left
  const dur = flowSeconds(running)

  return (
    <section className="apple-section apple-section--tight" aria-label="Fleet topology">
      <div className="nl-stage">
        <svg className="nl-stage-svg" viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`Controller connected to ${hosts.length} hosts running ${running} VMs`}>
          <defs>
            <linearGradient id="nl-link" gradientUnits="userSpaceOnUse" x1={cx + 70} y1="0" x2={hx - 70} y2="0">
              <stop offset="0" stopColor="var(--apple-blue, #0071e3)" stopOpacity="0.15" />
              <stop offset="1" stopColor="var(--apple-blue, #0071e3)" stopOpacity="0.55" />
            </linearGradient>
          </defs>

          <g transform={`translate(${ox} 0)`}>
          <g>
            <rect x={cx - 70} y={H / 2 - 34} width={140} height={68} rx={16} className="nl-node" />
            <text x={cx} y={H / 2 - 4} textAnchor="middle" className="nl-node-title">Controller</text>
            <text x={cx} y={H / 2 + 16} textAnchor="middle" className="nl-node-sub">{hosts.length} host{hosts.length === 1 ? '' : 's'}</text>
          </g>

          {shown.map((h, i) => {
            const y = 20 + rowH * i + rowH / 2
            const online = h.state !== 'offline'
            const mine = vms.filter((v) => v.host_id === h.id)
            const path = `M ${cx + 70} ${H / 2} C ${cx + 200} ${H / 2}, ${hx - 140} ${y}, ${hx - 70} ${y}`
            return (
              <g key={h.id}>
                <path d={path} className={online ? 'nl-link' : 'nl-link is-down'} fill="none" />
                {online
                  ? [0, 1].map((k) => (
                      <circle key={k} r={3.5} className="nl-dot">
                        <animateMotion dur={`${dur}s`} begin={`-${(k * dur) / 2 + i * 0.35}s`} repeatCount="indefinite" path={path} />
                      </circle>
                    ))
                  : null}
                <rect x={hx - 70} y={y - 26} width={140} height={52} rx={14} className={online ? 'nl-node' : 'nl-node is-down'} />
                <text x={hx} y={y - 3} textAnchor="middle" className="nl-node-title">{h.hostname.length > 16 ? `${h.hostname.slice(0, 15)}…` : h.hostname}</text>
                <text x={hx} y={y + 14} textAnchor="middle" className="nl-node-sub">{online ? `${mine.length} VM${mine.length === 1 ? '' : 's'}` : 'offline'}</text>
                {mine.slice(0, 18).map((v, k) => (
                  <circle
                    key={v.id}
                    cx={hx + 100 + (k % 9) * 20}
                    cy={y - 8 + Math.floor(k / 9) * 18}
                    r={6}
                    className={v.observed_state === 'running' ? 'nl-vm is-running' : 'nl-vm'}
                  >
                    <title>{`${v.name} · ${v.observed_state}`}</title>
                  </circle>
                ))}
                {mine.length > 18 ? <text x={hx + 100 + 9 * 20 + 6} y={y + 4} className="nl-node-sub">+{mine.length - 18}</text> : null}
              </g>
            )
          })}
          {extra > 0 ? <text x={hx} y={H - 4} textAnchor="middle" className="nl-node-sub">+{extra} more host{extra === 1 ? '' : 's'}</text> : null}
          </g>
        </svg>
      </div>
    </section>
  )
}
