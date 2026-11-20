// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Bridge-less direct redirect between an outer device and a VM tap. Entries
// sit idle until the enforcement lease is live; nothing is persisted.

import { useCallback, useState } from 'react'
import { MacGlassPanel, MacToggle } from '../platform/mac/PlatformMacUi'
import { getBpfDirect, setBpfDirect } from '../../api/bpf'
import { statusPillClasses } from '../../utils/semanticColors'
import { Empty, Field, Metrics, headRowCls, rowCls, thCls, useBpfAction, useBpfLoad } from './shared'

export default function DirectTab() {
  const action = useBpfAction()
  const { data: st, setData, error } = useBpfLoad(useCallback(() => getBpfDirect(), []))
  const [vm, setVm] = useState('')
  const [outer, setOuter] = useState('')
  const [tap, setTap] = useState('')
  const [ips, setIps] = useState('')
  const [force, setForce] = useState(false)

  if (error) return <MacGlassPanel title="Direct redirect"><Empty>{error}</Empty></MacGlassPanel>
  if (!st) return null
  const apply = (p: Promise<typeof st>, msg: string) =>
    void action(p, msg).then((r) => { if (r) setData(r) })
  const add = () =>
    apply(
      setBpfDirect({
        vm: vm.trim(),
        outer_iface: outer.trim(),
        tap: tap.trim() || null,
        ips: ips.split(/[\s,]+/).filter(Boolean),
        force,
      }),
      `Direct redirect set for ${vm}`,
    )
  const pill = st.active
    ? { tone: 'warn' as const, text: 'Redirecting' }
    : st.entries.length
      ? { tone: 'info' as const, text: 'Idle (no lease)' }
      : { tone: 'neutral' as const, text: 'Off' }

  return (
    <>
      <Metrics
        items={[
          { label: 'Into VMs', value: st.redirected_in.toLocaleString() },
          { label: 'Out of VMs', value: st.redirected_out.toLocaleString() },
          { label: 'Idle matches', value: st.idle.toLocaleString() },
          { label: 'Entries', value: st.entries.length.toLocaleString() },
        ]}
      />
      <MacGlassPanel
        title="Direct redirect"
        subtitle="Skip the bridge: frames for a VM's MAC or IP go straight from the outer device to its tap, and back. Redirects only while the enforcement lease is live."
        action={<span className={statusPillClasses(pill.tone)}>{pill.text}</span>}
      >
        <div className="space-y-3">
          <div className="flex flex-wrap items-end gap-2">
            <Field label="VM" htmlFor="direct-vm">
              <input id="direct-vm" className="input text-sm w-40" value={vm} onChange={(e) => setVm(e.target.value)} />
            </Field>
            <Field label="Outer device" htmlFor="direct-outer">
              <input id="direct-outer" className="input text-sm w-36 font-mono" value={outer} onChange={(e) => setOuter(e.target.value)} />
            </Field>
            <Field label="Tap (optional)" htmlFor="direct-tap">
              <input id="direct-tap" className="input text-sm w-32 font-mono" value={tap} onChange={(e) => setTap(e.target.value)} />
            </Field>
            <Field label="Guest IPs" htmlFor="direct-ips">
              <input id="direct-ips" className="input text-sm w-48 font-mono" value={ips} onChange={(e) => setIps(e.target.value)} />
            </Field>
            <MacToggle label="Allow physical NIC" checked={force} onChange={setForce} />
            <button type="button" className="btn-primary text-sm" disabled={!vm.trim() || !outer.trim()} onClick={add}>
              Add
            </button>
          </div>
          {st.entries.length === 0 ? (
            <Empty>No direct redirects.</Empty>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className={headRowCls}>
                    <th className={thCls}>VM</th>
                    <th className={thCls}>Outer</th>
                    <th className={thCls}>Tap</th>
                    <th className={thCls}>MAC</th>
                    <th className={thCls}>IPs</th>
                    <th className={thCls} />
                  </tr>
                </thead>
                <tbody>
                  {st.entries.map((e) => (
                    <tr key={e.vm} className={rowCls}>
                      <td className="py-1.5 pr-2">{e.vm}</td>
                      <td className="py-1.5 pr-2 font-mono">{e.outer_iface}</td>
                      <td className="py-1.5 pr-2 font-mono">{e.tap}{e.reverse ? '' : ' (in only)'}</td>
                      <td className="py-1.5 pr-2 font-mono">{e.mac}</td>
                      <td className="py-1.5 pr-2 font-mono">{e.ips.join(', ') || '—'}</td>
                      <td className="py-1.5 text-right">
                        <button
                          type="button"
                          className="btn-secondary text-xs"
                          onClick={() => apply(setBpfDirect({ vm: e.vm, outer_iface: e.outer_iface, enabled: false }), `Removed ${e.vm}`)}
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
