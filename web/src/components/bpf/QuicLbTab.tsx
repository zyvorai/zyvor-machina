// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// QUIC connection-ID load balancer on the uplink XDP dispatcher.

import { useCallback, useState } from 'react'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import { getBpfQuicLb, setBpfQuicLb, type BpfQuicLbMode } from '../../api/bpf'
import { statusPillClasses } from '../../utils/semanticColors'
import { Empty, Field, Metrics, headRowCls, rowCls, thCls, useBpfAction, useBpfLoad } from './shared'

export default function QuicLbTab() {
  const action = useBpfAction()
  const { data: st, setData, error } = useBpfLoad(useCallback(() => getBpfQuicLb(), []))
  const [iface, setIface] = useState('')
  const [vip, setVip] = useState('')
  const [port, setPort] = useState(443)
  const [backends, setBackends] = useState('')
  const [cidLen, setCidLen] = useState(8)
  const [mode, setMode] = useState<BpfQuicLbMode>('dsr')

  if (error) return <MacGlassPanel title="QUIC load balancer"><Empty>{error}</Empty></MacGlassPanel>
  if (!st) return null
  const apply = (p: Promise<typeof st>, msg: string) => void action(p, msg).then((r) => { if (r) setData(r) })
  const add = () =>
    apply(
      setBpfQuicLb({
        iface: iface.trim() || st.iface || '',
        vip: vip.trim(),
        port,
        cid_len: cidLen,
        mode,
        backends: backends.split(/[\s,]+/).filter(Boolean).map((addr) => ({ addr })),
      }),
      `QUIC service ${vip}:${port} saved`,
    )
  const sum = (k: 'routed_cid' | 'maglev' | 'tx' | 'errors') => st.services.reduce((a, s) => a + s[k], 0)

  return (
    <>
      <Metrics
        items={[
          { label: 'Routed by CID', value: sum('routed_cid').toLocaleString() },
          { label: 'Routed by Maglev', value: sum('maglev').toLocaleString() },
          { label: 'Forwarded', value: sum('tx').toLocaleString() },
          { label: 'Errors', value: sum('errors').toLocaleString() },
        ]}
      />
      <MacGlassPanel
        title="QUIC load balancer"
        subtitle="UDP to a VIP is steered in XDP by the server id in the QUIC connection ID, so connections survive client address changes. Initials and unknown ids use Maglev on the 5-tuple."
        action={
          <span className={statusPillClasses(st.attached ? 'ok' : 'neutral')}>
            {st.attached ? `On ${st.iface}` : 'Off'}
          </span>
        }
      >
        <div className="space-y-3">
          <div className="flex flex-wrap items-end gap-2">
            <Field label="Uplink" htmlFor="qlb-iface">
              <input id="qlb-iface" className="input text-sm w-28 font-mono" placeholder={st.iface ?? 'eth0'} value={iface} onChange={(e) => setIface(e.target.value)} />
            </Field>
            <Field label="VIP" htmlFor="qlb-vip">
              <input id="qlb-vip" className="input text-sm w-36 font-mono" value={vip} onChange={(e) => setVip(e.target.value)} />
            </Field>
            <Field label="Port" htmlFor="qlb-port">
              <input id="qlb-port" type="number" min={1} max={65535} className="input text-sm w-24" value={port} onChange={(e) => setPort(Number(e.target.value))} />
            </Field>
            <Field label="Backends" htmlFor="qlb-be">
              <input id="qlb-be" className="input text-sm w-56 font-mono" placeholder="10.0.0.11, 10.0.0.12" value={backends} onChange={(e) => setBackends(e.target.value)} />
            </Field>
            <Field label="CID length" htmlFor="qlb-cid">
              <input id="qlb-cid" type="number" min={3} max={20} className="input text-sm w-20" value={cidLen} onChange={(e) => setCidLen(Number(e.target.value))} />
            </Field>
            <Field label="Delivery" htmlFor="qlb-mode">
              <select id="qlb-mode" className="input text-sm" value={mode} onChange={(e) => setMode(e.target.value as BpfQuicLbMode)}>
                <option value="dsr">Direct server return</option>
                <option value="ipip">IPIP</option>
              </select>
            </Field>
            <button type="button" className="btn-primary text-sm" disabled={!vip.trim() || !backends.trim()} onClick={add}>
              Save
            </button>
          </div>
          {st.services.length === 0 ? (
            <Empty>No QUIC services.</Empty>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className={headRowCls}>
                    <th className={thCls}>Service</th>
                    <th className={thCls}>Delivery</th>
                    <th className={thCls}>Backends (server id)</th>
                    <th className={thCls}>CID / Maglev</th>
                    <th className={thCls}>Unknown ids</th>
                    <th className={thCls}>Errors</th>
                    <th className={thCls} />
                  </tr>
                </thead>
                <tbody>
                  {st.services.map((s) => (
                    <tr key={`${s.vip}:${s.port}`} className={rowCls}>
                      <td className="py-1.5 pr-2 font-mono">{s.vip}:{s.port}</td>
                      <td className="py-1.5 pr-2">{s.mode === 'ipip' ? 'IPIP' : 'DSR'}</td>
                      <td className="py-1.5 pr-2 font-mono text-xs">
                        {s.backends.map((b) => `${b.addr} (${b.server_id.toString(16).padStart(4, '0')})`).join(', ')}
                      </td>
                      <td className="py-1.5 pr-2">{s.routed_cid.toLocaleString()} / {s.maglev.toLocaleString()}</td>
                      <td className="py-1.5 pr-2">{s.unknown_sid.toLocaleString()}</td>
                      <td className="py-1.5 pr-2">{s.errors.toLocaleString()}</td>
                      <td className="py-1.5 text-right">
                        <button
                          type="button"
                          className="btn-secondary text-xs"
                          onClick={() => apply(setBpfQuicLb({ vip: s.vip, port: s.port, enabled: false }), `Removed ${s.vip}:${s.port}`)}
                        >
                          Remove
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </MacGlassPanel>
    </>
  )
}
