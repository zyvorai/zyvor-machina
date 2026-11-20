// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// What a draft policy set would have done to the last 7 days of traffic.

import { TahoeTableWrap } from '../platform/tahoe/TahoeListKit'
import { headRowCls, rowCls, thCls } from '../bpf/shared'
import type { ReplayChange, ReplayResult } from '../../api/vmNetpol'
import { statusPillClasses, statusToneClass } from '../../utils/semanticColors'

function Changes({ title, tone, items }: { title: string; tone: 'error' | 'warn'; items: ReplayChange[] }) {
  if (items.length === 0) return null
  return (
    <div className="space-y-1">
      <div className={`text-xs font-medium ${statusToneClass(tone)}`}>{title}</div>
      <TahoeTableWrap>
        <table className="w-full text-xs" aria-label={title}>
          <thead>
            <tr className={headRowCls}>
              <th scope="col" className={thCls}>Connection</th>
              <th scope="col" className={thCls}>Request</th>
              <th scope="col" className={thCls}>Flows</th>
              <th scope="col" className={thCls}>Now</th>
              <th scope="col" className={thCls}>With draft</th>
              <th scope="col" className="py-2">Last seen</th>
            </tr>
          </thead>
          <tbody>
            {items.map((c, i) => (
              <tr key={i} className={rowCls}>
                <td className="py-2 pr-2 font-mono">{c.src} → {c.dst} {c.proto.toLowerCase()}/{c.port}</td>
                <td className="py-2 pr-2 font-mono">{c.request ?? '—'}</td>
                <td className="py-2 pr-2">{c.flows}</td>
                <td className="py-2 pr-2">{c.before}</td>
                <td className="py-2 pr-2"><span className={statusPillClasses(tone)}>{c.after}</span></td>
                <td className="py-2 text-[var(--text-muted)]">{c.last_seen}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </TahoeTableWrap>
    </div>
  )
}

export default function ReplaySummary({ r }: { r: ReplayResult }) {
  const clean = r.would_break.length === 0
  return (
    <div className="space-y-3 text-xs">
      <div className={`text-sm font-semibold ${statusToneClass(clean ? 'ok' : 'error')}`}>
        {clean
          ? `Safe: none of the ${r.evaluated} observed connections would be blocked.`
          : `${r.would_break.length} observed connection(s) (${r.flows_breaking} flows) would be blocked.`}
      </div>
      <div className="text-[var(--text-muted)]">
        {r.evaluated} distinct connections replayed from the flow history · {r.unchanged} unchanged · {r.would_allow.length} newly allowed
        {r.not_evaluated ? ` · ${r.not_evaluated} skipped (endpoints no longer resolvable)` : ''}
      </div>
      <Changes title="Would break" tone="error" items={r.would_break} />
      <Changes title="Would newly allow" tone="warn" items={r.would_allow} />
    </div>
  )
}
