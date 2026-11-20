// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// JA3/JA4 ClientHello fingerprints (mn_tlsfp + tap L7) and HTTP metadata
// from libssl uprobes (plaintext bodies never leave bpfd).

import { useCallback, useEffect, useState } from 'react'
import { MacGlassPanel, MacToggle } from '../platform/mac/PlatformMacUi'
import { TahoeTableWrap } from '../platform/tahoe/TahoeListKit'
import {
  getBpfSslEvents,
  getBpfTls,
  getBpfTlsFingerprints,
  setBpfTls,
  workloadLabel,
  type BpfSslRecord,
  type BpfTlsConfig,
} from '../../api/bpf'
import { formatUserError } from '../../utils/apiError'
import { Empty, Field, Metrics, headRowCls, rowCls, splitList, thCls, useBpfAction, useBpfLoad } from './shared'

export default function TlsTab() {
  const action = useBpfAction()
  const status = useBpfLoad(useCallback(() => getBpfTls(), []))
  const fps = useBpfLoad(useCallback(() => getBpfTlsFingerprints(300), []))
  const [ssl, setSsl] = useState<BpfSslRecord[] | null>(null)
  const [sslError, setSslError] = useState<string | null>(null)
  const [c, setC] = useState<BpfTlsConfig | null>(null)
  const [comms, setComms] = useState('')

  useEffect(() => {
    getBpfSslEvents(200).then(setSsl, (e: unknown) => setSslError(formatUserError(e)))
  }, [])
  useEffect(() => {
    if (status.data && !c) {
      setC(status.data.config)
      setComms(status.data.config.ssl_comms.join(', '))
    }
  }, [status.data, c])

  const st = status.data
  const byJa4 = new Map<string, { count: number; snis: Set<string> }>()
  for (const f of fps.data ?? []) {
    const e = byJa4.get(f.ja4) ?? { count: 0, snis: new Set<string>() }
    e.count += 1
    if (f.sni) e.snis.add(f.sni)
    byJa4.set(f.ja4, e)
  }
  const top = [...byJa4.entries()].sort((a, b) => b[1].count - a[1].count).slice(0, 10)

  return (
    <>
      {st && (
        <Metrics
          items={[
            { label: 'Fingerprints', value: st.fingerprints_seen.toLocaleString() },
            { label: 'Distinct JA4', value: byJa4.size },
            { label: 'SSL events', value: st.ssl_events_seen.toLocaleString() },
            { label: 'libssl hooked', value: st.ssl_libraries.length },
          ]}
        />
      )}
      <MacGlassPanel title="Sampling" subtitle="Both samplers are opt-in and rate-limited host-wide">
        {status.error ? (
          <Empty>{status.error}</Empty>
        ) : c && st ? (
          <div className="space-y-3">
            <div className="flex flex-wrap items-end gap-4">
              <MacToggle label="ClientHello fingerprints (JA3 / JA4)" checked={c.fingerprints} onChange={(fingerprints) => setC({ ...c, fingerprints })} />
              <Field label="Samples / s" htmlFor="tls-fp-rate">
                <input id="tls-fp-rate" type="number" min={1} className="input text-sm w-24" value={c.fingerprint_rate} onChange={(e) => setC({ ...c, fingerprint_rate: Math.max(1, Number(e.target.value) || 1) })} />
              </Field>
            </div>
            <div className="flex flex-wrap items-end gap-4">
              <MacToggle label="OpenSSL uprobes (HTTP method / host / path)" checked={c.ssl_uprobes} onChange={(ssl_uprobes) => setC({ ...c, ssl_uprobes })} />
              <Field label="Processes (comm, comma separated)" htmlFor="tls-comms">
                <input id="tls-comms" className="input text-sm font-mono w-56" placeholder="curl, nginx" value={comms} onChange={(e) => setComms(e.target.value)} />
              </Field>
              <MacToggle label="All processes" checked={c.ssl_all_processes} onChange={(ssl_all_processes) => setC({ ...c, ssl_all_processes })} />
            </div>
            <button
              type="button"
              className="btn-primary text-sm"
              onClick={() =>
                void action(setBpfTls({ ...c, ssl_comms: splitList(comms) }), 'TLS sampling updated').then((r) => {
                  if (r) { status.setData(r); setC(r.config) }
                })
              }
            >
              Save
            </button>
            {(st.fingerprint_cgroup || st.ssl_libraries.length > 0) && (
              <p className="text-xs text-[var(--text-muted)]">
                {st.fingerprint_cgroup ? `mn_tlsfp on ${st.fingerprint_cgroup}. ` : ''}
                {st.ssl_libraries.length > 0 ? `Hooked: ${st.ssl_libraries.join(', ')}` : ''}
              </p>
            )}
            {st.notes.length > 0 && <ul className="text-xs text-[var(--text-muted)] space-y-1">{st.notes.map((n) => <li key={n}>{n}</li>)}</ul>}
          </div>
        ) : null}
      </MacGlassPanel>

      <MacGlassPanel title="Top JA4" subtitle="Most common client fingerprints in the recent window">
        {top.length === 0 ? (
          <Empty>No fingerprints yet.</Empty>
        ) : (
          <ul className="text-xs font-mono space-y-1">
            {top.map(([ja4, e]) => (
              <li key={ja4} className="flex justify-between gap-2">
                <span className="break-all">{ja4} <span className="text-[var(--text-muted)]">{[...e.snis].slice(0, 3).join(', ')}</span></span>
                <span className="text-[var(--text-muted)]">{e.count}</span>
              </li>
            ))}
          </ul>
        )}
      </MacGlassPanel>

      <MacGlassPanel title="Fingerprints">
        {fps.error ? (
          <Empty>{fps.error}</Empty>
        ) : (fps.data ?? []).length === 0 ? (
          <Empty>No ClientHellos sampled.</Empty>
        ) : (
          <TahoeTableWrap>
            <table className="w-full text-xs" aria-label="TLS fingerprints">
              <thead>
                <tr className={headRowCls}>
                  <th scope="col" className={thCls}>Time</th>
                  <th scope="col" className={thCls}>Workload</th>
                  <th scope="col" className={thCls}>Server</th>
                  <th scope="col" className={thCls}>JA4</th>
                  <th scope="col" className="py-2">JA3</th>
                </tr>
              </thead>
              <tbody>
                {(fps.data ?? []).slice(0, 100).map((f, i) => (
                  <tr key={`${f.ts}-${f.client}-${i}`} className={rowCls}>
                    <td className="py-2 pr-2 text-[var(--text-muted)]">{new Date(f.ts).toLocaleTimeString()}</td>
                    <td className="py-2 pr-2">{workloadLabel(f.workload) || f.iface || f.source}</td>
                    <td className="py-2 pr-2 font-mono">{f.sni ?? f.server}:{f.server_port}</td>
                    <td className="py-2 pr-2 font-mono">{f.ja4}{f.truncated ? ' *' : ''}</td>
                    <td className="py-2 font-mono">{f.ja3_hash}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        )}
      </MacGlassPanel>

      <MacGlassPanel title="TLS plaintext metadata" subtitle="Request lines and response codes from SSL_read / SSL_write (admin only)">
        {sslError ? (
          <Empty>{sslError}</Empty>
        ) : !ssl || ssl.length === 0 ? (
          <Empty>No SSL events.</Empty>
        ) : (
          <ul className="text-xs font-mono space-y-1 max-h-[50vh] overflow-y-auto">
            {ssl.map((r, i) => (
              <li key={`${r.ts}-${r.pid}-${i}`}>
                <span className="text-[var(--text-muted)]">{new Date(r.ts).toLocaleTimeString()} </span>
                {r.comm}[{r.pid}] {r.direction} {r.protocol}
                {r.method ? ` ${r.method} ${r.host ?? ''}${r.path ?? ''}` : ''}
                {r.status ? ` → ${r.status}` : ''}
                {r.workload ? <span className="text-[var(--text-muted)]"> ({workloadLabel(r.workload)})</span> : null}
              </li>
            ))}
          </ul>
        )}
      </MacGlassPanel>
    </>
  )
}
