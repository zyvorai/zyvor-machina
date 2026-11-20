// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useMemo, useRef } from 'react'
import type { ServiceMapEdge, ServiceMapNode } from '../../api/platformNetworkCanvas'
import { statusToneClass } from '../../utils/semanticColors'
function nodeFill(n: ServiceMapNode): string {
  if (n.risk === 'high' || n.status === 'blocked') return 'rgba(248,113,113,0.9)'
  if (n.risk === 'medium' || n.status === 'warning') return 'rgba(251,191,36,0.9)'
  return 'rgba(52,211,153,0.9)'
}

type Props = {
  nodes: ServiceMapNode[]
  edges: ServiceMapEdge[]
  className?: string
}

function nodeKey(n: ServiceMapNode): string {
  return `${n.namespace}/${n.name}`
}

export default function NetworkServiceMapGraph({ nodes, edges, className = '' }: Props) {
  const positions = useMemo(() => {
    const map = new Map<string, { x: number; y: number }>()
    const cols = Math.max(2, Math.ceil(Math.sqrt(nodes.length)))
    nodes.forEach((n, i) => {
      const col = i % cols
      const row = Math.floor(i / cols)
      map.set(nodeKey(n), {
        x: 48 + col * (320 / Math.max(cols - 1, 1)),
        y: 36 + row * (160 / Math.max(Math.ceil(nodes.length / cols) - 1, 1)),
      })
    })
    return map
  }, [nodes])

  const svgRef = useRef<SVGSVGElement>(null)

  if (nodes.length === 0) return null

  return (
    <svg
      ref={svgRef}
      viewBox="0 0 400 200"
      className={`w-full h-48 rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-surface)] ${className}`}
      aria-label="Service map graph"
      data-testid="network-service-map-graph"
    >
      {edges.slice(0, 24).map((e) => {
        const sourceKey = e.source_key ?? e.source
        const targetKey = e.target_key ?? e.target
        const from =
          positions.get(sourceKey)
          ?? positions.get(e.source)
          ?? positions.get(e.source.split('/').slice(-1)[0] ?? '')
        const to =
          positions.get(targetKey)
          ?? positions.get(e.target)
          ?? positions.get(e.target.split('/').slice(-1)[0] ?? '')
        if (!from || !to) return null
        const blocked = e.health === 'blocked' || (e.dropped_count ?? 0) > 0
        return (
          <line
            key={e.id}
            x1={from.x}
            y1={from.y}
            x2={to.x}
            y2={to.y}
            stroke={blocked ? 'rgba(248,113,113,0.55)' : 'rgba(56,189,248,0.35)'}
            strokeWidth={1.5}
          />
        )
      })}
      {nodes.slice(0, 18).map((n) => {
        const pos = positions.get(nodeKey(n))
        if (!pos) return null
        const tone =
          n.risk === 'high' || n.status === 'blocked'
            ? statusToneClass('error')
            : n.risk === 'medium' || n.status === 'warning'
              ? statusToneClass('warn')
              : statusToneClass('ok')
        return (
          <g key={nodeKey(n)}>
            <circle cx={pos.x} cy={pos.y} r={6} fill={nodeFill(n)} />
            <text x={pos.x + 10} y={pos.y + 4} className={`${tone} text-[8px]`}>
              {n.name}
            </text>
          </g>
        )
      })}
    </svg>
  )
}
