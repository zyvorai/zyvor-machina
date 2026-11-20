// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Lateral-movement detections from each host's flow history: port scans,
// host sweeps, deny bursts and first-time VM pairs.

import { useCallback, useEffect, useState } from 'react'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import { TahoeTableWrap } from '../platform/tahoe/TahoeListKit'
import { Empty, headRowCls, rowCls, thCls } from '../bpf/shared'
import { listFlowAlerts, type NetpolScope, type VmFlowAlert } from '../../api/vmNetpol'
import { formatUserError } from '../../utils/apiError'
import { statusPillClasses } from '../../utils/semanticColors'

const KIND_LABEL: Record<string, string> = {
  port_scan: 'Port scan',
  host_sweep: 'Host sweep',
  deny_burst: 'Deny burst',
  new_peer: 'New peer',
  threat_domain: 'Threat domain',
  new_domain: 'New domain',
}

const tone = (s: string) => (s === 'high' ? 'error' : s === 'medium' ? 'warn' : 'info')

export default function FlowAlerts({ scope, onQuarantine }: { scope: NetpolScope; onQuarantine?: (vm: string) => void }) {
  const [items, setItems] = useState<VmFlowAlert[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const load = useCallback(async () => {
    try {
      setItems(await listFlowAlerts(scope, 500))
      setError(null)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [scope])

  useEffect(() => {
    void load()
    const t = window.setInterval(() => void load(), 15000)
    return () => window.clearInterval(t)
  }, [load])

  return (
    <MacGlassPanel
      title="Alerts"
      subtitle="Detected from VM traffic over a 60-second window: 20+ ports on one target (port scan), 20+ targets on one port (host sweep), 50+ denied flows (deny burst), and VM pairs talking for the first time once a day of history exists. Fleet alerts are also sent to webhooks and SIEM as netpol.alert events; a fleet port scan or host sweep also proposes quarantining the source VM in Approvals."
    >
      {error ? (
        <Empty>{error}</Empty>
      ) : loading ? (
        <Empty>Loading…</Empty>
      ) : items.length === 0 ? (
        <Empty>No alerts. Detection runs on every host with flow logging on.</Empty>
      ) : (
        <TahoeTableWrap>
          <table className="w-full text-xs" aria-label="Flow alerts">
            <thead>
              <tr className={headRowCls}>
                <th scope="col" className={thCls}>Time</th>
                <th scope="col" className={thCls}>Severity</th>
                <th scope="col" className={thCls}>Kind</th>
                <th scope="col" className={thCls}>Source</th>
                {scope === 'fleet' && <th scope="col" className={thCls}>Host</th>}
                <th scope="col" className={onQuarantine ? thCls : 'py-2'}>Detail</th>
                {onQuarantine && <th scope="col" className="py-2"><span className="sr-only">Actions</span></th>}
              </tr>
            </thead>
            <tbody>
              {items.map((a, i) => (
                <tr key={`${a.ts}|${a.kind}|${a.src}|${i}`} className={rowCls}>
                  <td className="py-2 pr-2 text-[var(--text-muted)] whitespace-nowrap">{a.ts}</td>
                  <td className="py-2 pr-2"><span className={statusPillClasses(tone(a.severity))}>{a.severity}</span></td>
                  <td className="py-2 pr-2">{KIND_LABEL[a.kind] ?? a.kind}</td>
                  <td className="py-2 pr-2 font-mono">{a.src_vm ?? a.src}</td>
                  {scope === 'fleet' && <td className="py-2 pr-2">{a.host ?? '—'}</td>}
                  <td className={onQuarantine ? 'py-2 pr-2' : 'py-2'}>{a.detail}</td>
                  {onQuarantine && (
                    <td className="py-2 text-right whitespace-nowrap">
                      {a.src_vm && a.severity !== 'low' && (
                        <button type="button" className="btn-secondary text-xs" aria-label={`Quarantine ${a.src_vm}`} onClick={() => onQuarantine(a.src_vm as string)}>
                          Quarantine
                        </button>
                      )}
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        </TahoeTableWrap>
      )}
    </MacGlassPanel>
  )
}
