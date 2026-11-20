// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Service map graph from flow history edges: endpoints, merged links and a
// left-to-right layered layout (clients → servers → external destinations).

import type { VmFlowEdge, VmFlowL7Stat } from '../api/vmNetpol'

export type Tone = 'ok' | 'warn' | 'error'

export interface MapNode {
  id: string
  label: string
  kind: 'vm' | 'external' | 'world'
  /** 0..1 across the canvas. */
  x: number
  /** Pixels. */
  y: number
}

export interface MapLink {
  id: string
  src: string
  dst: string
  tone: Tone
  count: number
  ports: string[]
  edges: VmFlowEdge[]
  l7: VmFlowL7Stat[]
}

export interface ServiceMapGraph {
  nodes: MapNode[]
  links: MapLink[]
  height: number
  /** External addresses folded into "other". */
  collapsed: number
}

const MAX_EXTERNAL = 12
const ROW = 56
const TOP = 40

export function edgeTone(e: VmFlowEdge): Tone {
  if (e.verdict === 'DROPPED') return 'error'
  if (e.drop_reason) return 'warn'
  return 'ok'
}

const RANK: Record<Tone, number> = { ok: 0, warn: 1, error: 2 }

export function buildServiceMap(edges: VmFlowEdge[]): ServiceMapGraph {
  const vms = new Set<string>()
  for (const e of edges) {
    if (e.src_vm) vms.add(e.src_vm)
    if (e.dst_vm) vms.add(e.dst_vm)
  }
  const external = new Map<string, number>()
  const entity = new Map<string, string>()
  for (const e of edges) {
    for (const ep of [e.src, e.dst]) {
      if (!vms.has(ep)) external.set(ep, (external.get(ep) ?? 0) + e.count)
    }
    if (e.src_entity === 'host' || e.src_entity === 'remote-node') entity.set(e.src, e.src_entity)
    if (e.dst_entity === 'host' || e.dst_entity === 'remote-node') entity.set(e.dst, e.dst_entity)
  }
  const keep = new Set(
    [...external.entries()].sort((a, b) => b[1] - a[1]).slice(0, MAX_EXTERNAL).map(([k]) => k),
  )
  const collapsed = external.size - keep.size
  const node = (ep: string) => (vms.has(ep) || keep.has(ep) ? ep : 'other')

  type Acc = { link: MapLink; egress: number; ingress: number; l7: Map<string, VmFlowL7Stat> }
  const acc = new Map<string, Acc>()
  for (const e of edges) {
    const src = node(e.src)
    const dst = node(e.dst)
    const id = `${src}→${dst}`
    let a = acc.get(id)
    if (!a) {
      a = { link: { id, src, dst, tone: 'ok', count: 0, ports: [], edges: [], l7: [] }, egress: 0, ingress: 0, l7: new Map() }
      acc.set(id, a)
    }
    a.link.edges.push(e)
    if (e.direction === 'egress') a.egress += e.count
    else a.ingress += e.count
    const tone = edgeTone(e)
    if (RANK[tone] > RANK[a.link.tone]) a.link.tone = tone
    const port = `${e.port}/${e.proto.toLowerCase()}`
    if (!a.link.ports.includes(port)) a.link.ports.push(port)
    for (const s of e.l7 ?? []) {
      const k = `${s.kind} ${s.request}`
      const m = a.l7.get(k)
      if (!m) {
        a.l7.set(k, { ...s, status: { ...(s.status ?? {}) } })
        continue
      }
      m.count += s.count
      m.denied += s.denied
      m.latency_n += s.latency_n
      m.latency_ms_total += s.latency_ms_total
      m.latency_ms_max = Math.max(m.latency_ms_max, s.latency_ms_max)
      for (const [c, n] of Object.entries(s.status ?? {})) m.status![c] = (m.status![c] ?? 0) + n
    }
  }
  const links = [...acc.values()].map((a) => {
    // A VM-to-VM connection is recorded at both taps; count it once.
    a.link.count = Math.max(a.egress, a.ingress)
    a.link.ports.sort((x, y) => parseInt(x, 10) - parseInt(y, 10))
    a.link.l7 = [...a.l7.values()].sort((x, y) => y.count - x.count)
    a.link.edges.sort((x, y) => y.count - x.count)
    return a.link
  })
  links.sort((a, b) => b.count - a.count)

  const out = new Map<string, number>()
  const inn = new Map<string, number>()
  for (const l of links) {
    out.set(l.src, (out.get(l.src) ?? 0) + 1)
    inn.set(l.dst, (inn.get(l.dst) ?? 0) + 1)
  }
  const ids = new Set(links.flatMap((l) => [l.src, l.dst]))
  // VM rank = longest client → server chain into it; capped so cycles settle.
  const rank = new Map<string, number>([...vms].filter((v) => ids.has(v)).map((v) => [v, 0]))
  const vmLinks = links.filter((l) => vms.has(l.src) && vms.has(l.dst) && l.src !== l.dst)
  for (let i = 0; i < rank.size; i++) {
    let moved = false
    for (const l of vmLinks) {
      const want = rank.get(l.src)! + 1
      if (want > rank.get(l.dst)! && want < rank.size) {
        rank.set(l.dst, want)
        moved = true
      }
    }
    if (!moved) break
  }
  const maxRank = Math.max(0, ...rank.values())
  const column = (id: string): number => {
    if (vms.has(id)) return 1 + rank.get(id)!
    const o = out.get(id) ?? 0
    const i = inn.get(id) ?? 0
    return o > 0 && i === 0 ? 0 : maxRank + 2
  }
  const cols = new Map<number, string[]>()
  for (const id of [...ids].sort()) {
    const c = column(id)
    cols.set(c, [...(cols.get(c) ?? []), id])
  }
  const used = [...cols.keys()].sort((a, b) => a - b)
  const nodes: MapNode[] = []
  used.forEach((c, i) => {
    const x = used.length === 1 ? 0.5 : 0.1 + (0.8 * i) / (used.length - 1)
    cols.get(c)!.forEach((id, row) => {
      nodes.push({
        id,
        label: entity.has(id) ? `${entity.get(id)} · ${id}` : id,
        kind: vms.has(id) ? 'vm' : id === 'other' ? 'world' : 'external',
        x,
        y: TOP + row * ROW,
      })
    })
  })
  const rows = Math.max(1, ...[...cols.values()].map((v) => v.length))
  return { nodes, links, height: TOP * 2 + (rows - 1) * ROW, collapsed }
}
