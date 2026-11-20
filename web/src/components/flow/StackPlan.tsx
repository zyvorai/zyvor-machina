// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { AlertTriangle, CheckCircle2 } from 'lucide-react'
import type { NativeStack, StackPlan, StackPolicy } from '../../api/stacks'

const PEERS = new Set(['internet', 'world', 'fleet', 'host', 'any'])

const gib = (mib: number) => `${Math.round((mib / 1024) * 10) / 10} GiB`
const usd = (n: number) => `$${n.toFixed(2)}`

/** Groups laid out left to right by how far they are from outside traffic. */
export function PolicyGraph({ policies, groups }: { policies: StackPolicy[]; groups: string[] }) {
  if (policies.length === 0) {
    return <p className="text-sm text-[var(--text-muted)]">No policies: every group accepts all traffic.</p>
  }
  const nodes = new Set<string>(groups)
  for (const p of policies) {
    nodes.add(p.from)
    nodes.add(p.to)
  }
  const depth = new Map<string, number>()
  for (const n of nodes) depth.set(n, groups.includes(n) ? 1 : 0)
  for (let i = 0; i < nodes.size; i++) {
    for (const p of policies) {
      if (p.from === p.to || !groups.includes(p.to)) continue
      const d = (depth.get(p.from) ?? 0) + 1
      if (d > (depth.get(p.to) ?? 0) && d <= nodes.size) depth.set(p.to, d)
    }
  }
  const columns: string[][] = []
  for (const n of [...nodes].sort()) {
    const d = depth.get(n) ?? 0
    ;(columns[d] ??= []).push(n)
  }
  const cols = columns.filter(Boolean)
  const W = 132
  const H = 34
  const GX = 64
  const GY = 18
  const pos = new Map<string, { x: number; y: number }>()
  cols.forEach((col, ci) => col.forEach((n, ri) => pos.set(n, { x: ci * (W + GX), y: ri * (H + GY) })))
  const width = cols.length * (W + GX) - GX
  const height = Math.max(...cols.map((c) => c.length)) * (H + GY) - GY
  return (
    <svg
      role="img"
      aria-label="Policy graph"
      viewBox={`-4 -4 ${width + 8} ${height + 8}`}
      className="w-full max-w-3xl"
      style={{ maxHeight: 320 }}
    >
      <defs>
        <marker id="stack-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
          <path d="M 0 0 L 10 5 L 0 10 z" fill="var(--accent)" />
        </marker>
      </defs>
      {policies.map((p, i) => {
        const a = pos.get(p.from)
        const b = pos.get(p.to)
        if (!a || !b) return null
        const x1 = a.x + W
        const y1 = a.y + H / 2
        const x2 = b.x
        const y2 = b.y + H / 2
        const back = x2 <= x1
        const d = back
          ? `M ${a.x + W / 2} ${a.y + H} C ${a.x + W / 2} ${a.y + H + 30}, ${b.x + W / 2} ${b.y + H + 30}, ${b.x + W / 2} ${b.y + H}`
          : `M ${x1} ${y1} C ${x1 + GX / 2} ${y1}, ${x2 - GX / 2} ${y2}, ${x2} ${y2}`
        const label = p.ports?.length ? p.ports.join(',') : 'all'
        return (
          <g key={`${p.from}-${p.to}-${i}`}>
            <path d={d} fill="none" stroke="var(--accent)" strokeWidth={1.4} markerEnd="url(#stack-arrow)" />
            <text
              x={back ? (a.x + b.x + W) / 2 : (x1 + x2) / 2}
              y={back ? Math.max(a.y, b.y) + H + 26 : (y1 + y2) / 2 - 4}
              textAnchor="middle"
              fontSize={10}
              fill="var(--text-secondary)"
            >
              {label}
            </text>
          </g>
        )
      })}
      {[...pos.entries()].map(([n, { x, y }]) => {
        const external = !groups.includes(n)
        return (
          <g key={n}>
            <rect
              x={x}
              y={y}
              width={W}
              height={H}
              rx={10}
              fill={external ? 'var(--apple-surface)' : 'color-mix(in srgb, var(--accent) 12%, transparent)'}
              stroke={external ? 'var(--apple-hairline)' : 'var(--accent)'}
              strokeDasharray={external && PEERS.has(n) ? '4 3' : undefined}
            />
            <text x={x + W / 2} y={y + H / 2 + 4} textAnchor="middle" fontSize={12} fill="var(--text-primary)">
              {n}
            </text>
          </g>
        )
      })}
    </svg>
  )
}

export default function StackPlanPreview({ plan }: { plan: StackPlan }) {
  const groups = [...new Set(plan.vms.filter((v) => v.group).map((v) => v.group))]
  const check = (ok: boolean, text: string, label: string) => (
    <li key={`${label}:${text}`} className="flex items-start gap-2 text-sm" aria-label={label}>
      {ok ? (
        <CheckCircle2 className="w-4 h-4 mt-0.5 text-green-600 shrink-0" />
      ) : (
        <AlertTriangle className="w-4 h-4 mt-0.5 text-amber-600 shrink-0" />
      )}
      <span>{text}</span>
    </li>
  )
  return (
    <section aria-label="Stack plan" className="space-y-4">
      <div className="flex flex-wrap gap-2 text-sm">
        <span className="px-2.5 py-1 rounded-full bg-[var(--apple-surface)] border border-[var(--apple-hairline)]">
          {plan.totals.vms} VMs
        </span>
        <span className="px-2.5 py-1 rounded-full bg-[var(--apple-surface)] border border-[var(--apple-hairline)]">
          {plan.totals.vcpus} vCPU
        </span>
        <span className="px-2.5 py-1 rounded-full bg-[var(--apple-surface)] border border-[var(--apple-hairline)]">
          {gib(plan.totals.memory_mib)} memory
        </span>
        <span className="px-2.5 py-1 rounded-full bg-[var(--apple-surface)] border border-[var(--apple-hairline)]">
          {plan.totals.storage_gib} GiB disk
        </span>
        <span
          className="px-2.5 py-1 rounded-full bg-[var(--accent)]/15 text-[var(--link)] font-medium"
          aria-label="Monthly cost"
        >
          {usd(plan.monthly_usd)}/month
        </span>
      </div>

      <ul className="space-y-1">
        {plan.errors.map((e) => check(false, e, 'Template problem'))}
        {check(plan.quota.ok, plan.quota.detail, 'Quota')}
        {check(plan.placement.ok, plan.placement.detail, 'Placement')}
        {plan.replay_summary &&
          check(/^would break 0 /.test(plan.replay_summary), `Replay of recorded traffic: ${plan.replay_summary}.`, 'Replay')}
      </ul>

      <div className="overflow-x-auto rounded-xl border border-[var(--apple-hairline)]">
        <table className="apple-table" aria-label="Planned VMs">
          <thead>
            <tr>
              <th scope="col">VM</th>
              <th scope="col">Group</th>
              <th scope="col">Size</th>
              <th scope="col">Host</th>
              <th scope="col">Per month</th>
              <th scope="col">Change</th>
            </tr>
          </thead>
          <tbody>
            {plan.vms.map((v) => (
              <tr key={v.name}>
                <td className="font-mono text-xs">{v.name}</td>
                <td>{v.group || '—'}</td>
                <td className="text-[var(--text-muted)]">
                  {v.action === 'delete' ? '—' : `${v.vcpus} vCPU · ${gib(v.memory_mib)} · ${v.disk_gib} GiB${v.image ? ` · ${v.image}` : ''}`}
                </td>
                <td className="text-[var(--text-muted)]">{v.host ?? '—'}</td>
                <td>{v.action === 'delete' ? '—' : usd(v.monthly_usd)}</td>
                <td>
                  <span className={v.action === 'delete' ? 'text-red-600' : v.action === 'create' ? 'text-green-700' : v.action === 'resize' ? 'text-amber-700' : 'text-[var(--text-muted)]'}>
                    {v.action}
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="space-y-2">
        <h3 className="text-sm font-medium text-[var(--text-secondary)]">Who talks to whom</h3>
        <PolicyGraph policies={plan.policies} groups={groups} />
        {plan.policy_yaml && (
          <details>
            <summary className="text-xs text-[var(--text-muted)] cursor-pointer">Generated network policies</summary>
            <pre className="mt-2 font-mono text-xs input-field overflow-x-auto">{plan.policy_yaml}</pre>
          </details>
        )}
      </div>
    </section>
  )
}

export function DriftBadge({ stack }: { stack: NativeStack }) {
  const v2 = !!(stack.template_json.instances?.length || stack.template_json.policies?.length)
  if (!v2 || !stack.checked_at) return <span className="text-[var(--text-muted)]">—</span>
  const open = stack.drift_json?.open ?? 0
  return open === 0 ? (
    <span className="px-2 py-0.5 rounded-full text-xs bg-green-600/10 text-green-700" aria-label="Drift">In sync</span>
  ) : (
    <span className="px-2 py-0.5 rounded-full text-xs bg-amber-500/15 text-amber-700" aria-label="Drift">
      {open} drifted
    </span>
  )
}
