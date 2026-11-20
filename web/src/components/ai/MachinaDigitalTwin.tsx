// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { GitBranch, Zap } from 'lucide-react'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import {
  analyzeTwinImpact,
  getDigitalTwinGraph,
  simulateTwinBatch,
  type DigitalTwinGraph,
  type ImpactAnalysis,
} from '../../api/ai'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, riskTone, statusBadgeClasses, statusToneClass } from '../../utils/semanticColors'

export default function MachinaDigitalTwin() {
  const [graph, setGraph] = useState<DigitalTwinGraph | null>(null)
  const [impact, setImpact] = useState<(ImpactAnalysis & { estimated_downtime_sec?: number; vms_at_risk?: number; storage_unavailable_gib?: number }) | null>(null)
  const [hostId, setHostId] = useState('')
  const [simAction, setSimAction] = useState<'shutdown' | 'migrate' | 'isolate' | 'failure'>('shutdown')
  const [simKind, setSimKind] = useState<'host' | 'network' | 'storage' | 'switch' | 'vm'>('host')
  const [busy, setBusy] = useState(false)
  const [batchBusy, setBatchBusy] = useState(false)
  const [batchResults, setBatchResults] = useState<Array<ImpactAnalysis & { estimated_downtime_sec?: number; vms_at_risk?: number }>>([])
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    setError(null)
    try {
      const g = await getDigitalTwinGraph()
      setGraph(g)
      const firstHost = g.nodes.find((n) => n.kind === 'host')
      const firstNet = g.nodes.find((n) => n.kind === 'network')
      const firstStorage = g.nodes.find((n) => n.kind === 'storage')
      if (firstHost && !hostId) setHostId(firstHost.name)
      else if (firstNet && !hostId) {
        setSimKind('network')
        setHostId(firstNet.name)
      } else if (firstStorage && !hostId) {
        setSimKind('storage')
        setHostId(firstStorage.name)
      }
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }, [hostId])

  useEffect(() => { void load() }, [load])

  const scenarioAction = () =>
    simKind === 'network' || simKind === 'switch' ? 'isolate' : simKind === 'storage' ? 'drain' : simAction === 'failure' ? 'shutdown' : simAction

  const simulate = async () => {
    if (!hostId.trim()) return
    setBusy(true)
    setError(null)
    setBatchResults([])
    try {
      const result = await analyzeTwinImpact({
        action: scenarioAction(),
        target_kind: simKind,
        target_id: hostId.trim(),
      })
      setImpact(result)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const simulateBatch = async () => {
    if (!hostId.trim()) return
    setBatchBusy(true)
    setError(null)
    try {
      const scenarios: Array<{ action: string; target_kind: string; target_id: string }> = [
        { action: scenarioAction(), target_kind: simKind, target_id: hostId.trim() },
      ]
      if (simKind === 'host' && networks[0]) {
        scenarios.push({ action: 'isolate', target_kind: 'network', target_id: networks[0].name })
      }
      const result = await simulateTwinBatch(scenarios)
      setBatchResults(result.results)
      if (result.results[0]) setImpact(result.results[0])
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setBatchBusy(false)
    }
  }

  const hosts = graph?.nodes.filter((n) => n.kind === 'host') ?? []
  const networks = graph?.nodes.filter((n) => n.kind === 'network') ?? []
  const storages = graph?.nodes.filter((n) => n.kind === 'storage') ?? []
  const switches = graph?.nodes.filter((n) => n.kind === 'switch') ?? []
  const targets = simKind === 'network' ? networks : simKind === 'storage' ? storages : simKind === 'switch' ? switches : hosts

  return (
    <MacGlassPanel
      title="Infrastructure Digital Twin"
      subtitle="Live datacenter graph — simulate blast radius before changes"
    >
      <div className="flex flex-wrap items-end gap-3 text-sm">
        <label className="block">
          <span className="text-xs text-[var(--text-muted)]">Target</span>
          <select className="input mt-1 block text-xs" value={simKind} onChange={(e) => setSimKind(e.target.value as 'host' | 'network' | 'storage' | 'switch')}>
            <option value="host">Host</option>
            <option value="network">Network</option>
            <option value="storage">Storage pool</option>
            <option value="switch">Switch (LLDP)</option>
          </select>
        </label>
        {simKind === 'host' && (
          <label className="block">
            <span className="text-xs text-[var(--text-muted)]">Action</span>
            <select className="input mt-1 block text-xs" value={simAction} onChange={(e) => setSimAction(e.target.value as 'shutdown' | 'migrate' | 'isolate' | 'failure')}>
              <option value="shutdown">Shutdown</option>
              <option value="failure">Host failure</option>
              <option value="migrate">Evacuate / migrate</option>
            </select>
          </label>
        )}
        <label className="block">
          <span className="text-xs text-[var(--text-muted)]">{simKind === 'network' ? 'Network to isolate' : simKind === 'storage' ? 'Storage pool' : 'Host'}</span>
          <select className="input mt-1 block min-w-[12rem]" value={hostId} onChange={(e) => setHostId(e.target.value)}>
            {targets.map((h) => (
              <option key={h.id} value={h.name}>{h.name} {h.state ? `(${h.state})` : ''}</option>
            ))}
          </select>
        </label>
        <button
          type="button"
          className="btn-primary text-xs"
          disabled={busy || batchBusy}
          data-testid="twin-impact-analyze"
          onClick={() => void simulate()}
        >
          {busy ? 'Simulating…' : 'What breaks?'}
        </button>
        <button
          type="button"
          className="btn-secondary text-xs"
          disabled={busy || batchBusy}
          data-testid="twin-batch-simulate"
          onClick={() => void simulateBatch()}
        >
          {batchBusy ? 'Batch…' : 'Batch simulate'}
        </button>
        {graph && (
          <span className="text-xs text-[var(--text-muted)] flex items-center gap-1">
            <GitBranch className="w-3 h-3" /> {graph.node_count} nodes · {graph.edge_count} edges
          </span>
        )}
      </div>
      {error && <p className={`text-xs mt-2 ${statusToneClass('error')}`}>{error}</p>}
      {batchResults.length > 1 && (
        <div className="mt-3 space-y-2">
          <p className="text-xs text-[var(--text-muted)] uppercase tracking-wide">Batch scenarios ({batchResults.length})</p>
          {batchResults.map((r, i) => (
            <p key={`${r.target}-${i}`} className="text-xs text-[var(--text-muted)]">
              {i + 1}. {r.summary}
            </p>
          ))}
        </div>
      )}
      {impact && (
        <div className="mt-4 rounded-xl border border-white/[0.06] bg-[var(--apple-surface)] p-4 space-y-2 text-sm">
          <p className="flex items-center gap-2 font-medium text-[var(--text-primary)]">
            <Zap className="w-4 h-4 text-orange-400" />
            {impact.summary}
            <span className={`text-[10px] uppercase px-2 py-0.5 rounded-full ${statusBadgeClasses(riskTone(impact.severity))}`}>{impact.severity}</span>
          </p>
          {impact.affected_vms.length > 0 && (
            <p className="text-[var(--text-muted)] text-xs">VMs affected: {impact.affected_vms.join(', ')}</p>
          )}
          {impact.affected_applications.length > 0 && (
            <p className="text-[var(--text-muted)] text-xs">Applications: {impact.affected_applications.join(', ')}</p>
          )}
          {(impact.vms_at_risk ?? 0) > 0 && (
            <p className="text-[var(--text-muted)] text-xs">VMs at risk: {impact.vms_at_risk} · est. downtime {impact.estimated_downtime_sec ?? 0}s</p>
          )}
          {(impact.storage_unavailable_gib ?? 0) > 0 && (
            <p className="text-[var(--text-muted)] text-xs">Storage unavailable: {impact.storage_unavailable_gib} GiB</p>
          )}
          {impact.recommendations.map((r) => (
            <p key={r} className={`text-xs ${hubLinkClasses()}`}>→ {r}</p>
          ))}
        </div>
      )}
    </MacGlassPanel>
  )
}
