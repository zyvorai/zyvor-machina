// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import { CheckCircle2, X, XCircle } from 'lucide-react'
import {
  deleteHost,
  enqueueValidateHost,
  fenceHost,
  getPlatformHostDetail,
  hostMaintenance,
  syncHost,
  type PlatformHost,
  type PlatformHostDetail,
} from '../../../api/platform'
import ConfirmDialog from '../../ConfirmDialog'
import { formatAge, heartbeatAgeSecs, isControllerHost } from '../../../utils/hostAttention'

type Props = {
  host: PlatformHost
  busy: boolean
  /** Runs an action with the page's toast + reload handling; resolves true when it succeeded. */
  run: (label: string, fn: () => Promise<unknown>) => Promise<boolean>
  onClose: () => void
  now?: number
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex justify-between gap-3 text-xs">
      <dt className="text-[var(--text-muted)]">{label}</dt>
      <dd className="m-0 text-right text-[var(--text-primary)] break-all">{children}</dd>
    </div>
  )
}

/** Everything about one host and everything you can do to it, shared by the map, the cards and the list. */
export default function HostDrawer({ host, busy, run, onClose, now }: Props) {
  const [detail, setDetail] = useState<PlatformHostDetail | null>(null)
  const [evacuate, setEvacuate] = useState(true)
  const [confirm, setConfirm] = useState<'remove' | 'fence' | null>(null)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const loadDetail = useCallback(() => {
    getPlatformHostDetail(host.id).then(setDetail).catch(() => setDetail(null))
  }, [host.id])

  useEffect(() => {
    setDetail(null)
    loadDetail()
    return () => { if (timer.current) clearTimeout(timer.current) }
  }, [loadDetail])

  const act = async (label: string, fn: () => Promise<unknown>) => {
    const ok = await run(label, fn)
    if (ok) {
      loadDetail()
      if (timer.current) clearTimeout(timer.current)
      timer.current = setTimeout(loadDetail, 6000)
    }
    return ok
  }

  const report = detail?.validation_report ?? []
  const failing = report.filter((c) => !c.passed)
  const unfence = host.fenced

  return (
    <aside aria-label={`${host.hostname} details`} data-testid="host-drawer" className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-5 flex flex-col gap-4 lg:sticky lg:top-4 self-start">
      <header className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <h2 className="m-0 text-base font-semibold text-[var(--text-primary)] truncate">{host.hostname}</h2>
          <p className="m-0 mt-0.5 text-xs font-mono text-[var(--accent)]">{host.address || '—'}</p>
        </div>
        <button type="button" aria-label="Close details" className="btn-secondary text-xs p-1.5" onClick={onClose}><X className="w-4 h-4" /></button>
      </header>

      <dl className="m-0 flex flex-col gap-1.5" data-testid="host-drawer-facts">
        <Row label="Role">{isControllerHost(host) ? 'Controller (local machine)' : 'Node'}</Row>
        <Row label="State">{host.maintenance_mode ? 'maintenance' : host.state}</Row>
        <Row label="Heartbeat">{formatAge(heartbeatAgeSecs(host.last_heartbeat_at, now))}</Row>
        <Row label="Agent">{host.agent_grpc_addr || '—'}</Row>
        {host.transport ? <Row label="Transport">{host.transport === 'mtls' ? 'mutual TLS' : host.transport}</Row> : null}
        {(detail?.agent_version || host.agent_version) ? <Row label="Agent version">{detail?.agent_version || host.agent_version}</Row> : null}
        {detail?.libvirt_version ? <Row label="libvirt / QEMU">{detail.libvirt_version} / {detail.qemu_version || '—'}</Row> : null}
        {detail?.cpu_model ? <Row label="CPU">{detail.cpu_model}</Row> : null}
        <Row label="VMs">{host.vm_count}</Row>
      </dl>

      <section aria-label="Validation" data-testid="host-drawer-validation">
        <div className="flex items-center justify-between mb-1.5">
          <h3 className="m-0 text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)]">Validation · {host.validation_status || 'pending'}</h3>
          <button type="button" className="btn-secondary text-xs" disabled={busy} onClick={() => void act('Validation queued', () => enqueueValidateHost(host.id))} data-testid="drawer-recheck">Re-check</button>
        </div>
        {report.length === 0 ? (
          <p className="m-0 text-xs text-[var(--text-muted)]">No report yet. Re-check runs the checklist against the agent.</p>
        ) : (
          <ul className="m-0 p-0 list-none flex flex-col gap-1">
            {report.map((c) => (
              <li key={c.name} className="text-xs" data-testid={`check-${c.name}`}>
                <span className="inline-flex items-start gap-1.5">
                  {c.passed ? <CheckCircle2 className="w-3.5 h-3.5 mt-px text-[var(--nl-accent-green-text)]" /> : <XCircle className="w-3.5 h-3.5 mt-px text-[var(--nl-danger)]" />}
                  <span className={c.passed ? 'text-[var(--text-secondary)]' : 'text-[var(--text-primary)]'}>{c.name}: {c.message}</span>
                </span>
                {!c.passed && c.remediation ? <p className="m-0 ml-5 text-[var(--text-muted)]">Fix: {c.remediation}</p> : null}
              </li>
            ))}
          </ul>
        )}
        {failing.length > 0 ? <p className="m-0 mt-1 text-xs text-[var(--nl-danger)]" data-testid="drawer-failing">{failing.length} check{failing.length === 1 ? '' : 's'} failing</p> : null}
      </section>

      <div className="flex flex-col gap-2">
        <Link to={`/platform/hosts/${host.id}`} className="btn-primary text-sm text-center">Open host</Link>
        <Link to={`/platform/vms?lens=topology&host=${encodeURIComponent(host.id)}`} className="btn-secondary text-xs text-center">Open in Machine Finder</Link>
        <button type="button" className="btn-secondary text-xs" disabled={busy} onClick={() => void act('Sync queued', () => syncHost(host.id))}>Sync</button>
        {host.maintenance_mode ? (
          <button type="button" className="btn-secondary text-xs" disabled={busy} onClick={() => void act('Maintenance ended', () => hostMaintenance(host.id, 'exit'))}>Exit maintenance</button>
        ) : (
          <div className="flex items-center gap-2">
            <button type="button" className="btn-secondary text-xs flex-1" disabled={busy} onClick={() => void act('Maintenance entered', () => hostMaintenance(host.id, 'enter', evacuate))}>Enter maintenance</button>
            <label className="text-xs text-[var(--text-secondary)] inline-flex items-center gap-1">
              <input type="checkbox" checked={evacuate} onChange={(e) => setEvacuate(e.target.checked)} /> Move VMs off
            </label>
          </div>
        )}
        <button type="button" className="btn-secondary text-xs" disabled={busy} onClick={() => (unfence ? void act('Host unfenced', () => fenceHost(host.id)) : setConfirm('fence'))}>{unfence ? 'Unfence' : 'Fence'}</button>
        <button type="button" className="text-xs rounded-lg border border-[var(--nl-danger)] text-[var(--nl-danger)] px-3 py-1.5 bg-transparent cursor-pointer" disabled={busy} onClick={() => setConfirm('remove')} data-testid="drawer-remove">Remove host…</button>
      </div>

      <ConfirmDialog
        open={confirm === 'fence'}
        title={`Fence ${host.hostname}?`}
        message="A fenced host gets no new VMs and no automatic actions. Use it when the machine is misbehaving."
        confirmLabel="Fence host"
        variant="warning"
        onCancel={() => setConfirm(null)}
        onConfirm={() => { setConfirm(null); void act('Host fenced', () => fenceHost(host.id)) }}
      />
      <ConfirmDialog
        open={confirm === 'remove'}
        title={`Remove ${host.hostname}?`}
        message={`This removes the host from the fleet and forgets its agent token. ${host.vm_count > 0 ? `It still has ${host.vm_count} VM${host.vm_count === 1 ? '' : 's'}; the controller will refuse unless they are gone.` : 'The machine itself is not touched.'}`}
        confirmLabel="Remove host"
        typeToMatch={host.hostname}
        onCancel={() => setConfirm(null)}
        onConfirm={() => {
          setConfirm(null)
          void act('Host removed', () => deleteHost(host.id)).then((ok) => { if (ok) onClose() })
        }}
      />
    </aside>
  )
}
