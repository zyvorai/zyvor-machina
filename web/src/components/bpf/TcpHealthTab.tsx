// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// TCP connect latency / failures and per-peer pressure from mn_sockops, plus
// the ICMP error histogram from the tap datapath.

import { useCallback } from 'react'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import { TahoeTableWrap } from '../platform/tahoe/TahoeListKit'
import { getBpfHealth, getBpfIcmpErrors } from '../../api/bpf'
import { statusToneClass } from '../../utils/semanticColors'
import { Empty, fmtBytes, fmtUs, headRowCls, rowCls, thCls, useBpfLoad } from './shared'

export default function TcpHealthTab() {
  const load = useCallback(() => Promise.all([getBpfHealth(), getBpfIcmpErrors()]), [])
  const { data, error } = useBpfLoad(load)
  if (error) return <MacGlassPanel title="TCP health"><Empty>{error}</Empty></MacGlassPanel>
  if (!data) return null
  const [health, icmp] = data
  const connect = [...(health.connect ?? [])].sort((a, b) => b.count - a.count)
  const pressure = [...(health.pressure ?? [])].sort((a, b) => b.total_retrans - a.total_retrans)
  return (
    <>
      <MacGlassPanel
        title="Connect latency"
        subtitle={health.sockops ? `mn_sockops on ${health.sockops} · SYN → established per destination` : 'mn_sockops is not attached (enable TCP telemetry)'}
      >
        {connect.length === 0 ? (
          <Empty>No outbound connects recorded.</Empty>
        ) : (
          <TahoeTableWrap>
            <table className="w-full text-xs" aria-label="TCP connect latency">
              <thead>
                <tr className={headRowCls}>
                  <th scope="col" className={thCls}>Destination</th>
                  <th scope="col" className={thCls}>Connects</th>
                  <th scope="col" className={thCls}>Failures</th>
                  <th scope="col" className={thCls}>Avg</th>
                  <th scope="col" className={thCls}>p90 ≤</th>
                  <th scope="col" className="py-2">Max</th>
                </tr>
              </thead>
              <tbody>
                {connect.slice(0, 50).map((c) => (
                  <tr key={`${c.addr}:${c.port}`} className={rowCls}>
                    <td className="py-2 pr-2 font-mono">{c.addr}:{c.port}</td>
                    <td className="py-2 pr-2">{c.count}</td>
                    <td className={`py-2 pr-2 ${c.failures > 0 ? statusToneClass('warn') : ''}`}>{c.failures}</td>
                    <td className="py-2 pr-2">{fmtUs(c.avg_us)}</td>
                    <td className="py-2 pr-2">{c.p90_le_us != null ? fmtUs(c.p90_le_us) : '> 1 s'}</td>
                    <td className="py-2">{fmtUs(c.max_us)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        )}
      </MacGlassPanel>

      <MacGlassPanel title="Peer pressure" subtitle="Latest RTT / congestion window / retransmits per remote address">
        {pressure.length === 0 ? (
          <Empty>No established connections sampled.</Empty>
        ) : (
          <TahoeTableWrap>
            <table className="w-full text-xs" aria-label="TCP peer pressure">
              <thead>
                <tr className={headRowCls}>
                  <th scope="col" className={thCls}>Peer</th>
                  <th scope="col" className={thCls}>sRTT</th>
                  <th scope="col" className={thCls}>cwnd / ssthresh</th>
                  <th scope="col" className={thCls}>Retransmits</th>
                  <th scope="col" className={thCls}>Delivery rate</th>
                  <th scope="col" className="py-2">Age</th>
                </tr>
              </thead>
              <tbody>
                {pressure.slice(0, 50).map((p) => (
                  <tr key={p.addr} className={rowCls}>
                    <td className="py-2 pr-2 font-mono">{p.addr}</td>
                    <td className="py-2 pr-2">{fmtUs(p.srtt_us)}</td>
                    <td className="py-2 pr-2">{p.cwnd} / {p.ssthresh >= 0x7fffffff ? '∞' : p.ssthresh}</td>
                    <td className={`py-2 pr-2 ${p.total_retrans > 0 ? statusToneClass('warn') : ''}`}>{p.total_retrans}</td>
                    <td className="py-2 pr-2">{p.delivery_rate_bps ? `${fmtBytes(p.delivery_rate_bps)}/s` : '—'}</td>
                    <td className="py-2 text-[var(--text-muted)]">{p.age_secs}s</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        )}
      </MacGlassPanel>

      <MacGlassPanel title="ICMP errors" subtitle="Unreachable / time exceeded / packet too big seen on attached interfaces">
        {icmp.length === 0 ? (
          <Empty>No ICMP errors seen.</Empty>
        ) : (
          <ul className="text-xs font-mono space-y-1">
            {icmp.map((e) => (
              <li key={`${e.iface}-${e.kind}-${e.code}-${e.family}-${e.direction}`} className="flex justify-between gap-2">
                <span>{e.vm ?? e.iface} · {e.family} {e.kind} code {e.code} · {e.direction.replace('_', ' ')}</span>
                <span className="text-[var(--text-muted)]">{e.count}</span>
              </li>
            ))}
          </ul>
        )}
      </MacGlassPanel>
    </>
  )
}
