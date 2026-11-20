// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// machina-cni service load balancing on this node: Maglev tables, ClientIP
// affinity, NodePort mode and the XDP fast path.

import { useCallback } from 'react'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import { TahoeTableWrap } from '../platform/tahoe/TahoeListKit'
import { getBpfCni, getBpfCniServices } from '../../api/bpf'
import { statusPillClasses } from '../../utils/semanticColors'
import { Empty, Metrics, PROTO, headRowCls, rowCls, thCls, useBpfLoad } from './shared'

export default function ServiceLbTab() {
  const load = useCallback(() => Promise.all([getBpfCni(), getBpfCniServices()]), [])
  const { data, error } = useBpfLoad(load)
  if (error) return <MacGlassPanel title="Service LB"><Empty>{error}</Empty></MacGlassPanel>
  if (!data) return null
  const [cni, services] = data
  if (!cni.configured) {
    return (
      <MacGlassPanel title="Service LB" subtitle="machina-cni is not configured on this node">
        <Empty>Enable the <code>machina-cni</code> agent on a Kubernetes node (cluster bootstrap does this in its <code>cni</code> phase) to sync services, endpoints and NetworkPolicy here.</Empty>
      </MacGlassPanel>
    )
  }
  return (
    <>
      <Metrics
        items={[
          { label: 'Services', value: cni.services },
          { label: 'Maglev tables', value: cni.maglev_services },
          { label: 'Pod endpoints', value: cni.endpoints.length },
          { label: 'NodePort', value: `${cni.lb_mode.toUpperCase()}${cni.xdp ? ' · XDP' : ''}` },
          { label: 'Identities', value: cni.identities },
        ]}
      />
      <MacGlassPanel
        title="Services"
        subtitle={`Socket-level LB on connect(); 2+ backends get a Maglev table (consistent across nodes). Node ${cni.node_addr ?? '—'}${cni.node_addr6 ? ` / ${cni.node_addr6}` : ''}${cni.uplink ? ` · uplink ${cni.uplink}` : ''}${cni.last_sync ? ` · synced ${new Date(cni.last_sync).toLocaleTimeString()}` : ''}`}
      >
        {services.length === 0 ? (
          <Empty>No services synced.</Empty>
        ) : (
          <TahoeTableWrap>
            <table className="w-full text-xs" aria-label="CNI services">
              <thead>
                <tr className={headRowCls}>
                  <th scope="col" className={thCls}>Service</th>
                  <th scope="col" className={thCls}>Frontend</th>
                  <th scope="col" className={thCls}>Backends</th>
                  <th scope="col" className={thCls}>Selection</th>
                  <th scope="col" className="py-2">Affinity</th>
                </tr>
              </thead>
              <tbody>
                {services.map((s) => (
                  <tr key={`${s.addr}-${s.port}-${s.proto}`} className={rowCls}>
                    <td className="py-2 pr-2">{s.name ?? '—'}</td>
                    <td className="py-2 pr-2 font-mono">{s.addr.includes(':') ? `[${s.addr}]` : s.addr}:{s.port}/{PROTO[s.proto] ?? s.proto}</td>
                    <td className="py-2 pr-2 font-mono">
                      {s.backends.length === 0 ? <span className="text-[var(--text-muted)]">none</span> : s.backends.map((b) => (
                        <div key={`${b.addr}:${b.port}`}>
                          {b.addr}:{b.port}
                          {b.remote ? <span className="text-[var(--text-muted)]"> remote{b.node ? ` via ${b.node}` : ''}</span> : null}
                        </div>
                      ))}
                    </td>
                    <td className="py-2 pr-2">
                      <span className={statusPillClasses(s.maglev ? 'info' : 'neutral')}>{s.maglev ? 'Maglev' : s.backends.length > 1 ? 'random' : 'single'}</span>
                    </td>
                    <td className="py-2">
                      {s.affinity_secs ? `ClientIP ${s.affinity_secs}s · ${s.affinity_entries} pinned` : <span className="text-[var(--text-muted)]">none</span>}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        )}
      </MacGlassPanel>
    </>
  )
}
