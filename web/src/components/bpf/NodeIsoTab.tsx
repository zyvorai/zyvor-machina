// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Emergency node isolation: drop everything on the uplink except the
// allowlist, for a short mandatory lease, then fail open on its own.

import { useCallback, useEffect, useState } from 'react'
import { ShieldAlert } from 'lucide-react'
import { MacGlassPanel, MacToggle } from '../platform/mac/PlatformMacUi'
import { getBpfNodeIso, setBpfNodeIso, type BpfNodeIsoConfig } from '../../api/bpf'
import { statusPillClasses, statusToneClass } from '../../utils/semanticColors'
import { Empty, Field, Metrics, splitList, splitPorts, useBpfAction, useBpfLoad } from './shared'

const MIN_LEASE = 10
const MAX_LEASE = 900

export default function NodeIsoTab() {
  const action = useBpfAction()
  const { data: st, setData, error, reload } = useBpfLoad(useCallback(() => getBpfNodeIso(), []))
  const [c, setC] = useState<BpfNodeIsoConfig | null>(null)
  const [lease, setLease] = useState(120)
  const [tcp, setTcp] = useState('')
  const [udp, setUdp] = useState('')
  const [exempt, setExempt] = useState('')

  useEffect(() => {
    if (st && !c) {
      setC({ ...st.config, dry_run: st.attached ? st.config.dry_run : true })
      setTcp(st.config.allow_tcp.join(', '))
      setUdp(st.config.allow_udp.join(', '))
      setExempt(st.config.exempt.join('\n'))
    }
  }, [st, c])

  useEffect(() => {
    if (!st?.attached) return
    const t = setInterval(() => void reload(), 5000)
    return () => clearInterval(t)
  }, [st?.attached, reload])

  if (error) return <MacGlassPanel title="Node isolation"><Empty>{error}</Empty></MacGlassPanel>
  if (!st || !c) return null

  const allowTcp = splitPorts(tcp)
  const exemptList = splitList(exempt)
  const lockout = !c.dry_run && !allowTcp.includes(22) && exemptList.length === 0
  const arm = () => {
    if (!c.dry_run && !window.confirm(`Drop all traffic on ${c.iface} except the allowlist for ${lease}s?`)) return
    void action(
      setBpfNodeIso({ ...c, enabled: true, lease_secs: lease, allow_tcp: allowTcp, allow_udp: splitPorts(udp), exempt: exemptList }),
      c.dry_run ? `Dry run armed for ${lease}s` : `Node isolated for ${lease}s`,
    ).then((r) => { if (r) setData(r) })
  }
  const disarm = () =>
    void action(setBpfNodeIso({ ...c, enabled: false }), 'Node isolation lifted').then((r) => { if (r) setData(r) })

  return (
    <>
      <Metrics
        items={[
          { label: 'State', value: <span className={statusToneClass(st.isolating ? 'error' : st.attached ? 'warn' : 'neutral')}>{st.isolating ? 'Isolating' : st.attached ? 'Dry run' : 'Off'}</span> },
          { label: 'Lease left', value: st.lease_remaining_secs != null ? `${st.lease_remaining_secs}s` : '—' },
          { label: 'Dropped in / out', value: `${st.stats.dropped_in} / ${st.stats.dropped_out}` },
          { label: 'Would drop', value: st.stats.would_drop.toLocaleString() },
          { label: 'Passed', value: st.stats.passed.toLocaleString() },
        ]}
      />
      <MacGlassPanel
        title="Node isolation"
        subtitle={`Drops everything on the uplink except the allowlist (ARP, IPv6 ND and DHCP always pass). Lease ${MIN_LEASE}–${MAX_LEASE}s; the datapath fails open by itself when it ends.`}
        action={
          <span className={statusPillClasses(st.isolating ? 'error' : st.attached ? 'warn' : 'neutral')}>
            {st.attached ? `${st.isolating ? 'Isolating' : 'Dry run'} on ${st.attached}` : st.lease_expired ? 'Lease lapsed' : 'Off'}
          </span>
        }
      >
        <div className="space-y-4">
          <p className={`text-sm inline-flex items-center gap-2 ${statusToneClass('warn')}`}>
            <ShieldAlert className="w-4 h-4 shrink-0" />
            Start with a dry run: it counts what would be dropped without dropping anything.
          </p>
          <div className="flex flex-wrap items-end gap-4">
            <Field label="Uplink interface" htmlFor="ni-iface">
              <input id="ni-iface" className="input text-sm font-mono w-40" placeholder="eth0" value={c.iface} onChange={(e) => setC({ ...c, iface: e.target.value.trim() })} />
            </Field>
            <Field label={`Lease (s, ${MIN_LEASE}–${MAX_LEASE})`} htmlFor="ni-lease">
              <input
                id="ni-lease"
                type="number"
                min={MIN_LEASE}
                max={MAX_LEASE}
                className="input text-sm w-24"
                value={lease}
                onChange={(e) => setLease(Math.max(MIN_LEASE, Math.min(MAX_LEASE, Number(e.target.value) || MIN_LEASE)))}
              />
            </Field>
            <MacToggle label="Dry run" checked={c.dry_run} onChange={(dry_run) => setC({ ...c, dry_run })} />
            <MacToggle label="Allow ICMP" checked={c.allow_icmp} onChange={(allow_icmp) => setC({ ...c, allow_icmp })} />
          </div>
          <div className="grid gap-3 sm:grid-cols-3">
            <Field label="Allowed TCP ports (either side)" htmlFor="ni-tcp">
              <input id="ni-tcp" className="input text-sm font-mono" value={tcp} onChange={(e) => setTcp(e.target.value)} />
            </Field>
            <Field label="Allowed UDP ports" htmlFor="ni-udp">
              <input id="ni-udp" className="input text-sm font-mono" placeholder="53" value={udp} onChange={(e) => setUdp(e.target.value)} />
            </Field>
            <Field label="Exempt CIDRs (one per line)" htmlFor="ni-exempt">
              <textarea id="ni-exempt" className="input text-sm font-mono h-20" placeholder="10.0.0.0/8" value={exempt} onChange={(e) => setExempt(e.target.value)} />
            </Field>
          </div>
          {lockout && <p className={`text-xs ${statusToneClass('error')}`}>Allow TCP 22 or add an exempt CIDR, or you will lock yourself out.</p>}
          <div className="flex flex-wrap gap-2">
            <button type="button" className={c.dry_run ? 'btn-secondary text-sm' : 'btn-primary text-sm'} disabled={!c.iface || lockout} onClick={arm}>
              {st.attached ? 'Renew' : c.dry_run ? 'Start dry run' : 'Isolate node'}
            </button>
            <button type="button" className="btn-secondary text-sm" disabled={!st.attached} onClick={disarm}>
              Lift isolation
            </button>
          </div>
          {st.lease_expires_at && (
            <p className="text-xs text-[var(--text-muted)]">Fails open at {new Date(st.lease_expires_at).toLocaleTimeString()}</p>
          )}
        </div>
      </MacGlassPanel>
    </>
  )
}
