// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { GitBranch, Search } from 'lucide-react'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import MachinaExplainObjectPanel from './MachinaExplainObjectPanel'
import InfraGraphScrubberPanel from './InfraGraphScrubberPanel'
import {
  explainInfraPath,
  getInfraGraph,
  queryInfraGraph,
  type InfraGraph,
  type PathResult,
} from '../../api/ai'
import { formatUserError } from '../../utils/apiError'
import { statusToneClass } from '../../utils/semanticColors'

export default function MachinaInfraGraphBrain() {
  const [graph, setGraph] = useState<InfraGraph | null>(null)
  const [pathFrom, setPathFrom] = useState('')
  const [pathTo, setPathTo] = useState('')
  const [port, setPort] = useState('')
  const [path, setPath] = useState<PathResult | null>(null)
  const [search, setSearch] = useState('Ubuntu VMs over 8GB RAM')
  const [searchHits, setSearchHits] = useState<Array<{ kind: string; id: string; name: string; detail: string }>>([])
  const [selected, setSelected] = useState<{ kind: string; id: string; name: string } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    setError(null)
    try {
      setGraph(await getInfraGraph())
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const runPath = async () => {
    if (!pathFrom.trim() || !pathTo.trim()) return
    setBusy(true)
    setError(null)
    try {
      setPath(await explainInfraPath({
        from: pathFrom.trim(),
        to: pathTo.trim(),
        port: port ? Number(port) : undefined,
      }))
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const runSearch = async () => {
    setBusy(true)
    try {
      const r = await queryInfraGraph(search)
      setSearchHits(r.hits.map((h) => ({ kind: h.kind, id: h.id, name: h.name, detail: h.detail })))
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const explainNode = (kind: string, id: string, name: string) => {
    setSelected({ kind, id, name })
  }

  return (
    <MacGlassPanel title="Infrastructure Graph Brain" className="space-y-4">
      {error && <p className={`text-sm ${statusToneClass('error')}`}>{error}</p>}
      {graph && (
        <p className="text-xs text-[var(--text-muted)] flex items-center gap-2">
          <GitBranch className="w-4 h-4" />
          {graph.node_count} nodes · {graph.edge_count} edges (hosts, VMs, storage, networks, firewall, backups, apps)
        </p>
      )}

      <InfraGraphScrubberPanel hours={4} />

      {selected && (
        <MachinaExplainObjectPanel
          key={`${selected.kind}-${selected.id}`}
          kind={selected.kind}
          id={selected.id}
          name={selected.name}
          onClose={() => setSelected(null)}
        />
      )}

      <div className="space-y-2">
        <p className="text-xs font-medium text-[var(--text-muted)]">Graph nodes (click to explain)</p>
        <ul className="text-xs max-h-28 overflow-y-auto space-y-0.5">
          {(graph?.nodes ?? []).filter((n) => n.kind === 'vm' || n.kind === 'host').slice(0, 20).map((n) => (
            <li key={`${n.kind}-${n.id}`}>
              <button type="button" className="text-[var(--text-muted)] hover:text-orange-600" onClick={() => explainNode(n.kind, n.id, n.name)}>
                <span className="text-[var(--text-faint)]">{n.kind}</span> {n.name}
                {n.health_score != null ? ` · ${n.health_score}` : ''}
              </button>
            </li>
          ))}
        </ul>
      </div>

      <div className="space-y-2">
        <p className="text-xs font-medium text-[var(--text-muted)]">Connectivity path</p>
        <div className="flex flex-wrap gap-2">
          <input aria-label="From VM" className="input text-sm flex-1 min-w-[8rem]" placeholder="From VM" value={pathFrom} onChange={(e) => setPathFrom(e.target.value)} />
          <input aria-label="To VM" className="input text-sm flex-1 min-w-[8rem]" placeholder="To VM" value={pathTo} onChange={(e) => setPathTo(e.target.value)} />
          <input aria-label="Port" className="input text-sm w-20" placeholder="Port" value={port} onChange={(e) => setPort(e.target.value)} />
          <button type="button" className="btn-secondary text-sm" disabled={busy} onClick={() => void runPath()}>Analyze</button>
        </div>
        {path && (
          <div className="rounded-lg border border-white/10 bg-[var(--apple-surface)] p-3 text-sm space-y-2">
            <p className={path.can_reach ? 'text-emerald-600' : 'text-amber-600'}>{path.explanation}</p>
            <p className="text-xs text-[var(--text-muted)]">Hops: {path.hops.join(' → ')}</p>
            {path.blockers.map((b) => (
              <p key={b.kind} className="text-xs text-amber-700/90">{b.message} — {b.remediation}</p>
            ))}
            {(path.evidence ?? []).map((ev) => (
              <p key={ev.detail} className="text-xs text-[var(--text-muted)]">[{ev.source}] {ev.detail}</p>
            ))}
          </div>
        )}
      </div>

      <div className="space-y-2">
        <p className="text-xs font-medium text-[var(--text-muted)] flex items-center gap-1"><Search className="w-3.5 h-3.5" /> Infrastructure search</p>
        <div className="flex gap-2">
          <input aria-label="Infrastructure search query" className="input text-sm flex-1" value={search} onChange={(e) => setSearch(e.target.value)} />
          <button type="button" className="btn-secondary text-sm" disabled={busy} onClick={() => void runSearch()}>Search</button>
        </div>
        <ul className="text-xs text-[var(--text-muted)] space-y-1 max-h-32 overflow-y-auto">
          {searchHits.map((h) => (
            <li key={`${h.kind}-${h.id}`}>
              <button type="button" className="hover:text-orange-600" onClick={() => explainNode(h.kind, h.id, h.name)}>
                <span className="text-[var(--text-muted)]">{h.kind}</span> {h.name} — {h.detail}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </MacGlassPanel>
  )
}
