// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Globe } from 'lucide-react'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import PageLayout from '../components/PageLayout'
import { listNetworks, type NativeNetwork } from '../api/nativeNetworks'
import { listPorts, type NativePort } from '../api/nativePorts'
import { listVms, type NativeVm } from '../api/nativeVms'
import { formatUserError } from '../utils/apiError'

type NodeKind = 'network' | 'port' | 'instance'
interface Node { id: string; label: string; kind: NodeKind; status?: string }
interface Edge { from: string; to: string }

const KIND_COL: Record<NodeKind, number> = { network: 40, port: 400, instance: 760 }
const KIND_COLOR: Record<NodeKind, string> = { network: 'var(--accent)', port: 'var(--text-muted)', instance: 'var(--verdant)' }

function layoutNodes(nodes: Node[]): (Node & { x: number; y: number })[] {
  const counters: Record<number, number> = {}
  return nodes.map((n) => {
    const col = KIND_COL[n.kind]
    const idx = counters[col] ?? 0
    counters[col] = idx + 1
    return { ...n, x: col, y: 40 + idx * 72 }
  })
}

// Native topology — networks/ports/instances only; no old external-cloud gate
// component to gate it behind any more (the daemon's external-cloud-client
// integration has since been fully removed). Neutron subnets/routers/floating-
// IPs have no native equivalent, so those node kinds are simply absent rather
// than faked.
export default function FleetCloudTopologyPage() {
  return <FleetCloudTopologyContent />
}

function FleetCloudTopologyContent() {
  const [networks, setNetworks] = useState<NativeNetwork[]>([])
  const [ports, setPorts] = useState<NativePort[]>([])
  const [vms, setVms] = useState<NativeVm[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const [nets, prts, machines] = await Promise.all([listNetworks(), listPorts(), listVms()])
      setNetworks(nets)
      setPorts(prts)
      setVms(machines)
    } catch (e: unknown) {
      setError(formatUserError(e))
      setNetworks([])
      setPorts([])
      setVms([])
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const { nodes, edges } = useMemo(() => {
    const n: Node[] = []
    const e: Edge[] = []
    for (const net of networks) n.push({ id: `net-${net.id}`, label: net.name, kind: 'network' })
    for (const port of ports) {
      n.push({ id: `port-${port.id}`, label: port.mac_address || port.id.slice(0, 8), kind: 'port', status: port.status })
      e.push({ from: `net-${port.network_id}`, to: `port-${port.id}` })
      if (port.vm_id) e.push({ from: `port-${port.id}`, to: `vm-${port.vm_id}` })
    }
    const vmIdsWithPorts = new Set(ports.filter((p) => p.vm_id).map((p) => p.vm_id))
    for (const vm of vms) {
      if (vmIdsWithPorts.has(vm.id)) n.push({ id: `vm-${vm.id}`, label: vm.name, kind: 'instance' })
    }
    return { nodes: n, edges: e }
  }, [networks, ports, vms])

  const laid = useMemo(() => layoutNodes(nodes), [nodes])
  const pos = useMemo(() => Object.fromEntries(laid.map((n) => [n.id, n])), [laid])
  const maxY = Math.max(200, ...laid.map((n) => n.y + 48))

  return (
    <PageLayout
      prepend={<FleetCloudSubNav />}
      title="Network topology"
      icon={<Globe className="w-7 h-7 text-[var(--accent)]" />}
      error={error}
      errorTitle="Failed to load topology"
      technicalDetail={error}
      errorTone="red"
      onErrorRetry={() => void load()}
      contentLoading={loading}
    >
      {!loading && laid.length === 0 && !error && (
        <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-8 text-center text-sm text-[var(--text-muted)]">
          No networks or connected instances to graph yet.
        </div>
      )}
      {!loading && laid.length > 0 && (
        <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 overflow-x-auto">
          <svg width="960" height={maxY} className="w-full min-w-[640px]" viewBox={`0 0 960 ${maxY}`}>
            {edges.map((e, i) => {
              const a = pos[e.from]
              const b = pos[e.to]
              if (!a || !b) return null
              return (
                <line key={`${e.from}-${e.to}-${i}`} x1={a.x + 90} y1={a.y + 24} x2={b.x + 90} y2={b.y + 24}
                  stroke="var(--apple-hairline)" strokeWidth={1.5} />
              )
            })}
            {laid.map((n) => (
              <g key={n.id} transform={`translate(${n.x}, ${n.y})`}>
                <rect width={180} height={48} rx={8} fill="var(--apple-surface-elevated)" stroke={KIND_COLOR[n.kind]} strokeWidth={1.5} />
                <text x={10} y={20} fill="var(--text-primary)" fontSize={11} fontWeight={600}>{n.label.slice(0, 22)}</text>
                <text x={10} y={36} fill="var(--text-muted)" fontSize={9}>{n.kind}{n.status ? ` · ${n.status}` : ''}</text>
              </g>
            ))}
          </svg>
        </div>
      )}
      <FleetCloudFooter />
    </PageLayout>
  )
}
