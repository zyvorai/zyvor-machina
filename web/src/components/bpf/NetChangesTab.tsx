// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Network change audit: which process added, removed or changed a link,
// address, route, neighbour, rule, qdisc or tc filter on this host.

import { useCallback, useState } from 'react'
import { MacGlassPanel, MacToggle } from '../platform/mac/PlatformMacUi'
import { getBpfRtnl, getBpfRtnlEvents, setBpfRtnl, workloadLabel } from '../../api/bpf'
import { statusPillClasses } from '../../utils/semanticColors'
import { Empty, Field, Metrics, headRowCls, rowCls, thCls, useBpfAction, useBpfLoad } from './shared'

const KINDS = ['link', 'addr', 'route', 'neigh', 'rule', 'qdisc', 'class', 'filter']

export default function NetChangesTab() {
  const action = useBpfAction()
  const { data: st, setData, error } = useBpfLoad(useCallback(() => getBpfRtnl(), []))
  const [iface, setIface] = useState('')
  const { data: events, reload } = useBpfLoad(useCallback(() => getBpfRtnlEvents(300, iface || undefined), [iface]))

  if (error) return <MacGlassPanel title="Network changes"><Empty>{error}</Empty></MacGlassPanel>
  if (!st) return null
  const kinds = st.config.kinds.length ? st.config.kinds : KINDS.filter((k) => k !== 'class')
  const save = (patch: Partial<typeof st.config>) =>
    void action(setBpfRtnl({ ...st.config, ...patch }), 'Network change audit updated').then((r) => { if (r) setData(r) })

  return (
    <>
      <Metrics
        items={[
          { label: 'Recorded', value: st.events.toLocaleString() },
          { label: 'Dropped (ring full)', value: st.dropped.toLocaleString() },
          { label: 'Stored', value: st.stored.toLocaleString() },
        ]}
      />
      <MacGlassPanel
        title="Network change audit"
        subtitle="A kprobe on rtnetlink_rcv_msg records the process behind every state-changing netlink request. Observe only."
        action={<span className={statusPillClasses(st.attached ? 'ok' : 'neutral')}>{st.attached ? 'Recording' : 'Off'}</span>}
      >
        <div className="space-y-3">
          <div className="flex flex-wrap gap-4">
            <MacToggle label="Enabled" checked={st.config.enabled} onChange={(enabled) => save({ enabled })} />
            <MacToggle label="Host namespace only" checked={st.config.host_netns_only} onChange={(host_netns_only) => save({ host_netns_only })} />
          </div>
          <div className="flex flex-wrap gap-3">
            {KINDS.map((k) => (
              <MacToggle
                key={k}
                label={k}
                checked={kinds.includes(k)}
                onChange={(on) => save({ kinds: on ? [...kinds, k] : kinds.filter((x) => x !== k) })}
              />
            ))}
          </div>
          {st.notes.map((n) => <p key={n} className="text-xs text-[var(--text-muted)]">{n}</p>)}
        </div>
      </MacGlassPanel>
      <MacGlassPanel
        title="Recent changes"
        action={
          <div className="flex items-end gap-2">
            <Field label="Interface" htmlFor="rtnl-iface">
              <input id="rtnl-iface" className="input text-sm font-mono w-32" value={iface} onChange={(e) => setIface(e.target.value.trim())} />
            </Field>
            <button type="button" className="btn-secondary text-sm" onClick={() => void reload()}>Refresh</button>
          </div>
        }
      >
        {!events?.length ? (
          <Empty>No network changes recorded yet.</Empty>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className={headRowCls}>
                  <th className={thCls}>Time</th>
                  <th className={thCls}>Change</th>
                  <th className={thCls}>Interface</th>
                  <th className={thCls}>Destination</th>
                  <th className={thCls}>Process</th>
                  <th className={thCls}>Workload</th>
                </tr>
              </thead>
              <tbody>
                {events.map((e, i) => (
                  <tr key={`${e.ts}-${i}`} className={rowCls}>
                    <td className="py-1.5 pr-2 whitespace-nowrap">{new Date(e.ts).toLocaleTimeString()}</td>
                    <td className="py-1.5 pr-2 font-mono">{e.kind} {e.action}{e.create ? ' (create)' : ''}</td>
                    <td className="py-1.5 pr-2 font-mono">{e.iface ?? (e.ifindex ? `if${e.ifindex}` : '—')}</td>
                    <td className="py-1.5 pr-2 font-mono">{e.dst ?? '—'}</td>
                    <td className="py-1.5 pr-2 font-mono" title={e.cmdline ?? undefined}>{e.comm} ({e.tgid}) uid {e.uid}</td>
                    <td className="py-1.5 pr-2">{workloadLabel(e.workload) || (e.host_netns === false ? 'other netns' : '—')}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </MacGlassPanel>
    </>
  )
}
