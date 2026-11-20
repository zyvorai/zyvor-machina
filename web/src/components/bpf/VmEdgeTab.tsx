// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// VM edge (per-tap identity policy, isolation, rate limits) and the QEMU
// sandbox (device allowlist + egress ports on machine-qemu scopes).

import { useCallback, useEffect, useState } from 'react'
import { MacGlassPanel, MacSegmentedControl, MacToggle } from '../platform/mac/PlatformMacUi'
import { TahoeTableWrap } from '../platform/tahoe/TahoeListKit'
import { getBpfSandbox, getBpfVmEdge, setBpfSandbox, type BpfSandboxConfig, type BpfSandboxHit } from '../../api/bpf'
import { statusPillClasses, statusToneClass } from '../../utils/semanticColors'
import { Empty, Field, Metrics, fmtBytes, headRowCls, rowCls, splitList, thCls, useBpfAction, useBpfLoad } from './shared'

function Hits({ title, hits }: { title: string; hits: BpfSandboxHit[] }) {
  return (
    <div>
      <h4 className="text-xs font-medium text-[var(--text-muted)] mb-1">{title}</h4>
      {hits.length === 0 ? (
        <Empty>None.</Empty>
      ) : (
        <ul className="text-xs font-mono space-y-1">
          {hits.slice(0, 20).map((h) => (
            <li key={`${h.vm ?? h.cgroup}-${h.target}`} className="flex justify-between gap-2">
              <span>{h.vm ?? h.cgroup ?? '?'} · {h.target}</span>
              <span className="text-[var(--text-muted)]">{h.count}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

export default function VmEdgeTab() {
  const action = useBpfAction()
  const edge = useBpfLoad(useCallback(() => getBpfVmEdge(), []))
  const sandbox = useBpfLoad(useCallback(() => getBpfSandbox(), []))
  const [draft, setDraft] = useState<BpfSandboxConfig | null>(null)
  useEffect(() => { if (sandbox.data && !draft) setDraft(sandbox.data.config) }, [sandbox.data, draft])

  const e = edge.data
  const s = sandbox.data
  return (
    <>
      {e && (
        <Metrics
          items={[
            { label: 'VMs', value: e.vms },
            { label: 'Taps', value: e.taps.length },
            { label: 'Groups', value: Object.keys(e.groups).length },
            { label: 'Rules', value: e.rules },
            { label: 'Edge', value: <span className={statusToneClass(e.enforcing ? 'warn' : 'neutral')}>{e.enforcing ? 'Enforcing' : 'Observing'}</span> },
          ]}
        />
      )}
      <MacGlassPanel
        title="VM edge"
        subtitle="Per-tap identity policy compiled from VM network policies, plus isolation and rate limits. Drops need the enforcement lease."
      >
        {edge.error ? (
          <Empty>{edge.error}</Empty>
        ) : !e || e.taps.length === 0 ? (
          <Empty>No VM taps under edge policy.</Empty>
        ) : (
          <TahoeTableWrap>
            <table className="w-full text-xs" aria-label="VM edge taps">
              <thead>
                <tr className={headRowCls}>
                  <th scope="col" className={thCls}>VM</th>
                  <th scope="col" className={thCls}>Tap</th>
                  <th scope="col" className={thCls}>Identity</th>
                  <th scope="col" className={thCls}>Flags</th>
                  <th scope="col" className={thCls}>Out</th>
                  <th scope="col" className={thCls}>In</th>
                  <th scope="col" className="py-2">Denied / rate-dropped</th>
                </tr>
              </thead>
              <tbody>
                {e.taps.map((t) => (
                  <tr key={t.iface} className={rowCls}>
                    <td className="py-2 pr-2">{t.vm}</td>
                    <td className="py-2 pr-2 font-mono">{t.iface}</td>
                    <td className="py-2 pr-2 font-mono">{t.identity}</td>
                    <td className="py-2 pr-2">{t.flags.join(', ') || '—'}</td>
                    <td className="py-2 pr-2">{fmtBytes(t.stats.out_bytes)}</td>
                    <td className="py-2 pr-2">{fmtBytes(t.stats.in_bytes)}</td>
                    <td className={`py-2 ${t.stats.denied + t.stats.rate_dropped > 0 ? statusToneClass('warn') : ''}`}>
                      {t.stats.denied} / {t.stats.rate_dropped}
                      {t.stats.observed > 0 ? <span className="text-[var(--text-muted)]"> ({t.stats.observed} observed)</span> : null}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        )}
        {e && e.missing.length > 0 && (
          <p className="text-xs text-[var(--text-muted)] mt-2">Not on this host: {e.missing.join(', ')}</p>
        )}
      </MacGlassPanel>

      <MacGlassPanel
        title="QEMU sandbox"
        subtitle="Device allowlist and egress ports on each VM's machine-qemu scope, AND-ed with libvirt's own device cgroup."
        action={s && <span className={statusPillClasses(s.enforcing ? 'warn' : 'neutral')}>{s.enforcing ? 'Enforcing' : 'Observing'}</span>}
      >
        {sandbox.error ? (
          <Empty>{sandbox.error}</Empty>
        ) : s && draft ? (
          <div className="space-y-4">
            <div className="flex flex-wrap items-end gap-4">
              <MacSegmentedControl
                label="Mode"
                options={[{ value: 'observe', label: 'Observe' }, { value: 'enforce', label: 'Enforce' }]}
                value={draft.mode === 'enforce' ? 'enforce' : 'observe'}
                onChange={(mode) => setDraft({ ...draft, mode })}
              />
              <MacToggle label="Sandbox every running VM" checked={draft.auto} onChange={(auto) => setDraft({ ...draft, auto })} />
            </div>
            <div className="grid gap-3 sm:grid-cols-2">
              <Field label="Extra device rules (c|b MAJOR:MINOR rwm, one per line)" htmlFor="sb-devices">
                <textarea
                  id="sb-devices"
                  className="input text-sm font-mono h-20"
                  value={draft.extra_devices.join('\n')}
                  onChange={(ev) => setDraft({ ...draft, extra_devices: ev.target.value.split('\n') })}
                />
              </Field>
              <Field label="QEMU egress ports (migration, NBD)" htmlFor="sb-ports">
                <input
                  id="sb-ports"
                  className="input text-sm font-mono"
                  value={draft.egress_ports.join(', ')}
                  onChange={(ev) => setDraft({ ...draft, egress_ports: ev.target.value.split(',') })}
                />
              </Field>
            </div>
            <button
              type="button"
              className="btn-primary text-sm"
              onClick={() =>
                void action(
                  setBpfSandbox({ ...draft, extra_devices: splitList(draft.extra_devices.join('\n')), egress_ports: splitList(draft.egress_ports.join(',')) }),
                  'Sandbox updated',
                ).then((r) => { if (r) { sandbox.setData(r); setDraft(r.config) } })
              }
            >
              Save sandbox
            </button>
            <div className="text-xs text-[var(--text-muted)]">
              Sandboxed: {Object.keys(s.attached).length === 0 ? 'none' : Object.keys(s.attached).join(', ')}
            </div>
            <div className="grid gap-4 sm:grid-cols-2">
              <Hits title="Device accesses outside the allowlist" hits={s.device_hits} />
              <Hits title="Egress outside allowed ports" hits={s.egress_hits} />
            </div>
            {s.notes.length > 0 && (
              <ul className="text-xs text-[var(--text-muted)] space-y-1">{s.notes.map((n) => <li key={n}>{n}</li>)}</ul>
            )}
          </div>
        ) : null}
      </MacGlassPanel>
    </>
  )
}
