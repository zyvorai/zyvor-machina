// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Fleet network canvas — Machina topology + native eBPF flows from every host's machina-bpfd.

import { platformFetch, type TopologyGraph } from './platform'
import type { NativeBpfStatus } from './zeusSecurity'

/** One tracked connection from machina-bpfd's flow table. */
export type BpfFlow = {
  host_id?: string
  hostname?: string
  iface?: string
  vm?: string | null
  proto?: string
  local?: string
  local_port?: number
  remote?: string
  remote_port?: number
  /** `local` = the workload opened it, `remote` = inbound. */
  origin?: string
  tx_bytes?: number
  rx_bytes?: number
  tx_pkts?: number
  rx_pkts?: number
  first_seen?: string
  last_seen?: string
  verdict?: string
}

export type ServiceMapNode = {
  name: string
  namespace: string
  status?: string
  connections_in?: number
  connections_out?: number
  blocked_flows?: number
  risk?: string
}

export type ServiceMapEdge = {
  id: string
  source: string
  target: string
  source_key?: string
  target_key?: string
  health?: string
  flow_count?: number
  dropped_count?: number
  policy?: string
  is_external?: boolean
}

export type NetworkCanvasPayload = {
  topology: TopologyGraph
  flows: { flows?: BpfFlow[]; total?: number; note?: string }
  flow_stats: {
    total_flows?: number
    tx_bytes?: number
    rx_bytes?: number
    denied_flows?: number
    by_proto?: Record<string, number>
    top_talkers?: Array<{ name: string; tx_bytes: number; rx_bytes: number; flows: number }>
  }
  anomalies: { anomalies?: Array<{ summary?: string; description?: string; severity?: string; host_id?: string; kind?: string; ts?: string }>; note?: string }
  native_bpf: NativeBpfStatus
  network_pulse: {
    enabled?: boolean
    overview?: Record<string, unknown>
    service_map?: {
      nodes?: ServiceMapNode[]
      edges?: ServiceMapEdge[]
      meta?: { stats?: { services?: number; connections?: number; blocked?: number; warnings?: number } }
      overlays?: { top_talker_nodes?: string[]; attack_path_workloads?: string[] }
    }
    workloads?: { workloads?: Array<{ namespace: string; name: string; status?: string }> }
    timeline?: { events?: Array<{ summary?: string; message?: string; severity?: string; timestamp?: string; kind?: string }> }
    threats?: { threats?: Array<{ title?: string; description?: string; severity?: string; summary?: string; kind?: string }> }
    top_talkers?: { talkers?: Array<{ name?: string; flows?: number; tx_bytes?: number; rx_bytes?: number }> }
    k8s_nodes?: { nodes?: Array<{ name: string; status: string; pods_count?: number }> }
    flow_stats?: Record<string, unknown>
    anomalies?: { anomalies?: Array<{ summary?: string; description?: string }> }
    note?: string
  }
}

export const getNetworkCanvas = () =>
  platformFetch<NetworkCanvasPayload>('/api/v1/network-canvas')
