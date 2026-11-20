// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Just-in-time access: a policy that lets one VM (or the host) reach a port
// on another VM and removes itself. On the fleet, requests go to Approvals
// and need a second admin.

import { useCallback, useEffect, useState } from 'react'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import { TahoeTableWrap } from '../platform/tahoe/TahoeListKit'
import { Empty, Field, headRowCls, rowCls, thCls } from '../bpf/shared'
import {
  JIT_DURATIONS,
  approveJit,
  describeJit,
  formatRemaining,
  listJit,
  rejectJit,
  requestJit,
  revokeJit,
  type JitGrant,
  type JitPending,
  type NetpolScope,
} from '../../api/vmNetpol'
import { formatUserError } from '../../utils/apiError'
import { useToastContext } from '../../contexts/ToastContext'
import { statusPillClasses } from '../../utils/semanticColors'

export default function JitPanel({
  scope,
  vmNames,
  onChanged,
}: {
  scope: NetpolScope
  vmNames: string[]
  onChanged: () => void
}) {
  const toast = useToastContext()
  const fleet = scope === 'fleet'
  const [grants, setGrants] = useState<JitGrant[]>([])
  const [pending, setPending] = useState<JitPending[]>([])
  const [form, setForm] = useState({ from: '', to: '', port: '22', protocol: 'TCP', secs: 3600, reason: '' })
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    try {
      const r = await listJit(scope)
      setGrants(r.items)
      setPending(r.pending)
    } catch {
      setGrants([])
      setPending([])
    }
  }, [scope])

  useEffect(() => { void load() }, [load])

  const changed = async () => {
    await load()
    onChanged()
  }

  const submit = async (grantNow: boolean) => {
    const port = Number(form.port || 0)
    if (!Number.isInteger(port) || port < 0 || port > 65535) {
      toast.error('Port must be 0–65535 (0 or empty = every port)')
      return
    }
    setBusy(true)
    try {
      const r = await requestJit(
        scope,
        { from: form.from.trim(), to: form.to.trim(), port, protocol: form.protocol, secs: form.secs, reason: form.reason.trim() || undefined },
        grantNow,
      )
      if (r.pending) toast.success('Requested — another admin approves it in Approvals or below')
      else if (r.granted) toast.success(`Granted ${describeJit(r.granted)} — ${formatRemaining(r.granted.remaining_secs)}`)
      setForm((f) => ({ ...f, reason: '' }))
      await changed()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const decide = async (p: JitPending, approve: boolean) => {
    try {
      await (approve ? approveJit(p.id) : rejectJit(p.id))
      toast.success(approve ? `Approved: ${p.label}` : `Rejected: ${p.label}`)
      await changed()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const revoke = async (g: JitGrant) => {
    if (!window.confirm(`Revoke ${describeJit(g)} now?`)) return
    try {
      await revokeJit(scope, g.name)
      toast.success(`Revoked ${describeJit(g)}`)
      await changed()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const ready = form.from.trim() !== '' && form.to.trim() !== ''

  return (
    <MacGlassPanel
      title="Temporary access"
      subtitle={`Let one VM (or the host) reach a port on another VM for a fixed time. The grant only adds an allow — it never isolates either VM — and removes itself when the time is up.${fleet ? ' Requests need a second admin to approve.' : ''}`}
    >
      <div id="jit-form" className="flex flex-wrap items-end gap-3">
        <Field label="From" htmlFor="jit-from">
          <input id="jit-from" list="jit-vms" className="input text-sm w-40" placeholder="VM or host" value={form.from} onChange={(e) => setForm({ ...form, from: e.target.value })} />
        </Field>
        <Field label="To" htmlFor="jit-to">
          <input id="jit-to" list="jit-vms" className="input text-sm w-40" value={form.to} onChange={(e) => setForm({ ...form, to: e.target.value })} />
        </Field>
        <Field label="Port" htmlFor="jit-port">
          <input id="jit-port" className="input text-sm w-20" inputMode="numeric" placeholder="any" value={form.port} onChange={(e) => setForm({ ...form, port: e.target.value })} />
        </Field>
        <Field label="Protocol" htmlFor="jit-proto">
          <select id="jit-proto" className="input text-sm" value={form.protocol} onChange={(e) => setForm({ ...form, protocol: e.target.value })}>
            {['TCP', 'UDP', 'SCTP', 'ANY'].map((p) => <option key={p}>{p}</option>)}
          </select>
        </Field>
        <Field label="For" htmlFor="jit-secs">
          <select id="jit-secs" className="input text-sm" value={form.secs} onChange={(e) => setForm({ ...form, secs: Number(e.target.value) })}>
            {JIT_DURATIONS.map((d) => <option key={d.secs} value={d.secs}>{d.label}</option>)}
          </select>
        </Field>
        <Field label="Reason" htmlFor="jit-reason">
          <input id="jit-reason" className="input text-sm w-52" placeholder="DB migration" value={form.reason} onChange={(e) => setForm({ ...form, reason: e.target.value })} />
        </Field>
        <button type="button" className="btn-primary text-sm" disabled={busy || !ready} onClick={() => void submit(false)}>
          {fleet ? 'Request access' : 'Grant access'}
        </button>
        {fleet && (
          <button type="button" className="btn-secondary text-sm" disabled={busy || !ready} onClick={() => void submit(true)} title="Admins: grant without a second approval">
            Grant now
          </button>
        )}
        <datalist id="jit-vms">
          <option value="host" />
          {vmNames.map((n) => <option key={n} value={n} />)}
        </datalist>
      </div>

      {pending.length > 0 && (
        <div className="mt-4">
          <TahoeTableWrap>
            <table className="w-full text-xs" aria-label="Pending access requests">
              <thead>
                <tr className={headRowCls}>
                  <th scope="col" className={thCls}>Request</th>
                  <th scope="col" className={thCls}>Reason</th>
                  <th scope="col" className={thCls}>By</th>
                  <th scope="col" className={thCls}>Since</th>
                  <th scope="col" className="py-2"><span className="sr-only">Actions</span></th>
                </tr>
              </thead>
              <tbody>
                {pending.map((p) => (
                  <tr key={p.id} className={rowCls}>
                    <td className="py-2 pr-2 font-medium">{p.label}</td>
                    <td className="py-2 pr-2">{p.object_ref?.reason || '—'}</td>
                    <td className="py-2 pr-2">{p.requested_by}</td>
                    <td className="py-2 pr-2 text-[var(--text-muted)]">{p.created_at}</td>
                    <td className="py-2 text-right whitespace-nowrap">
                      <button type="button" className="btn-primary text-xs mr-2" onClick={() => void decide(p, true)}>Approve</button>
                      <button type="button" className="btn-secondary text-xs" onClick={() => void decide(p, false)}>Reject</button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        </div>
      )}

      <div className="mt-4">
        {grants.length === 0 ? (
          <Empty>No temporary access.</Empty>
        ) : (
          <TahoeTableWrap>
            <table className="w-full text-xs" aria-label="Temporary access">
              <thead>
                <tr className={headRowCls}>
                  <th scope="col" className={thCls}>Access</th>
                  <th scope="col" className={thCls}>Remaining</th>
                  <th scope="col" className={thCls}>Reason</th>
                  <th scope="col" className={thCls}>Granted by</th>
                  <th scope="col" className="py-2"><span className="sr-only">Actions</span></th>
                </tr>
              </thead>
              <tbody>
                {grants.map((g) => (
                  <tr key={g.name} className={rowCls}>
                    <td className="py-2 pr-2 font-mono">{describeJit(g)}</td>
                    <td className="py-2 pr-2">
                      <span className={statusPillClasses('warn')} title={`until ${g.expires_at}`}>{formatRemaining(g.remaining_secs)}</span>
                    </td>
                    <td className="py-2 pr-2">{g.reason || '—'}</td>
                    <td className="py-2 pr-2">{g.granted_by || '—'}</td>
                    <td className="py-2 text-right">
                      <button type="button" className="btn-secondary text-xs" onClick={() => void revoke(g)}>Revoke</button>
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
