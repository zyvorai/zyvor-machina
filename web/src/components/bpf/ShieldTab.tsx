// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// XDP DDoS shield on the uplink: per-source per-class PPS buckets plus
// allow / deny CIDRs. Drops only in enforce with the enforcement lease live.

import { useCallback, useEffect, useState } from 'react'
import { MacGlassPanel, MacSegmentedControl, MacToggle } from '../platform/mac/PlatformMacUi'
import { getBpfShield, setBpfShield, type BpfShieldConfig } from '../../api/bpf'
import { statusPillClasses } from '../../utils/semanticColors'
import { Empty, Field, Metrics, fmtBytes, splitList, useBpfAction, useBpfLoad } from './shared'

const RATES = [
  ['syn_pps', 'SYN pps'],
  ['udp_pps', 'UDP pps'],
  ['icmp_pps', 'ICMP pps'],
  ['other_pps', 'Other pps'],
] as const

export default function ShieldTab() {
  const action = useBpfAction()
  const { data: st, setData, error } = useBpfLoad(useCallback(() => getBpfShield(), []))
  const [c, setC] = useState<BpfShieldConfig | null>(null)
  useEffect(() => { if (st && !c) setC(st.config) }, [st, c])

  if (error) return <MacGlassPanel title="XDP shield"><Empty>{error}</Empty></MacGlassPanel>
  if (!st || !c) return null
  const save = () =>
    void action(
      setBpfShield({ ...c, protected: splitList(c.protected.join('\n')), allow: splitList(c.allow.join('\n')), deny: splitList(c.deny.join('\n')) }),
      `Shield ${c.mode}`,
    ).then((r) => { if (r) { setData(r); setC(r.config) } })
  const list = (k: 'protected' | 'allow' | 'deny', label: string, ph: string) => (
    <Field label={label} htmlFor={`sh-${k}`}>
      <textarea
        id={`sh-${k}`}
        className="input text-sm font-mono h-20"
        placeholder={ph}
        value={c[k].join('\n')}
        onChange={(e) => setC({ ...c, [k]: e.target.value.split('\n') })}
      />
    </Field>
  )
  return (
    <>
      <Metrics
        items={[
          { label: 'Checked', value: st.stats.checked.toLocaleString() },
          { label: 'Dropped', value: `${st.stats.dropped.toLocaleString()} · ${fmtBytes(st.stats.dropped_bytes)}` },
          { label: 'Audited', value: st.stats.audited.toLocaleString() },
          { label: 'Denied CIDR', value: st.stats.denied.toLocaleString() },
          { label: 'Tracked sources', value: st.tracked_sources.toLocaleString() },
        ]}
      />
      <MacGlassPanel
        title="XDP DDoS shield"
        subtitle="Runs first in the uplink XDP dispatcher (shared with the NodePort fast path). Enforce drops only while the enforcement lease is live."
        action={
          <span className={statusPillClasses(st.enforcing ? 'warn' : st.attached ? 'info' : 'neutral')}>
            {st.enforcing ? `Enforcing on ${st.attached}` : st.attached ? `${st.config.mode} on ${st.attached}` : 'Off'}
          </span>
        }
      >
        <div className="space-y-4">
          <div className="flex flex-wrap items-end gap-4">
            <MacSegmentedControl
              label="Mode"
              options={[{ value: 'off', label: 'Off' }, { value: 'audit', label: 'Audit' }, { value: 'enforce', label: 'Enforce' }]}
              value={c.mode}
              onChange={(mode) => setC({ ...c, mode })}
            />
            <Field label="Uplink interface" htmlFor="sh-iface">
              <input id="sh-iface" className="input text-sm font-mono w-40" placeholder="eth0" value={c.iface} onChange={(e) => setC({ ...c, iface: e.target.value.trim() })} />
            </Field>
            <Field label="Burst (s)" htmlFor="sh-burst">
              <input id="sh-burst" type="number" min={1} max={60} className="input text-sm w-20" value={c.burst_secs} onChange={(e) => setC({ ...c, burst_secs: Number(e.target.value) || 1 })} />
            </Field>
            <MacToggle label="Protect every destination" checked={c.protect_all} onChange={(protect_all) => setC({ ...c, protect_all })} />
          </div>
          <div className="flex flex-wrap gap-3">
            {RATES.map(([k, label]) => (
              <Field key={k} label={`${label} / source (0 = unlimited)`} htmlFor={`sh-${k}`}>
                <input id={`sh-${k}`} type="number" min={0} className="input text-sm w-28" value={c[k]} onChange={(e) => setC({ ...c, [k]: Math.max(0, Number(e.target.value) || 0) })} />
              </Field>
            ))}
          </div>
          <div className="grid gap-3 sm:grid-cols-3">
            {!c.protect_all && list('protected', 'Protected addresses', '203.0.113.10')}
            {list('allow', 'Allow CIDRs (never limited)', '10.0.0.0/8')}
            {list('deny', 'Deny CIDRs (always dropped)', '198.51.100.0/24')}
          </div>
          <button type="button" className="btn-primary text-sm" onClick={save}>Apply shield</button>
        </div>
      </MacGlassPanel>
      <MacGlassPanel title="Top over-rate sources" subtitle="Per class, by packets over the bucket">
        {st.sources.length === 0 ? (
          <Empty>No source over its rate.</Empty>
        ) : (
          <ul className="text-xs font-mono space-y-1">
            {st.sources.map((s) => (
              <li key={`${s.addr}-${s.class}`} className="flex justify-between gap-2">
                <span>{s.addr} · {s.class}</span>
                <span className="text-[var(--text-muted)]">{s.hits.toLocaleString()}</span>
              </li>
            ))}
          </ul>
        )}
        <p className="text-xs text-[var(--text-muted)] mt-3">
          Limited — SYN {st.stats.syn_limited} · UDP {st.stats.udp_limited} · ICMP {st.stats.icmp_limited} · other {st.stats.other_limited} · malformed {st.stats.malformed}
        </p>
      </MacGlassPanel>
    </>
  )
}
