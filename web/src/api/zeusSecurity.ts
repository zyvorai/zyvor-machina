// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'
import type { BpfStatus } from './bpf'

/** Fleet summary of the native eBPF datapath (machina-bpfd on each host). */
export interface NativeBpfStatus {
  enabled: boolean
  reachable: boolean
  hosts_total: number
  hosts_reachable: number
  enforcing_hosts: number
  version: string
  summary: string
}

export interface ZeusSecurityStatus {
  native_bpf: NativeBpfStatus
  zeus_firewall: Record<string, unknown>
  fabric_reachable: boolean
}

export interface FleetThreatSummary {
  fleet_threat_score: number
  firewall_targets: number
  critical_events: Array<Record<string, unknown>>
  native_bpf: Record<string, unknown>
  security_graph_summary: string
}

interface SecurityGraphNode {
  id: string
  kind: string
  label: string
  risk?: string
}

export interface SecurityGraph {
  nodes: SecurityGraphNode[]
  edges: Array<{ from: string; to: string; label: string }>
}

export interface SecurityEvent {
  id?: string
  host_id?: string
  timestamp?: string
  kind?: string
  severity?: string
  summary?: string
  process?: { pid?: number; ppid?: number; user?: string; binary?: string; args?: string }
  network?: { dst_ip?: string; port?: number; protocol?: string }
  dns?: { query?: string }
  file?: { path?: string; action?: string }
  k8s?: { namespace?: string; pod?: string; container?: string }
  verdict?: string
  // `/zeus-security/fleet/timeline` (unlike the per-host timeline above) returns
  // this flatter shape instead — same event feed, different endpoint/schema.
  message?: string
  namespace?: string
  source?: string
  type?: string
}

export const getZeusSecurityStatus = () => platformFetch<ZeusSecurityStatus>('/api/v1/zeus-security/status')
export const getFleetThreatSummary = () => platformFetch<FleetThreatSummary>('/api/v1/zeus-security/fleet/threat')
export const getZeusSecurityGraph = () => platformFetch<SecurityGraph>('/api/v1/zeus-security/graph')
export const getZeusSecuritySensors = () => platformFetch<{ sensors: Array<Record<string, unknown>> }>('/api/v1/zeus-security/sensors')
export const getZeusAssetInventory = () => platformFetch<Record<string, unknown>>('/api/v1/zeus-security/asset-inventory')

export const getFleetSecurityTimeline = (hours = 24) =>
  platformFetch<{ events: SecurityEvent[] }>(`/api/v1/zeus-security/fleet/timeline?hours=${hours}`)

export const getSecurityCorrelations = () =>
  platformFetch<{ correlations: Array<Record<string, unknown>> }>('/api/v1/zeus-security/correlations')

interface FabricHealthIssue {
  severity?: string
  kind?: string
  summary?: string
  host_id?: string
}

export interface FabricHealth {
  status?: string
  sensors_total?: number
  sensors_healthy?: number
  summary?: string
  issues?: FabricHealthIssue[]
  hunt_index?: { configured?: boolean; reachable?: boolean; document_count?: number }
}

export const getFabricHealth = () =>
  platformFetch<FabricHealth>('/api/v1/zeus-security/fabric/health')

export interface HuntQuery {
  id: string
  name: string
  query?: string
  description?: string
  severity?: string
}

export const getHuntQueries = () =>
  platformFetch<{ queries: HuntQuery[] }>('/api/v1/zeus-security/hunt/queries')

export const runHuntQuery = (queryId: string, hostId?: string) => {
  const q = hostId ? `?host_id=${encodeURIComponent(hostId)}` : ''
  return platformFetch<{
    ok?: boolean
    query_id?: string
    name?: string
    results?: Array<Record<string, unknown>>
    hit_count?: number
    source?: string
  }>(`/api/v1/zeus-security/hunt/run/${encodeURIComponent(queryId)}${q}`, { method: 'POST' })
}

export const syncSecurityAlerts = () =>
  platformFetch<{ inserted: number; summary: string }>('/api/v1/zeus-security/alerts/sync', { method: 'POST' })

export const getHostSecuritySummary = (hostId: string) =>
  platformFetch<Record<string, unknown>>(`/api/v1/zeus-security/hosts/${encodeURIComponent(hostId)}/summary`)

export const getHostProcesses = (hostId: string, hours = 24) =>
  platformFetch<{ processes: SecurityEvent[] }>(`/api/v1/zeus-security/hosts/${encodeURIComponent(hostId)}/processes?hours=${hours}`)

export const getHostConnections = (hostId: string, hours = 24) =>
  platformFetch<{ connections: SecurityEvent[] }>(`/api/v1/zeus-security/hosts/${encodeURIComponent(hostId)}/connections?hours=${hours}`)

export const getHostDns = (hostId: string, hours = 24) =>
  platformFetch<{ dns: SecurityEvent[] }>(`/api/v1/zeus-security/hosts/${encodeURIComponent(hostId)}/dns?hours=${hours}`)

export const getHostSecurityFiles = (hostId: string, hours = 168) =>
  platformFetch<{ files: SecurityEvent[] }>(`/api/v1/zeus-security/hosts/${encodeURIComponent(hostId)}/files?hours=${hours}`)

export const getHostSecurityPorts = (hostId: string) =>
  platformFetch<{ ports: Array<Record<string, unknown>> }>(`/api/v1/zeus-security/hosts/${encodeURIComponent(hostId)}/ports`)

export interface ContainerHierarchy {
  host_id?: string
  summary?: string
  namespaces?: Array<Record<string, unknown>>
}

export const getHostContainers = (hostId: string) =>
  platformFetch<ContainerHierarchy>(`/api/v1/zeus-security/hosts/${encodeURIComponent(hostId)}/containers`)

export const getHostSecurityTimeline = (hostId: string, hours = 24) =>
  platformFetch<{ events: SecurityEvent[] }>(`/api/v1/zeus-security/hosts/${encodeURIComponent(hostId)}/timeline?hours=${hours}`)

export const getHostProcessGraph = (hostId: string, pid?: number) => {
  const q = pid != null ? `?pid=${pid}` : ''
  return platformFetch<Record<string, unknown>>(`/api/v1/zeus-security/hosts/${encodeURIComponent(hostId)}/process-graph${q}`)
}

/** machina-bpfd status as reported through the host's agent. */
type SecurityFabricStatus = BpfStatus

export interface HostFabricStatusResponse {
  host_id: string
  agent_reachable: boolean
  message?: string
  fabric?: SecurityFabricStatus
}

export const getHostFabricStatus = (hostId: string) =>
  platformFetch<HostFabricStatusResponse>(`/api/v1/zeus-security/hosts/${encodeURIComponent(hostId)}/fabric-status`)

export const searchZeusSecurity = (query: string, hostId?: string) =>
  platformFetch<{ results: SecurityEvent[] }>('/api/v1/zeus-security/search', {
    method: 'POST',
    body: JSON.stringify({ query, host_id: hostId }),
  })

export const explainSecurityEvent = (event: SecurityEvent, hostId?: string) =>
  platformFetch<{ explanation: string; risk: string; recommendation: string; llm_powered?: boolean }>(
    '/api/v1/ai/security/explain-event',
    {
      method: 'POST',
      body: JSON.stringify({ event, host_id: hostId }),
    },
  )

export const reconstructAttack = (hostId: string, hours = 24) =>
  platformFetch<{ attack_chain: string[]; summary: string; llm_powered?: boolean }>(
    '/api/v1/ai/security/attack-reconstruct',
    {
      method: 'POST',
      body: JSON.stringify({ host_id: hostId, hours }),
    },
  )

export const nlSecuritySearch = (query: string, hostId?: string) =>
  platformFetch<{
    original_query: string
    search_query: string
    results: Record<string, unknown>
    hit_count?: number
    search_backend?: string
    llm_powered?: boolean
  }>('/api/v1/ai/security/nl-search', { method: 'POST', body: JSON.stringify({ query, host_id: hostId }) })

export const getSecurityHuntSummary = (hours = 48) =>
  platformFetch<{ summary: string; priority_actions?: string[]; llm_powered?: boolean }>(
    '/api/v1/ai/security/hunt-summary',
    { method: 'POST', body: JSON.stringify({ hours }) },
  )

export interface EnforcementPolicy {
  id?: string
  name: string
  kind: string
  match: string
  enabled?: boolean
  scope?: string
  applied_hosts?: string[]
  description?: string
  backend?: string
}

export interface EnforcementStatus {
  mode?: string
  policies_total?: number
  policies_enabled?: number
  applied_hosts?: string[]
  blocked_events?: number
  summary?: string
  attached?: boolean
  /** At least one host's machina-bpfd answered. */
  reachable?: boolean
  api_mode?: string
  kinds?: string[]
  hosts?: Array<{
    host_id: string
    hostname: string
    reachable: boolean
    error?: string | null
    mode?: { mode?: string; lease_expires_at?: string | null; lease_remaining_secs?: number | null } | null
  }>
  /** attach/sync/detach: false when some host didn't take the change. */
  ok?: boolean
  note?: string
}

/** Mutating enforcement calls wrap the native result; rejected policies are HTTP 400. */
export interface EnforcementMutation {
  native: {
    ok?: boolean
    policy?: EnforcementPolicy
    sync?: Array<{ host_id: string; hostname: string; ok: boolean; error?: string | null }>
    host_ids?: string[]
    removed_from_hosts?: string[]
    note?: string
    summary?: string
  }
  summary: string
}

export const attachEnforcement = () =>
  platformFetch<EnforcementStatus>('/api/v1/zeus-security/enforcement/attach', { method: 'POST', body: '{}' })

export const syncEnforcement = () =>
  platformFetch<EnforcementStatus>('/api/v1/zeus-security/enforcement/sync', { method: 'POST', body: '{}' })

export const detachEnforcement = () =>
  platformFetch<EnforcementStatus>('/api/v1/zeus-security/enforcement/detach', { method: 'POST', body: '{}' })

export const getEnforcementStatus = () =>
  platformFetch<EnforcementStatus>('/api/v1/zeus-security/enforcement/status')

export const getEnforcementPolicies = () =>
  platformFetch<{ policies: EnforcementPolicy[]; kinds?: string[] }>('/api/v1/zeus-security/enforcement/policies')

export const createEnforcementPolicy = (body: {
  name: string
  kind: string
  match: string
  scope?: string
  description?: string
}) =>
  platformFetch<EnforcementMutation>('/api/v1/zeus-security/enforcement/policies', {
    method: 'POST',
    body: JSON.stringify(body),
  })

export const applyEnforcementPolicy = (policyId: string, hostIds: string[]) =>
  platformFetch<EnforcementMutation>(`/api/v1/zeus-security/enforcement/policies/${encodeURIComponent(policyId)}/apply`, {
    method: 'POST',
    body: JSON.stringify({ host_ids: hostIds }),
  })

export const patchEnforcementPolicy = (
  policyId: string,
  body: { enabled?: boolean; match?: string; description?: string },
) =>
  platformFetch<EnforcementMutation>(`/api/v1/zeus-security/enforcement/policies/${encodeURIComponent(policyId)}`, {
    method: 'PATCH',
    body: JSON.stringify(body),
  })

export const deleteEnforcementPolicy = (policyId: string) =>
  platformFetch<EnforcementMutation>(`/api/v1/zeus-security/enforcement/policies/${encodeURIComponent(policyId)}`, {
    method: 'DELETE',
  })

export const getEnforcementPolicyDocument = (policyId: string) =>
  platformFetch<{ native_policy?: Record<string, unknown> | null; datapath_rules?: string[] }>(
    `/api/v1/zeus-security/enforcement/policies/${encodeURIComponent(policyId)}/document`,
  )

export interface FleetSensorRow {
  host_id: string
  hostname: string
  host_state: string
  sensor_status: string
  sensor?: Record<string, unknown>
}

export const getFleetSensors = () =>
  platformFetch<{ matrix: FleetSensorRow[]; summary?: string; sensors?: Array<Record<string, unknown>> }>(
    '/api/v1/zeus-security/fleet/sensors',
  )

export const getHostEnforcement = (hostId: string) =>
  platformFetch<Record<string, unknown>>(`/api/v1/zeus-security/hosts/${encodeURIComponent(hostId)}/enforcement`)
