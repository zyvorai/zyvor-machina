// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// AF_XDP fast path on a dedicated interface. Consumers register their
// sockets with machina-bpfd directly; this tab attaches the program and
// shows the per-queue gates.

import { useCallback, useState } from 'react'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import { getBpfAfxdp, setBpfAfxdp } from '../../api/bpf'
import { statusPillClasses } from '../../utils/semanticColors'
import { Empty, Field, headRowCls, rowCls, thCls, useBpfAction, useBpfLoad } from './shared'

export default function AfxdpTab() {
  const action = useBpfAction()
  const { data: st, setData, error, reload } = useBpfLoad(useCallback(() => getBpfAfxdp(), []))
  const [iface, setIface] = useState('')

  if (error) return <MacGlassPanel title="AF_XDP"><Empty>{error}</Empty></MacGlassPanel>
  if (!st) return null
  const apply = (p: Promise<typeof st>, msg: string) => void action(p, msg).then((r) => { if (r) setData(r) })

  return (
    <MacGlassPanel
      title="AF_XDP fast path"
      subtitle="Frames on a queue with a registered AF_XDP socket go straight to that user-space consumer; everything else passes to the kernel. Only dedicated interfaces: anything with a default route or the uplink dispatcher is refused."
      action={<span className={statusPillClasses(st.attached ? 'ok' : 'neutral')}>{st.attached ? `On ${st.iface}` : 'Off'}</span>}
    >
      <div className="space-y-3">
        {st.attached ? (
          <div className="flex gap-2">
            <button type="button" className="btn-secondary text-sm" onClick={() => void reload()}>Refresh</button>
            <button
              type="button"
              className="btn-secondary text-sm"
              onClick={() => apply(setBpfAfxdp({ iface: st.iface ?? '', enabled: false }), 'AF_XDP detached')}
            >
              Detach
            </button>
          </div>
        ) : (
          <div className="flex flex-wrap items-end gap-2">
            <Field label="Dedicated interface" htmlFor="afxdp-iface">
              <input id="afxdp-iface" className="input text-sm w-40 font-mono" value={iface} onChange={(e) => setIface(e.target.value)} />
            </Field>
            <button
              type="button"
              className="btn-primary text-sm"
              disabled={!iface.trim()}
              onClick={() => apply(setBpfAfxdp({ iface: iface.trim(), enabled: true }), `AF_XDP attached to ${iface}`)}
            >
              Attach
            </button>
          </div>
        )}
        {st.queues.length === 0 ? (
          <Empty>{st.attached ? 'No consumer has registered a socket yet.' : 'Not attached.'}</Empty>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className={headRowCls}>
                  <th className={thCls}>Queue</th>
                  <th className={thCls}>To AF_XDP</th>
                  <th className={thCls}>Passed (no socket)</th>
                </tr>
              </thead>
              <tbody>
                {st.queues.map((q) => (
                  <tr key={q.queue} className={rowCls}>
                    <td className="py-1.5 pr-2 font-mono">{q.queue}</td>
                    <td className="py-1.5 pr-2">{q.redirected.toLocaleString()}</td>
                    <td className="py-1.5 pr-2">{q.no_socket.toLocaleString()}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </MacGlassPanel>
  )
}
