// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// VMM guard: BPF-LSM hooks on QEMU cgroups. Audit by default; enforcement
// needs `bpf` as an active LSM and a lease.

import { useCallback, useState } from 'react'
import { MacGlassPanel, MacToggle } from '../platform/mac/PlatformMacUi'
import { getBpfGuard, getBpfGuardEvents, setBpfGuard } from '../../api/bpf'
import { statusPillClasses } from '../../utils/semanticColors'
import { Empty, Field, Metrics, headRowCls, rowCls, thCls, useBpfAction, useBpfLoad } from './shared'

export default function VmmGuardTab() {
  const action = useBpfAction()
  const { data: st, setData, error } = useBpfLoad(useCallback(() => getBpfGuard(), []))
  const { data: events, reload } = useBpfLoad(useCallback(() => getBpfGuardEvents(300), []))
  const [lease, setLease] = useState(300)

  if (error) return <MacGlassPanel title="VMM guard"><Empty>{error}</Empty></MacGlassPanel>
  if (!st) return null
  const save = (patch: Partial<typeof st.config>, msg = 'VMM guard updated') =>
    void action(setBpfGuard({ ...st.config, ...patch }), msg).then((r) => { if (r) setData(r) })
  const pill = st.enforcing
    ? { tone: 'warn' as const, text: `Enforcing · ${st.lease_remaining_secs ?? 0}s left` }
    : st.hooks.length
      ? { tone: 'ok' as const, text: 'Auditing' }
      : { tone: 'neutral' as const, text: 'Off' }

  return (
    <>
      {!st.lsm_active && (
        <MacGlassPanel title="BPF-LSM is not active on this host">
          <p className="text-sm text-[var(--text-secondary)]">
            The kernel supports BPF-LSM, but <code className="font-mono">bpf</code> is not in the active LSM list
            (<code className="font-mono">{st.lsm_list || 'unknown'}</code>), so the guard cannot run. Add it to the kernel
            command line — for example <code className="font-mono">lsm={st.lsm_list ? `${st.lsm_list},bpf` : '…,bpf'}</code> in{' '}
            <code className="font-mono">GRUB_CMDLINE_LINUX</code> — then update GRUB and reboot.
          </p>
        </MacGlassPanel>
      )}
      <Metrics
        items={[
          { label: 'Audited', value: st.audited.toLocaleString() },
          { label: 'Denied', value: st.denied.toLocaleString() },
          { label: 'Guarded VMs', value: st.guarded.length.toLocaleString() },
          { label: 'Dropped events', value: st.dropped.toLocaleString() },
        ]}
      />
      <MacGlassPanel
        title="VMM guard"
        subtitle="LSM hooks on each QEMU cgroup: exec outside the binary allowlist, writable+executable mappings, and char devices outside QEMU's device allowlist."
        action={<span className={statusPillClasses(pill.tone)}>{pill.text}</span>}
      >
        <div className="space-y-3">
          <div className="flex flex-wrap gap-4">
            <MacToggle label="Enabled" checked={st.config.enabled} onChange={(enabled) => save({ enabled, mode: 'audit', lease_secs: null })} />
            <MacToggle label="Exec allowlist" checked={st.config.exec} onChange={(exec) => save({ exec })} />
            <MacToggle label="W^X" checked={st.config.wx} onChange={(wx) => save({ wx })} />
            <MacToggle label="Device allowlist" checked={st.config.devices} onChange={(devices) => save({ devices })} />
          </div>
          <div className="flex flex-wrap items-end gap-2">
            <Field label="Lease (s)" htmlFor="guard-lease">
              <input
                id="guard-lease"
                type="number"
                min={1}
                max={3600}
                className="input text-sm w-28"
                value={lease}
                onChange={(e) => setLease(Number(e.target.value))}
              />
            </Field>
            {st.enforcing ? (
              <button type="button" className="btn-secondary text-sm" onClick={() => save({ mode: 'audit', lease_secs: null }, 'Back to audit')}>
                Stop enforcing
              </button>
            ) : (
              <button
                type="button"
                className="btn-primary text-sm"
                disabled={!st.lsm_active || !st.config.enabled}
                onClick={() => save({ mode: 'enforce', lease_secs: lease }, `Enforcing for ${lease}s`)}
              >
                Enforce under lease
              </button>
            )}
          </div>
          {st.lease_expired && <p className="text-xs text-[var(--text-muted)]">The last enforcement lease expired; the guard is back in audit mode.</p>}
          {st.notes.map((n) => <p key={n} className="text-xs text-[var(--text-muted)]">{n}</p>)}
          <details className="text-xs text-[var(--text-secondary)]">
            <summary className="cursor-pointer">Allowlists ({st.allowed_exec.length} binaries, {st.allowed_devices.length} devices)</summary>
            <div className="mt-2 grid gap-2 md:grid-cols-2 font-mono">
              <div>{st.allowed_exec.map((p) => <div key={p}>{p}</div>)}</div>
              <div>{st.allowed_devices.map((d) => <div key={d}>{d}</div>)}</div>
            </div>
          </details>
        </div>
      </MacGlassPanel>
      <MacGlassPanel
        title="Violations"
        action={<button type="button" className="btn-secondary text-sm" onClick={() => void reload()}>Refresh</button>}
      >
        {!events?.length ? (
          <Empty>No violations recorded.</Empty>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className={headRowCls}>
                  <th className={thCls}>Time</th>
                  <th className={thCls}>VM</th>
                  <th className={thCls}>Hook</th>
                  <th className={thCls}>Verdict</th>
                  <th className={thCls}>Process</th>
                  <th className={thCls}>Detail</th>
                </tr>
              </thead>
              <tbody>
                {events.map((e, i) => (
                  <tr key={`${e.ts}-${i}`} className={rowCls}>
                    <td className="py-1.5 pr-2 whitespace-nowrap">{new Date(e.ts).toLocaleTimeString()}</td>
                    <td className="py-1.5 pr-2">{e.vm ?? '—'}</td>
                    <td className="py-1.5 pr-2 font-mono">{e.hook}</td>
                    <td className="py-1.5 pr-2">
                      <span className={statusPillClasses(e.denied ? 'error' : 'info')}>{e.denied ? 'Denied' : 'Audited'}</span>
                    </td>
                    <td className="py-1.5 pr-2 font-mono">{e.comm} ({e.tgid})</td>
                    <td className="py-1.5 pr-2 text-xs">{e.detail}</td>
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
