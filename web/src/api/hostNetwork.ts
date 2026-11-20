// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { readJsonArray, readJsonObject, apiPost, apiDelete } from './client'

const API = '/api/v1'

export interface HostInterface {
  name: string
  iface_type: string
  state: string
  mac: string
  ipv4: string[]
  mtu: number
  master: string
}

export interface PortForwardRule {
  id: string
  protocol: string
  host_port: number
  vm_ip: string
  vm_port: number
  description: string
}

export interface FirewallRule {
  id: string
  vm_ip: string
  direction: string
  protocol: string
  port: number
  action: string
  description: string
}

interface CreateBridgeRequest {
  name: string
  interfaces: string[]
  mtu: number
  stp: boolean
}

interface CreatePortForwardRequest {
  protocol: string
  host_port: number
  vm_ip: string
  vm_port: number
  description?: string
}

interface CreateFirewallRuleRequest {
  vm_ip: string
  direction: string
  protocol: string
  port: number
  action: string
  description?: string
}

export interface SysctlTuningRow {
  key: string
  recommended: string
  current: string | null
  current_error: string | null
}

export interface SysctlTuningResponse {
  dropin_path: string
  recommended_conf: string
  rows: SysctlTuningRow[]
  notes: string[]
}

export interface SystemdNetworkDiagnostics {
  systemd_networkd_active: boolean
  network_manager_active: boolean
  networkctl_list: string
  networkctl_status_all: string
  resolvectl_status: string
  resolvectl_statistics: string
  networkd_recent_logs: string
  resolved_recent_logs: string
}

interface LldpNeighbor {
  local_interface: string
  chassis_id: string
  system_name: string
  port_id: string
  port_description: string
  system_description: string
  capabilities: string
}

export interface LldpInventory {
  source: string
  neighbors: LldpNeighbor[]
  raw_text: string
  summary: string
}

/** Read-only `ip route show table all` / `ip -6 route show table all` output. */
export interface HostRoutingTables {
  ipv4: string
  ipv6: string
}

/** Add or delete one static route via `ip route` (validated on the daemon; browser session). */
interface KernelRouteChangeRequest {
  family: 'ipv4' | 'ipv6' | string
  operation: 'add' | 'delete' | string
  destination: string
  via?: string | null
  dev?: string | null
  table?: number | null
}

// Host interfaces
export const listHostInterfaces = () => readJsonArray<HostInterface>(`${API}/host/interfaces`)

/** Recommended sysctl drop-in + current runtime values (hypervisor / high-concurrency tuning). */
export const getSysctlTuning = () => readJsonObject<SysctlTuningResponse>(`${API}/host/sysctl-tuning`)
export const getSystemdNetworkDiagnostics = () =>
  readJsonObject<SystemdNetworkDiagnostics>(`${API}/host/network-diag`)
export const getLldpNeighbors = () => readJsonObject<LldpInventory>(`${API}/host/lldp`)
export const getSystemdInterfaceStatus = (name: string) =>
  readJsonObject<{ interface: string; status: string }>(
    `${API}/host/network-diag/interface/${encodeURIComponent(name)}`,
  )

export const getHostRoutingTables = () => readJsonObject<HostRoutingTables>(`${API}/host/routing-tables`)

export const postHostKernelRoute = (req: KernelRouteChangeRequest) =>
  apiPost<{ status: string }>(`${API}/host/routing`, req)

// Bridges
export const createBridge = (req: CreateBridgeRequest) => apiPost<unknown>(`${API}/host/bridges`, req)
export const deleteBridge = (name: string) => apiDelete(`${API}/host/bridges/${encodeURIComponent(name)}`)

// Port forwarding
export const listPortForwards = () => readJsonArray<PortForwardRule>(`${API}/portforward`)
export const createPortForward = (req: CreatePortForwardRequest) => apiPost<unknown>(`${API}/portforward`, req)
export const deletePortForward = (req: { protocol: string; host_port: number; vm_ip: string; vm_port: number }) =>
  apiPost<unknown>(`${API}/portforward/delete`, req)

// Firewall
export const listFirewallRules = () => readJsonArray<FirewallRule>(`${API}/firewall`)
export const createFirewallRule = (req: CreateFirewallRuleRequest) => apiPost<unknown>(`${API}/firewall`, req)
export const deleteFirewallRule = (req: CreateFirewallRuleRequest) => apiPost<unknown>(`${API}/firewall/delete`, req)
