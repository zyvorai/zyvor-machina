// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type React from 'react'

interface GraphNode {
  pid: number
  binary?: string
  ppid?: number
}

interface GraphEdge {
  from: number
  to: number
  binary?: string
}

interface ProcessGraphData {
  nodes?: GraphNode[]
  edges?: GraphEdge[]
  ancestry?: GraphNode[]
  children?: GraphNode[]
  pid?: number
}

export default function ProcessGraphCanvas({ data, className = '' }: { data: ProcessGraphData | null; className?: string }) {
  if (!data) {
    return <p className="text-sm text-[var(--text-muted)]">No process graph data.</p>
  }

  const ancestry = data.ancestry ?? []
  const children = data.children ?? []
  const nodes = data.nodes ?? []
  const edges = data.edges ?? []

  if (ancestry.length > 0) {
    return (
      <div className={`space-y-1 font-mono text-sm ${className}`}>
        {ancestry.map((n, i) => (
          <div key={n.pid} className="flex items-center gap-2" style={{ paddingLeft: `${i * 16}px` }}>
            {i > 0 && <span className="text-[var(--text-faint)]">└──</span>}
            <span className="text-[var(--text-primary)]">{n.binary || `pid ${n.pid}`}</span>
            <span className="text-[var(--text-muted)] text-xs">({n.pid})</span>
          </div>
        ))}
        {children.length > 0 && (
          <div className="mt-3 pt-3 border-t border-[var(--apple-hairline)]">
            <p className="text-xs text-[var(--text-muted)] mb-2">Children</p>
            {children.map((c) => (
              <div key={c.pid} className="text-[var(--text-secondary)] pl-4">
                └── {c.binary || `pid ${c.pid}`} ({c.pid})
              </div>
            ))}
          </div>
        )}
      </div>
    )
  }

  if (nodes.length === 0 && edges.length === 0) {
    return <p className="text-sm text-[var(--text-muted)]">No process relationships recorded yet.</p>
  }

  const roots = nodes.filter((n) => !edges.some((e) => e.to === n.pid))
  const childMap = new Map<number, GraphEdge[]>()
  for (const e of edges) {
    const list = childMap.get(e.from) ?? []
    list.push(e)
    childMap.set(e.from, list)
  }

  function renderTree(pid: number, depth: number, seen: Set<number>): React.ReactNode {
    if (seen.has(pid) || depth > 8) return null
    seen.add(pid)
    const node = nodes.find((n) => n.pid === pid)
    const kids = childMap.get(pid) ?? []
    return (
      <div key={`${pid}-${depth}`}>
        <div className="flex items-center gap-2" style={{ paddingLeft: `${depth * 16}px` }}>
          {depth > 0 && <span className="text-[var(--text-faint)]">└──</span>}
          <span className="text-[var(--text-primary)]">{node?.binary || `pid ${pid}`}</span>
        </div>
        {kids.map((e) => renderTree(e.to, depth + 1, seen))}
      </div>
    )
  }

  return (
    <div className={`font-mono text-sm space-y-0.5 ${className}`}>
      {roots.map((r) => renderTree(r.pid, 0, new Set()))}
    </div>
  )
}
