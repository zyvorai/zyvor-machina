// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Per-container eBPF policy inside the guest (network allow rules on the
// container's cgroup, BPF-LSM MAC), applied by the in-guest GuestKit agent.
// Audit by default; enforce needs a lease and reverts by itself in-guest.

import { useCallback, useEffect, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import {
  applyGuestLsm,
  applyGuestNetpolicy,
  formatGuestNetRules,
  getGuestLsm,
  getGuestNetpolicy,
  parseGuestNetRules,
  type GuestLsmStatus,
  type GuestNetStatus,
  type GuestPolicyEvent,
  type GuestPolicyMode,
} from '../../api/guestPolicy'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { statusPillClasses } from '../../utils/semanticColors'

const labelCls = 'text-[10px] uppercase tracking-wider text-[var(--text-muted)]'
const cardCls = 'tahoe-glass-card p-5 space-y-4'
const list = (s: string) => s.split(/[\s,]+/).filter(Boolean)

function ModeLease({
  mode,
  setMode,
  lease,
  setLease,
}: {
  mode: GuestPolicyMode
  setMode: (m: GuestPolicyMode) => void
  lease: string
  setLease: (s: string) => void
}) {
  return (
    <>
      <label className="space-y-1">
        <span className={labelCls}>Mode</span>
        <select className="input py-1 text-xs" value={mode} onChange={(e) => setMode(e.target.value as GuestPolicyMode)}>
          <option value="audit">Audit (report only)</option>
          <option value="enforce">Enforce (leased)</option>
          <option value="off">Off (remove)</option>
        </select>
      </label>
      {mode === 'enforce' && (
        <label className="space-y-1">
          <span className={labelCls}>Lease (s, ≤ 3600)</span>
          <input className="input w-24 py-1 text-xs font-mono" value={lease} onChange={(e) => setLease(e.target.value)} />
        </label>
      )}
    </>
  )
}

function ModePill({ mode, lease, expired }: { mode: string; lease: number | null; expired: boolean }) {
  if (mode === 'enforce') return <span className={statusPillClasses('warn')}>Enforcing · {lease ?? 0}s left</span>
  return <span className={statusPillClasses(expired ? 'neutral' : 'info')}>{expired ? 'Audit (lease expired)' : 'Audit'}</span>
}

function Events({ events }: { events: GuestPolicyEvent[] }) {
  if (!events.length) return <p className="text-xs text-[var(--text-muted)]">No violations recorded.</p>
  return (
    <ul className="text-xs font-mono space-y-0.5 max-h-48 overflow-y-auto">
      {events.slice(-20).reverse().map((e, i) => (
        <li key={`${e.ts_ns}-${i}`} className={e.denied ? 'text-red-500' : 'text-[var(--text-secondary)]'}>
          {e.denied ? 'DENY ' : 'AUDIT'} {e.target ?? e.cgroup_id} {e.action}{' '}
          {e.kind === 'net' ? `${e.peer ?? ''}${e.port ? `:${e.port}` : ''} proto ${e.proto ?? 0}` : `${e.comm ?? ''} ${e.detail ?? ''}`}
        </li>
      ))}
    </ul>
  )
}

export default function VmGuestPolicyPanel({ vmName, running }: { vmName: string; running: boolean }) {
  const toast = useToastContext()
  const [net, setNet] = useState<GuestNetStatus | null>(null)
  const [lsm, setLsm] = useState<GuestLsmStatus | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const [npContainer, setNpContainer] = useState('')
  const [npMode, setNpMode] = useState<GuestPolicyMode>('audit')
  const [npLease, setNpLease] = useState('300')
  const [npEgress, setNpEgress] = useState(true)
  const [npIngress, setNpIngress] = useState(false)
  const [npRules, setNpRules] = useState('')

  const [lsContainer, setLsContainer] = useState('')
  const [lsMode, setLsMode] = useState<GuestPolicyMode>('audit')
  const [lsLease, setLsLease] = useState('300')
  const [denyExec, setDenyExec] = useState(false)
  const [allowExec, setAllowExec] = useState('')
  const [denyWx, setDenyWx] = useState(true)
  const [restrictDevices, setRestrictDevices] = useState(false)
  const [allowDevices, setAllowDevices] = useState('')
  const [restrictWrites, setRestrictWrites] = useState(false)
  const [writable, setWritable] = useState('/tmp')

  const load = useCallback(async () => {
    if (!running) return
    setErr(null)
    try {
      const [n, l] = await Promise.all([getGuestNetpolicy(vmName), getGuestLsm(vmName)])
      setNet(n)
      setLsm(l)
    } catch (e) {
      setErr(formatUserError(e))
    }
  }, [vmName, running])

  useEffect(() => {
    void load()
  }, [load])

  const run = async (p: () => Promise<unknown>, ok: string) => {
    setBusy(true)
    try {
      await p()
      toast.success(ok)
      await load()
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const leaseOf = (mode: GuestPolicyMode, s: string) => (mode === 'enforce' ? Number(s) || undefined : undefined)

  const applyNet = () =>
    void run(async () => {
      const rules = parseGuestNetRules(npRules)
      await applyGuestNetpolicy(vmName, {
        container: npContainer.trim(),
        mode: npMode,
        lease_secs: leaseOf(npMode, npLease),
        egress: npEgress,
        ingress: npIngress,
        rules,
      })
    }, npMode === 'off' ? `Network policy removed from ${npContainer}` : `Network policy (${npMode}) applied to ${npContainer}`)

  const applyLsm = () =>
    void run(
      () =>
        applyGuestLsm(vmName, {
          container: lsContainer.trim(),
          mode: lsMode,
          lease_secs: leaseOf(lsMode, lsLease),
          deny_exec: denyExec,
          allow_exec: list(allowExec),
          deny_wx: denyWx,
          restrict_devices: restrictDevices,
          allow_devices: allowDevices.split(',').map((s) => s.trim()).filter(Boolean),
          restrict_writes: restrictWrites,
          writable_paths: list(writable),
        }),
      lsMode === 'off' ? `MAC policy removed from ${lsContainer}` : `MAC policy (${lsMode}) applied to ${lsContainer}`,
    )

  if (!running) {
    return <div className={cardCls}><p className="text-sm text-[var(--text-muted)]">Start the VM to manage per-container policy inside the guest.</p></div>
  }

  const unavailable = (s: GuestNetStatus | GuestLsmStatus | null) =>
    s && !s.available ? <p className="text-xs text-amber-600">Unavailable in the guest: {s.reason}</p> : null

  return (
    <div className="space-y-4">
      <div className="flex items-start justify-between gap-4">
        <p className="text-xs text-[var(--text-muted)] max-w-2xl">
          Enforced inside the guest by GuestKit (guestkitd) on each container&apos;s cgroup. Requires
          <code className="mx-1">capabilities: {'{'} ebpf: true {'}'}</code>in the guest&apos;s
          <code className="mx-1">/etc/guestkit/agent-policy.yaml</code>. Audit reports violations only; enforce
          drops/denies them until its lease runs out, then reverts to audit in the guest kernel. Nothing persists
          across a guestkitd restart.
        </p>
        <button type="button" className="btn-ghost text-xs inline-flex items-center gap-1" onClick={() => void load()} disabled={busy}>
          <RefreshCw className="w-3 h-3" /> Refresh
        </button>
      </div>
      {err && <div className={cardCls}><p className="text-sm text-red-500">{err}</p></div>}

      <section className={cardCls}>
        <h3 className="text-sm font-semibold">Container network policy</h3>
        {unavailable(net)}
        {net?.targets.length ? (
          <table className="w-full text-xs">
            <thead>
              <tr className="text-left text-[var(--text-muted)]">
                <th className="pb-1">Target</th><th>Mode</th><th>Policed</th><th>Rules</th><th>Allowed / audited / denied</th>
              </tr>
            </thead>
            <tbody>
              {net.targets.map((t) => (
                <tr key={t.target} className="border-t border-[var(--apple-hairline)]">
                  <td className="py-1 font-mono">{t.target}</td>
                  <td><ModePill mode={t.mode} lease={t.lease_remaining_secs} expired={t.lease_expired} /></td>
                  <td>{[t.egress && 'egress', t.ingress && 'ingress'].filter(Boolean).join(' + ')}</td>
                  <td className="font-mono whitespace-pre">{formatGuestNetRules(t.rules) || '—'}</td>
                  <td className="font-mono">{t.counters.allowed} / {t.counters.audited} / {t.counters.denied}</td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          net?.available && <p className="text-xs text-[var(--text-muted)]">No container is policed.</p>
        )}
        <div className="flex flex-wrap gap-3 items-end">
          <label className="space-y-1">
            <span className={labelCls}>Container</span>
            <input className="input py-1 text-xs" placeholder="name or id" value={npContainer} onChange={(e) => setNpContainer(e.target.value)} />
          </label>
          <ModeLease mode={npMode} setMode={setNpMode} lease={npLease} setLease={setNpLease} />
          <label className="flex items-center gap-1 text-xs"><input type="checkbox" checked={npEgress} onChange={(e) => setNpEgress(e.target.checked)} /> Egress</label>
          <label className="flex items-center gap-1 text-xs"><input type="checkbox" checked={npIngress} onChange={(e) => setNpIngress(e.target.checked)} /> Ingress</label>
        </div>
        {npMode !== 'off' && (
          <label className="space-y-1 block">
            <span className={labelCls}>Allow rules — one per line: egress|ingress CIDR [tcp|udp|any] [port]</span>
            <textarea
              className="input w-full font-mono text-xs h-20"
              placeholder={'egress 10.0.0.0/8 tcp 5432\negress 0.0.0.0/0 udp 53'}
              value={npRules}
              onChange={(e) => setNpRules(e.target.value)}
            />
          </label>
        )}
        <button type="button" className="btn-primary text-xs" disabled={busy || !npContainer.trim()} onClick={applyNet}>
          {npMode === 'off' ? 'Remove policy' : 'Apply network policy'}
        </button>
        <div className="space-y-1">
          <p className={labelCls}>Recent violations</p>
          <Events events={net?.events ?? []} />
        </div>
      </section>

      <section className={cardCls}>
        <div className="flex items-center gap-2">
          <h3 className="text-sm font-semibold">Container MAC (BPF-LSM)</h3>
          {lsm && (
            <span className={statusPillClasses(lsm.lsm_active ? 'ok' : 'warn')}>
              {lsm.lsm_active ? 'BPF-LSM active' : 'BPF-LSM inactive'}
            </span>
          )}
        </div>
        {unavailable(lsm)}
        {lsm?.note && <p className="text-xs text-amber-600">{lsm.note}</p>}
        {lsm?.targets.length ? (
          <table className="w-full text-xs">
            <thead>
              <tr className="text-left text-[var(--text-muted)]">
                <th className="pb-1">Target</th><th>Mode</th><th>Controls</th><th>Violations</th>
              </tr>
            </thead>
            <tbody>
              {lsm.targets.map((t) => (
                <tr key={t.target} className="border-t border-[var(--apple-hairline)]">
                  <td className="py-1 font-mono">{t.target}</td>
                  <td><ModePill mode={t.mode} lease={t.lease_remaining_secs} expired={t.lease_expired} /></td>
                  <td>
                    {[t.deny_exec && 'exec', t.deny_wx && 'W+X', t.restrict_devices && 'devices', t.restrict_writes && 'writes']
                      .filter(Boolean)
                      .join(', ')}
                  </td>
                  <td className="font-mono">
                    {t.counters.length
                      ? t.counters.map((c) => `${c.action}${c.denied ? ' denied' : ''}: ${c.count}`).join(' · ')
                      : '—'}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          lsm?.available && <p className="text-xs text-[var(--text-muted)]">No container has a MAC policy.</p>
        )}
        <div className="flex flex-wrap gap-3 items-end">
          <label className="space-y-1">
            <span className={labelCls}>Container</span>
            <input className="input py-1 text-xs" placeholder="name or id" value={lsContainer} onChange={(e) => setLsContainer(e.target.value)} />
          </label>
          <ModeLease mode={lsMode} setMode={setLsMode} lease={lsLease} setLease={setLsLease} />
        </div>
        {lsMode !== 'off' && (
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1">
              <label className="flex items-center gap-1 text-xs"><input type="checkbox" checked={denyExec} onChange={(e) => setDenyExec(e.target.checked)} /> Deny exec outside allowlist</label>
              <input className="input w-full py-1 text-xs font-mono" placeholder="/usr/bin/python3, /app/server" value={allowExec} onChange={(e) => setAllowExec(e.target.value)} disabled={!denyExec} />
            </div>
            <label className="flex items-center gap-1 text-xs self-start"><input type="checkbox" checked={denyWx} onChange={(e) => setDenyWx(e.target.checked)} /> Deny writable+executable mappings</label>
            <div className="space-y-1">
              <label className="flex items-center gap-1 text-xs"><input type="checkbox" checked={restrictDevices} onChange={(e) => setRestrictDevices(e.target.checked)} /> Restrict device opens (tty/null/random always allowed)</label>
              <input className="input w-full py-1 text-xs font-mono" placeholder="c 10:200, b 8:*" value={allowDevices} onChange={(e) => setAllowDevices(e.target.value)} disabled={!restrictDevices} />
            </div>
            <div className="space-y-1">
              <label className="flex items-center gap-1 text-xs"><input type="checkbox" checked={restrictWrites} onChange={(e) => setRestrictWrites(e.target.checked)} /> Restrict writes to these filesystems</label>
              <input className="input w-full py-1 text-xs font-mono" placeholder="/tmp, /data" value={writable} onChange={(e) => setWritable(e.target.value)} disabled={!restrictWrites} />
            </div>
          </div>
        )}
        <button type="button" className="btn-primary text-xs" disabled={busy || !lsContainer.trim()} onClick={applyLsm}>
          {lsMode === 'off' ? 'Remove MAC policy' : 'Apply MAC policy'}
        </button>
        <div className="space-y-1">
          <p className={labelCls}>Recent violations</p>
          <Events events={lsm?.events ?? []} />
        </div>
      </section>
    </div>
  )
}
