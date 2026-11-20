// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// One-click VM quarantine: every flow of the VM is cut for a fixed time,
// whatever the enforcement mode, and the kernel lifts it at the deadline.

import { useState } from 'react'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import { TahoeTableWrap } from '../platform/tahoe/TahoeListKit'
import { Empty, Field, headRowCls, rowCls, thCls } from '../bpf/shared'
import {
  QUARANTINE_DURATIONS,
  describeAllow,
  formatRemaining,
  groupQuarantines,
  quarantineVm,
  releaseQuarantine,
  type NetpolScope,
  type VmQuarantine,
} from '../../api/vmNetpol'
import { formatUserError } from '../../utils/apiError'
import { useToastContext } from '../../contexts/ToastContext'
import { statusPillClasses } from '../../utils/semanticColors'

export default function QuarantinePanel({
  scope,
  vmNames,
  vm,
  setVm,
  items,
  onChanged,
}: {
  scope: NetpolScope
  vmNames: string[]
  vm: string
  setVm: (vm: string) => void
  items: VmQuarantine[]
  onChanged: () => void
}) {
  const toast = useToastContext()
  const [secs, setSecs] = useState(3600)
  const [ssh, setSsh] = useState(true)
  const [reason, setReason] = useState('')
  const [busy, setBusy] = useState(false)
  const rows = groupQuarantines(items)
  const duration = QUARANTINE_DURATIONS.find((d) => d.secs === secs)?.label ?? `${secs}s`

  const quarantine = async () => {
    const name = vm.trim()
    if (!name) return
    const except = ssh ? ', except SSH from its host' : ''
    if (!window.confirm(`Quarantine ${name}? Every connection of the VM is cut now, including open ones${except}. It lifts itself after ${duration}.`)) return
    setBusy(true)
    try {
      await quarantineVm(scope, name, { secs, allow_host_ssh: ssh, reason: reason.trim() || undefined })
      toast.success(`${name} quarantined for ${duration}`)
      setReason('')
      setVm('')
      onChanged()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const release = async (name: string) => {
    if (!window.confirm(`Release ${name}? Its traffic is governed by the VM network policies again.`)) return
    try {
      await releaseQuarantine(scope, name)
      toast.success(`Released ${name}`)
      onChanged()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  return (
    <MacGlassPanel
      title="Quarantine"
      subtitle="Cut a suspicious VM off the network for a fixed time — every flow, including connections already open, in observe mode too. DHCP and IPv6 neighbour discovery still pass. The kernel lifts it when the time is up, even if Machina is down."
    >
      <div id="quarantine-form" className="flex flex-wrap items-end gap-3">
        <Field label="VM" htmlFor="q-vm">
          <input id="q-vm" list="q-vms" className="input text-sm w-44" value={vm} onChange={(e) => setVm(e.target.value)} />
        </Field>
        <Field label="For" htmlFor="q-secs">
          <select id="q-secs" className="input text-sm" value={secs} onChange={(e) => setSecs(Number(e.target.value))}>
            {QUARANTINE_DURATIONS.map((d) => <option key={d.secs} value={d.secs}>{d.label}</option>)}
          </select>
        </Field>
        <Field label="Reason" htmlFor="q-reason">
          <input id="q-reason" className="input text-sm w-56" value={reason} placeholder="port scan from web-1" onChange={(e) => setReason(e.target.value)} />
        </Field>
        <label className="text-xs flex items-center gap-1 pb-2">
          <input type="checkbox" checked={ssh} onChange={(e) => setSsh(e.target.checked)} /> Allow SSH from the host
        </label>
        <button type="button" className="btn-primary text-sm" disabled={busy || !vm.trim()} onClick={() => void quarantine()}>
          {busy ? 'Quarantining…' : 'Quarantine'}
        </button>
        <datalist id="q-vms">{vmNames.map((n) => <option key={n} value={n} />)}</datalist>
      </div>
      <div className="mt-4">
        {rows.length === 0 ? (
          <Empty>No VM is quarantined.</Empty>
        ) : (
          <TahoeTableWrap>
            <table className="w-full text-xs" aria-label="Quarantined VMs">
              <thead>
                <tr className={headRowCls}>
                  <th scope="col" className={thCls}>VM</th>
                  <th scope="col" className={thCls}>Remaining</th>
                  {scope === 'fleet' && <th scope="col" className={thCls}>Running on</th>}
                  <th scope="col" className={thCls}>Exceptions</th>
                  <th scope="col" className={thCls}>Reason</th>
                  <th scope="col" className={thCls}>By</th>
                  <th scope="col" className="py-2"><span className="sr-only">Actions</span></th>
                </tr>
              </thead>
              <tbody>
                {rows.map((q) => (
                  <tr key={q.vm} className={rowCls}>
                    <td className="py-2 pr-2 font-medium">
                      {q.vm}
                      {q.taps.length === 0 && <div className="text-[var(--text-muted)]">not running — applies when it starts</div>}
                    </td>
                    <td className="py-2 pr-2">
                      <span className={statusPillClasses('error')} title={`until ${q.until}`}>{formatRemaining(q.remaining_secs)}</span>
                    </td>
                    {scope === 'fleet' && <td className="py-2 pr-2">{q.hosts.join(', ') || '—'}</td>}
                    <td className="py-2 pr-2 font-mono">{q.allow.map(describeAllow).join('; ') || 'none'}</td>
                    <td className="py-2 pr-2">{q.reason || '—'}</td>
                    <td className="py-2 pr-2">{q.by || '—'}</td>
                    <td className="py-2 text-right">
                      <button type="button" className="btn-secondary text-xs" onClick={() => void release(q.vm)}>Release</button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        )}
      </div>
    </MacGlassPanel>
  )
}
