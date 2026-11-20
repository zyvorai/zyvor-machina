// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch, getControllerBase, platformHeaders, downloadControllerExport } from './platform'

export interface AiSettings {
  enabled: boolean
  mode: string
  provider: string
  model: string
  api_key_configured: boolean
  autopilot_interval_secs: number
  autopilot_max_actions?: number
  fleet_peer_urls?: string[]
  autopilot_last_run?: string
}

export interface SpotlightIntent {
  id: string
  label: string
  review: string
  action: string
  vm_name?: string
  navigate?: string
  prefill?: Record<string, unknown>
}

export interface SpotlightResult {
  intents: SpotlightIntent[]
  search_hits: Array<{ kind: string; id: string; label: string; sublabel?: string }>
  suggested_action?: SpotlightIntent
}

export interface VmDoctorReport {
  vm_id: string
  vm_name: string
  score: string
  score_numeric: number
  score_label: string
  healthy: boolean
  checks_passed: number
  checks_total: number
  issues: Array<{
    id: string
    severity: string
    message: string
    remediation?: string
    fix_action?: string
    fix_label?: string
  }>
  guest_tools_status: string
}

export interface MigrationAdvisorReport {
  vm_name: string
  provider: string
  readiness_percent: number
  safe: string[]
  risks: string[]
  recommended_target: Record<string, unknown>
  remediation: string[]
  guestkit_boot_score?: number
  guestkit_migration_score?: number
  guestkit_summary?: string
  firewall_dependencies?: string[]
  firewall_migration_summary?: string
}

export interface CostAnalysis {
  estimated_monthly_usd: number
  predicted_next_month_usd?: number
  vm_count: number
  idle_vm_count: number
  oversized_vm_count: number
  snapshot_heavy_count: number
  suggestions: string[]
}

export interface CapacityPlan {
  hosts_online: number
  memory_headroom_mib: number
  avg_cpu_percent: number
  storage_used_gib: number
  storage_capacity_gib: number
  storage_runway_days?: number
  cpu_headroom_percent: number
  estimated_small_vms_addable: number
  recommendations: string[]
}

export interface SecurityReport {
  risk_level: string
  findings: Array<{
    id: string
    severity: string
    title: string
    detail: string
    remediation: string
  }>
}

export const getAiSettings = () => platformFetch<AiSettings>('/api/v1/ai/settings')
export const patchAiSettings = (body: Partial<AiSettings & { api_key?: string }>) =>
  platformFetch<AiSettings>('/api/v1/ai/settings', { method: 'PATCH', body: JSON.stringify(body) })

export const aiSpotlight = (query: string) =>
  platformFetch<SpotlightResult>('/api/v1/ai/spotlight', { method: 'POST', body: JSON.stringify({ query }) })

type FleetVmGuestRow = {
  vm_id: string
  vm_name: string
  os_pretty_name: string
  guest_ip: string
  install_state: string
  user_count: number
  time_drift_ms?: number
  flags: string[]
}

export type FleetGuestQueryReport = {
  query: string
  summary: string
  matched_count: number
  scanned_count: number
  vms: FleetVmGuestRow[]
  llm_powered: boolean
}

export const fleetGuestQuery = (body: {
  query: string
  vm_ids?: string[]
  project?: string
  tag?: string
}) =>
  platformFetch<FleetGuestQueryReport>('/api/v1/ai/fleet/guest-query', {
    method: 'POST',
    body: JSON.stringify(body),
  })

type VmMigrationReadinessRow = {
  vm_id: string
  vm_name: string
  readiness_percent: number
  install_state: string
  os_pretty_name: string
  guest_ip: string
  qga_gaps: string[]
  remediation: string[]
  assurance_mode?: 'live_qga' | 'offline_guestkit' | string
  guestkit_summary?: string
}

export type MigrationReadinessReport = {
  executive_summary: string
  vm_count: number
  rows: VmMigrationReadinessRow[]
  prioritized_remediation: string[]
  llm_powered: boolean
}

export const migrationReadinessReport = (body: { vm_ids?: string[]; provider?: string } = {}) =>
  platformFetch<MigrationReadinessReport>('/api/v1/ai/migration/readiness-report', {
    method: 'POST',
    body: JSON.stringify(body),
  })

interface CopilotStreamEvent {
  type: 'chunk' | 'done' | 'error'
  text?: string
  deterministic?: boolean
  context_summary?: string
  message?: string
}

export async function aiCopilotStream(
  message: string,
  vmId: string | undefined,
  onEvent: (ev: CopilotStreamEvent) => void,
  hostId?: string,
  vmIds?: string[],
): Promise<void> {
  const url = `${getControllerBase()}/api/v1/ai/copilot/stream`
  const res = await fetch(url, {
    method: 'POST',
    credentials: 'same-origin',
    headers: platformHeaders(),
    body: JSON.stringify({ message, vm_id: vmId, host_id: hostId, vm_ids: vmIds }),
  })
  if (!res.ok) {
    throw new Error(`Copilot stream failed (HTTP ${res.status})`)
  }
  if (!res.body) throw new Error('Copilot stream: empty response body')

  const reader = res.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''

  while (true) {
    const { done, value } = await reader.read()
    if (done) break
    buffer += decoder.decode(value, { stream: true })
    const frames = buffer.split('\n\n')
    buffer = frames.pop() ?? ''
    for (const frame of frames) {
      for (const line of frame.split('\n')) {
        if (!line.startsWith('data: ')) continue
        const payload = line.slice(6).trim()
        if (!payload) continue
        // Skip a malformed frame rather than letting one bad byte reject the whole
        // stream (which discards the streamed answer and forces a second LLM call).
        let evt: CopilotStreamEvent
        try {
          evt = JSON.parse(payload) as CopilotStreamEvent
        } catch {
          continue
        }
        onEvent(evt)
      }
    }
  }
}

export const aiExplain = (screen: string, objectRef: Record<string, unknown> = {}) =>
  platformFetch<{ explanation: string }>('/api/v1/ai/explain', {
    method: 'POST',
    body: JSON.stringify({ screen, object_ref: objectRef }),
  })

export const aiRunbook = (incident: string, context: Record<string, unknown> = {}) =>
  platformFetch<{ incident: string; title: string; steps: string[]; commands: string[]; summary?: string }>(
    '/api/v1/ai/runbook',
    { method: 'POST', body: JSON.stringify({ incident, context }) },
  )

export const aiGenerateBlueprint = (prompt: string) =>
  platformFetch<{ name: string; description: string; actions: string[]; notes: string }>(
    '/api/v1/ai/blueprints/generate',
    { method: 'POST', body: JSON.stringify({ prompt }) },
  )

export const getAiCost = () => platformFetch<CostAnalysis>('/api/v1/ai/cost')
export const getAiCapacity = () => platformFetch<CapacityPlan>('/api/v1/ai/capacity')
export const getAiSecurity = () => platformFetch<SecurityReport>('/api/v1/ai/security')

export const getVmDoctor = (vmId: string) => platformFetch<VmDoctorReport>(`/api/v1/vms/${encodeURIComponent(vmId)}/doctor`)

export const getMigrationAdvisor = (vm: string, provider = 'vmware', os?: string, diskPath?: string) => {
  const q = new URLSearchParams({ vm, provider })
  if (os) q.set('os', os)
  if (diskPath) q.set('disk_path', diskPath)
  return platformFetch<MigrationAdvisorReport>(`/api/v1/migrations/advisor?${q}`)
}

export const aiNetworkExplain = (vm_a: string, vm_b: string, port?: number) =>
  platformFetch<{ can_reach: boolean; explanation: string; hops: string[]; remediation: string }>(
    '/api/v1/ai/network/explain',
    { method: 'POST', body: JSON.stringify({ vm_a, vm_b, port }) },
  )

interface PolicyExport {
  yaml: string
  rule_count: number
  quota_count: number
}

export const getAiPolicyExport = () => platformFetch<PolicyExport>('/api/v1/ai/policy/export')

export interface ProposedAction {
  id: string
  label: string
  review: string
  risk: string
  action_type: string
  object_ref: Record<string, unknown>
}

interface AutopilotProposal {
  mode: string
  actions: ProposedAction[]
}

export interface ComplianceReport {
  score: number
  grade: string
  summary: string
  checks: Array<{ id: string; name: string; passed: boolean; score: number; detail: string }>
  markdown: string
  finding_count: number
}

export const getAutopilotProposal = (vmId?: string) => {
  const q = vmId ? `?vm_id=${encodeURIComponent(vmId)}` : ''
  return platformFetch<AutopilotProposal>(`/api/v1/ai/autopilot/propose${q}`)
}

export const executeAutopilotAction = (action_type: string, object_ref: Record<string, unknown> = {}) =>
  platformFetch<{ message: string; task_ids: string[] }>('/api/v1/ai/autopilot/execute', {
    method: 'POST',
    body: JSON.stringify({ action_type, object_ref }),
  })

export const getAiCompliance = () => platformFetch<ComplianceReport>('/api/v1/ai/compliance')

export interface TerminalSuggestResult {
  vm_name: string
  observed_state: string
  suggestions: Array<{ label: string; command: string; description: string; scope: string }>
  notes: string
}

export const aiTerminalSuggest = (vmId?: string, vmName?: string) =>
  platformFetch<TerminalSuggestResult>('/api/v1/ai/terminal/suggest', {
    method: 'POST',
    body: JSON.stringify({ vm_id: vmId, vm_name: vmName }),
  })

export const runAutopilotSafe = (vmId?: string, maxActions = 3) =>
  platformFetch<{ executed_count: number; skipped_count: number; results: Array<{ message: string; task_ids: string[] }> }>(
    '/api/v1/ai/autopilot/run',
    { method: 'POST', body: JSON.stringify({ vm_id: vmId, max_actions: maxActions }) },
  )

export interface AutopilotHistoryEntry {
  id: string
  actor: string
  action: string
  created_at: string
  detail: Record<string, unknown>
}

export const getAutopilotHistory = (limit = 20) =>
  platformFetch<AutopilotHistoryEntry[]>(`/api/v1/ai/autopilot/history?limit=${limit}`)

export interface DigitalTwinGraph {
  nodes: Array<{ kind: string; id: string; name: string; state?: string }>
  edges: Array<{ from: string; to: string; label: string }>
  node_count: number
  edge_count: number
}

export interface ImpactAnalysis {
  action: string
  target: string
  severity: string
  summary: string
  affected_vms: string[]
  affected_applications: string[]
  storage_risks: string[]
  network_notes: string[]
  recommendations: string[]
}

export const getDigitalTwinGraph = () => platformFetch<DigitalTwinGraph>('/api/v1/ai/twin/graph')

export const analyzeTwinImpact = (body: { action: string; target_kind: string; target_id: string }) =>
  platformFetch<ImpactAnalysis>('/api/v1/ai/twin/impact', { method: 'POST', body: JSON.stringify(body) })

interface TimelineEntry {
  at: string
  source: string
  kind: string
  message: string
  severity: string
}

export interface IncidentAnalysis {
  window_hours: number
  timeline: TimelineEntry[]
  root_cause: string
  confidence: number
  contributing_factors: string[]
  suggested_actions: string[]
  evidence?: string[]
}

export const analyzeIncident = (params?: { hours?: number; vm_id?: string; vm_name?: string }) => {
  const qs = new URLSearchParams()
  if (params?.hours) qs.set('hours', String(params.hours))
  if (params?.vm_id) qs.set('vm_id', params.vm_id)
  if (params?.vm_name) qs.set('vm_name', params.vm_name)
  const q = qs.toString()
  return platformFetch<IncidentAnalysis>(`/api/v1/ai/incidents/analyze${q ? `?${q}` : ''}`)
}

export interface EnvironmentResourcePlan {
  label: string
  review: string
  developer_count: number
  vm_count: number
  total_vcpus: number
  total_memory_gib: number
  storage_gib: number
  estimated_monthly_usd: number
  environment_type: string
  gpu_required: boolean
  preview_only: boolean
  build_steps: string[]
}

export const planEnvironment = (query: string) =>
  platformFetch<EnvironmentResourcePlan>('/api/v1/ai/intent/environment', {
    method: 'POST',
    body: JSON.stringify({ query }),
  })

export const executeEnvironment = (query: string, dryRun = true) =>
  platformFetch<{ summary: string; plan: EnvironmentResourcePlan; vm_tasks: Array<{ name: string; host: string }> }>(
    '/api/v1/ai/intent/environment/execute',
    { method: 'POST', body: JSON.stringify({ query, dry_run: dryRun }) },
  )

export const getSreRemediate = () =>
  platformFetch<{ summary: string; remediations: Array<{ label: string; review: string; action: string }> }>(
    '/api/v1/ai/sre/remediate',
  )

export const getComplianceRemediate = () =>
  platformFetch<{ summary: string; remediations: Array<{ label: string; framework: string; review: string }> }>(
    '/api/v1/ai/compliance/remediate',
  )

export const getZyraSummary = () =>
  platformFetch<{
    status: string
    tagline: string
    hosts_online: number
    vm_count: number
    monthly_cost_usd: number
    security_risk: string
    compliance_grade: string
    firewall_critical_hosts?: number
    firewall_drift_hosts?: number
    baremetal_critical_count?: number
    exposure_waste_usd?: number
    highlights: string[]
  }>('/api/v1/ai/zyra/summary')

export const getFleetPowerOptimize = () =>
  platformFetch<{ summary: string; total_savings_usd_month: number; optimizations: Array<{ host: string; action: string; reason: string }> }>(
    '/api/v1/ai/fleet/power/optimize',
  )

export const getBaremetalProvision = (id: string) =>
  platformFetch<{ summary: string; steps: string[] }>(`/api/v1/baremetal/servers/${encodeURIComponent(id)}/provision`)

export interface SreForecast {
  vm_id: string
  vm_name: string
  resource: string
  severity: string
  message: string
  hours_until_critical?: number
  confidence: number
}

export const getSreForecast = () =>
  platformFetch<{ forecasts: SreForecast[] }>('/api/v1/ai/sre/forecast')

export interface FleetHeatmap {
  hosts: Array<{ host_id: string; hostname: string; cpu_percent: number; memory_percent: number; vm_count: number; classification: string }>
  hotspots: string[]
  cold_hosts: string[]
  power_waste_hosts: string[]
}

export const getFleetHeatmap = () => platformFetch<FleetHeatmap>('/api/v1/ai/fleet/heatmap')

interface FleetClusterSlice {
  label: string
  reachable: boolean
  vm_count: number
  estimated_monthly_usd: number
  memory_headroom_mib: number
  security_risk_level: string
}

interface FleetZyraSummary {
  clusters: FleetClusterSlice[]
  aggregate_monthly_usd: number
  aggregate_vm_count: number
  peer_count: number
  reachable_peers: number
}

export const getFleetSummary = () =>
  platformFetch<FleetZyraSummary>('/api/v1/ai/fleet/summary')

export const getFleetLocal = () =>
  platformFetch<FleetClusterSlice>('/api/v1/ai/fleet/local')

export interface RebalanceProposal {
  moves: Array<{ vm_id: string; vm_name: string; from_host: string; to_host: string; reason: string; score: number }>
  estimated_savings_pct: number
  summary: string
}

export const getFleetRebalanceProposal = (maxMoves = 5) =>
  platformFetch<RebalanceProposal>(`/api/v1/ai/fleet/rebalance/propose?max_moves=${maxMoves}`)

interface RebalanceExecuteResult {
  dry_run: boolean
  task_ids: string[]
  moves: RebalanceProposal['moves']
  summary: string
}

export const executeFleetRebalance = (dryRun = true, maxMoves = 5) =>
  platformFetch<RebalanceExecuteResult>('/api/v1/ai/fleet/rebalance/execute', {
    method: 'POST',
    body: JSON.stringify({ dry_run: dryRun, max_moves: maxMoves }),
  })

export interface CostAttributionReport {
  total_monthly_usd: number
  teams: Array<{ team: string; vm_count: number; estimated_monthly_usd: number; exposure_monthly_usd?: number; share_pct: number }>
  unattributed_monthly_usd: number
  summary: string
}

export const getCostAttribution = () => platformFetch<CostAttributionReport>('/api/v1/ai/cost/attribution')

interface ComplianceFrameworksReport {
  frameworks: Array<{ framework: string; score: number; grade: string; control_count: number; failed_count: number }>
  controls: Array<{ id: string; framework: string; title: string; passed: boolean; score: number }>
  summary: string
}

export const getComplianceFrameworks = () =>
  platformFetch<ComplianceFrameworksReport>('/api/v1/ai/compliance/frameworks')

export interface SecurityGraph {
  nodes: Array<{ id: string; kind: string; label: string; risk?: string }>
  edges: Array<{ from: string; to: string; label: string }>
}

export const getSecurityGraph = () => platformFetch<SecurityGraph>('/api/v1/ai/security/graph')

export const analyzeAttackPath = (source: string, target_vm: string) =>
  platformFetch<{ summary: string; path: string[]; edges: string[]; risk_score: number }>(
    '/api/v1/ai/security/attack-path',
    { method: 'POST', body: JSON.stringify({ source, target_vm }) },
  )

export const searchKnowledge = (query: string) =>
  platformFetch<{ query: string; hits: KnowledgeHit[] }>('/api/v1/ai/knowledge/search', {
    method: 'POST',
    body: JSON.stringify({ query }),
  })

export interface KnowledgeHit {
  kind: string
  id: string
  title: string
  snippet: string
  score: number
  navigate?: string
}

interface ServiceGraph {
  nodes: Array<{ kind: string; id: string; name: string }>
  edges: Array<{ from: string; to: string; label: string }>
  service_count: number
}

export const getServiceGraph = () => platformFetch<ServiceGraph>('/api/v1/ai/services/graph')

export interface InfrastructureMemory {
  incidents: Array<{ at: string; kind: string; summary: string; actor: string; lesson: string }>
  runbook_hints: string[]
}

export const getInfrastructureMemory = (limit = 20) =>
  platformFetch<InfrastructureMemory>(`/api/v1/ai/memory/incidents?limit=${limit}`)

export interface MissionStackPlan {
  label: string
  review: string
  gpu_node_count: number
  preview_only: boolean
  estimated_monthly_usd: number
  phases: Array<{ name: string; steps: string[]; automated: boolean }>
}

export const planMissionStack = (query: string) =>
  platformFetch<MissionStackPlan>('/api/v1/ai/mission/stack', {
    method: 'POST',
    body: JSON.stringify({ query }),
  })

interface MissionStackExecuteResult {
  dry_run: boolean
  plan: MissionStackPlan
  vm_tasks: Array<{ name: string; host: string; task_id?: string }>
  summary: string
}

export const executeMissionStack = (query: string, dryRun = true) =>
  platformFetch<MissionStackExecuteResult>('/api/v1/ai/mission/stack/execute', {
    method: 'POST',
    body: JSON.stringify({ query, dry_run: dryRun }),
  })

export const getGpuPlacement = (workload = 'inference') =>
  platformFetch<{
    summary: string
    candidates: Array<{ host_id?: string; hostname: string; gpu_capable: boolean; score: number; reason: string }>
  }>(`/api/v1/ai/fleet/gpu-placement?workload=${encodeURIComponent(workload)}`)

export const diagnoseKnowledge = (query: string) =>
  platformFetch<{ summary: string; hypotheses: Array<{ title: string; confidence: number; evidence: string; action: string }> }>(
    '/api/v1/ai/knowledge/diagnose',
    { method: 'POST', body: JSON.stringify({ query }) },
  )

export const diagnoseFleet = (query: string) =>
  platformFetch<{
    query: string
    summary: string
    zyra_status: string
    linux_summary: string
    hypotheses: Array<{ title: string; confidence: number; evidence: string; action: string }>
  }>('/api/v1/ai/fleet/diagnose', { method: 'POST', body: JSON.stringify({ query }) })

export const simulateServiceImpact = (service: string) =>
  platformFetch<{ summary: string; severity: string; affected_vms: string[] }>(
    '/api/v1/ai/services/impact',
    { method: 'POST', body: JSON.stringify({ service }) },
  )

export const getSimilarIncidents = (q: string, limit = 10) =>
  platformFetch<{ summary: string; incidents: Array<{ kind: string; summary: string; similarity: number }> }>(
    `/api/v1/ai/memory/similar?q=${encodeURIComponent(q)}&limit=${limit}`,
  )

interface RemediationHubItem {
  id: string
  source: string
  label: string
  review: string
  action: string
  priority: number
  risk: string
}

export const getRemediateHub = () =>
  platformFetch<{ summary: string; items: RemediationHubItem[] }>('/api/v1/ai/remediate/hub')

export const getKnowledgeRunbook = (query: string) =>
  platformFetch<{
    query: string
    diagnosis_summary: string
    runbook_title: string
    steps: string[]
    commands: string[]
    summary: string
  }>('/api/v1/ai/knowledge/runbook', { method: 'POST', body: JSON.stringify({ query }) })

export interface CostBudgetReport {
  monthly_budget_usd: number
  current_spend_usd: number
  predicted_spend_usd: number
  utilization_pct: number
  status: string
  alerts: Array<{ id: string; severity: string; message: string }>
  summary: string
}

export const getCostBudget = () => platformFetch<CostBudgetReport>('/api/v1/ai/cost/budget')

interface MissionStackStatus {
  gpu_vms: Array<{ name: string; observed_state: string; host: string | null; tags: string[] }>
  environment_vms: Array<{ name: string; observed_state: string; host: string | null; tags: string[] }>
  total_stack_vms: number
  running: number
  summary: string
}

export const getMissionStackStatus = () => platformFetch<MissionStackStatus>('/api/v1/ai/mission/stack/status')

export const downloadAiCostExport = () =>
  downloadControllerExport('/api/v1/ai/cost/export.csv', 'machina-cost-guardian.csv')

export const downloadAiCapacityExport = () =>
  downloadControllerExport('/api/v1/ai/capacity/export.csv', 'machina-capacity-planner.csv')

export const downloadCostAttributionExport = () =>
  downloadControllerExport('/api/v1/ai/cost/attribution/export.csv', 'machina-cost-attribution.csv')

export const downloadAiComplianceExport = () =>
  downloadControllerExport('/api/v1/ai/compliance/export', 'machina-compliance-report.html')

export const downloadAiCompliancePdf = () =>
  downloadControllerExport('/api/v1/ai/compliance/export.pdf', 'machina-compliance-report.pdf')

export interface BaremetalServer {
  id: string
  hostname: string
  bmc_address: string
  bmc_type: string
  state: string
  cpu_cores: number
  memory_mib: number
  firewall_profile?: string
  firewall_enabled?: boolean
  bmc_vlan?: string
  pxe_vlan?: string
  created_at?: string
}

export const listBaremetalServers = () => platformFetch<BaremetalServer[]>('/api/v1/baremetal/servers')

export const registerBaremetalServer = (body: {
  hostname: string
  bmc_address: string
  bmc_type?: string
  cpu_cores?: number
  memory_mib?: number
  firewall_profile?: string
  firewall_enabled?: boolean
  bmc_vlan?: string
  pxe_vlan?: string
}) =>
  platformFetch<BaremetalServer>('/api/v1/baremetal/servers', { method: 'POST', body: JSON.stringify(body) })

export const planBaremetalCapacity = (query: string) =>
  platformFetch<{ summary: string; servers_needed: number; total_cpu_cores: number; total_memory_gib: number }>(
    '/api/v1/baremetal/capacity/plan',
    { method: 'POST', body: JSON.stringify({ query }) },
  )

export const setBaremetalPower = (id: string, action: 'on' | 'off' | 'cycle', dryRun = true) =>
  platformFetch<{ summary: string; new_state: string; dry_run: boolean }>(
    `/api/v1/baremetal/servers/${encodeURIComponent(id)}/power`,
    { method: 'POST', body: JSON.stringify({ action, dry_run: dryRun }) },
  )

// --- Zyra AI redesign ---

export interface AiProviderRow {
  id: string
  name: string
  kind: string
  base_url: string
  org_id: string
  deployment_name: string
  api_key_configured: boolean
  enabled: boolean
  is_default: boolean
}

export interface AiModelRow {
  id: string
  provider_id: string
  model_id: string
  display_name: string
  context_window: number
  enabled: boolean
}

export interface ZyraAgentInfo {
  id: string
  name: string
  description: string
  task_class: string
}

interface ZyraChatResponse {
  reply: string
  deterministic: boolean
  agent_id: string
  context_summary?: string
}

export interface AiPromptRow {
  id: string
  scope: string
  title: string
  body: string
  tags: string[]
  agent_id: string
}

export interface MemorySettings {
  enabled: boolean
  team_scope: boolean
  project_scope: boolean
  retention_days: number
}

export interface ZyraActionRow {
  id: string
  source: string
  action_type: string
  label: string
  review: string
  risk: string
  status: string
}

export interface AgentPluginRow {
  slug: string
  name: string
  description: string
  agent_id: string
  installed: boolean
}

export const listAiProviders = () => platformFetch<AiProviderRow[]>('/api/v1/ai/providers')
export const createAiProvider = (body: Record<string, unknown>) =>
  platformFetch<AiProviderRow>('/api/v1/ai/providers', { method: 'POST', body: JSON.stringify(body) })
export const patchAiProvider = (id: string, body: Record<string, unknown>) =>
  platformFetch<AiProviderRow>(`/api/v1/ai/providers/${encodeURIComponent(id)}`, { method: 'PATCH', body: JSON.stringify(body) })
export const deleteAiProvider = (id: string) =>
  platformFetch<{ deleted: boolean }>(`/api/v1/ai/providers/${encodeURIComponent(id)}`, { method: 'DELETE' })
export const testAiProvider = (id: string) =>
  platformFetch<{ ok: boolean }>(`/api/v1/ai/providers/${encodeURIComponent(id)}/test`, { method: 'POST' })
export const listAiProviderModels = (id: string) =>
  platformFetch<AiModelRow[]>(`/api/v1/ai/providers/${encodeURIComponent(id)}/models`)

export interface RoutingRuleRow {
  task_class: string
  provider_id: string | null
  model_id: string | null
  enabled: boolean
}

export const listAiRoutingRules = () => platformFetch<RoutingRuleRow[]>('/api/v1/ai/routing/rules')

export const patchAiRoutingRule = (
  taskClass: string,
  body: { provider_id?: string | null; model_id?: string | null; enabled: boolean },
) =>
  platformFetch<RoutingRuleRow>(`/api/v1/ai/routing/rules/${encodeURIComponent(taskClass)}`, {
    method: 'PATCH',
    body: JSON.stringify(body),
  })

export interface ZyraEnterpriseOverview {
  zyra_admin_role: boolean
  zyra_execute_role: boolean
  zyra_read_role: boolean
  air_gap_llm: boolean
  audit_events_24h: number
  scim_enabled: boolean
  sso_configured: boolean
}

export const getZyraEnterpriseOverview = () =>
  platformFetch<ZyraEnterpriseOverview>('/api/v1/ai/enterprise/zyra')

export const patchZyraEnterprise = (body: { air_gap_llm?: boolean }) =>
  platformFetch<ZyraEnterpriseOverview>('/api/v1/ai/enterprise/zyra', {
    method: 'PATCH',
    body: JSON.stringify(body),
  })

export const listZyraAgents = () => platformFetch<ZyraAgentInfo[]>('/api/v1/ai/agents')

export const zyraChat = (body: {
  message: string
  agent?: string
  vm_id?: string
  host_id?: string
  vm_ids?: string[]
  page_path?: string
}) =>
  platformFetch<ZyraChatResponse>('/api/v1/ai/zyra/chat', { method: 'POST', body: JSON.stringify(body) })

export const listAiPrompts = () => platformFetch<AiPromptRow[]>('/api/v1/ai/prompts')
export const createAiPrompt = (body: Record<string, unknown>) =>
  platformFetch<AiPromptRow>('/api/v1/ai/prompts', { method: 'POST', body: JSON.stringify(body) })
export const patchAiPrompt = (
  id: string,
  body: { title?: string; body?: string; tags?: string[]; agent_id?: string },
) =>
  platformFetch<AiPromptRow>(`/api/v1/ai/prompts/${encodeURIComponent(id)}`, { method: 'PATCH', body: JSON.stringify(body) })
export const deleteAiPrompt = (id: string) =>
  platformFetch<{ deleted: boolean }>(`/api/v1/ai/prompts/${encodeURIComponent(id)}`, { method: 'DELETE' })

export const getMemorySettings = () => platformFetch<MemorySettings>('/api/v1/ai/memory/settings')
export const patchMemorySettings = (body: Partial<MemorySettings>) =>
  platformFetch<MemorySettings>('/api/v1/ai/memory/settings', { method: 'PATCH', body: JSON.stringify(body) })
export const purgeMemory = (scope: 'all' | 'user' = 'all') =>
  platformFetch<{ deleted: number }>(`/api/v1/ai/memory?scope=${encodeURIComponent(scope)}`, { method: 'DELETE' })

export const createZyraAction = (body: {
  action_type: string
  label: string
  review?: string
  risk?: string
  object_ref?: Record<string, unknown>
  source?: string
}) =>
  platformFetch<ZyraActionRow>('/api/v1/ai/actions', { method: 'POST', body: JSON.stringify(body) })

export const getZyraApprovalHub = () =>
  platformFetch<{ zyra_actions: ZyraActionRow[]; total_pending: number; firewall_pending: number }>(
    '/api/v1/ai/actions/hub',
  )

export const executeZyraAction = (id: string) =>
  platformFetch<{ message?: string }>(`/api/v1/ai/actions/${encodeURIComponent(id)}/execute`, { method: 'POST' })
export const rejectZyraAction = (id: string) =>
  platformFetch<{ rejected: boolean }>(`/api/v1/ai/actions/${encodeURIComponent(id)}/reject`, { method: 'POST' })

export interface FleetDigest {
  hours: number
  hosts_total: number
  hosts_not_online: number
  vms_total: number
  vms_running: number
  open_incidents: number
  pending_approvals: number
  actions_executed: number
  actions_failed: number
  actions_rejected: number
  actions_undone: number
  budget_status: string | null
  budget_alerts: string[]
  headline: 'ok' | 'attention' | 'critical'
  markdown: string
}
export const getFleetDigest = (hours = 24) =>
  platformFetch<FleetDigest>(`/api/v1/ai/digest?hours=${encodeURIComponent(String(hours))}`)

export interface DigestSchedule {
  enabled: boolean
  frequency: 'daily' | 'weekly'
  hour_utc: number
  weekday: number
  last_sent_at: string | null
}
export const getDigestSchedule = () => platformFetch<DigestSchedule>('/api/v1/ai/digest/schedule')
export const setDigestSchedule = (s: Omit<DigestSchedule, 'last_sent_at'>) =>
  platformFetch<DigestSchedule>('/api/v1/ai/digest/schedule', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(s),
  })
export const sendDigestNow = () => platformFetch<FleetDigest>('/api/v1/ai/digest/send', { method: 'POST' })

export interface ZyraChangeReview {
  action_id: string
  action_type: string
  risk: 'low' | 'medium' | 'high'
  verdict: 'approve' | 'approve_with_care' | 'hold'
  summary: string
  blast_radius: string[]
  rollback: string
  reversible: boolean
  checks: string[]
  approved_streak: number
}
export const getZyraChangeReview = (id: string) =>
  platformFetch<ZyraChangeReview>(`/api/v1/ai/actions/${encodeURIComponent(id)}/review`)

export interface ZyraAgentStep {
  kind: 'tool_call' | 'tool_result' | 'note'
  tool?: string | null
  detail: string
}
export interface ZyraAgentRun {
  answer: string
  steps: ZyraAgentStep[]
  proposed_action_ids: string[]
  provider: string
}

/** Run the tool-calling Zyra agent. It can look at the fleet and queue proposals; it never executes a change. */
export const runZyraAgent = (prompt: string) =>
  platformFetch<ZyraAgentRun>('/api/v1/ai/agent/run', { method: 'POST', body: JSON.stringify({ prompt }) })

/**
 * Same as {@link runZyraAgent} but reports each step the moment it happens. Resolves with the final run.
 * Throws `AgentStreamUnsupported` when the controller has no stream route (older build) so callers can fall back.
 */
export class AgentStreamUnsupported extends Error {}
export async function streamZyraAgent(prompt: string, onStep: (s: ZyraAgentStep) => void): Promise<ZyraAgentRun> {
  const res = await fetch(`${getControllerBase()}/api/v1/ai/agent/stream`, {
    method: 'POST',
    credentials: 'same-origin',
    headers: platformHeaders(),
    body: JSON.stringify({ prompt }),
  })
  if (res.status === 404 || res.status === 405) throw new AgentStreamUnsupported('no stream route')
  if (!res.ok) {
    let msg = `Zyra agent failed (HTTP ${res.status})`
    try { msg = ((await res.json()) as { error?: string }).error ?? msg } catch { /* keep default */ }
    throw new Error(msg)
  }
  if (!res.body) throw new AgentStreamUnsupported('empty body')
  const reader = res.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  let final: ZyraAgentRun | null = null
  for (;;) {
    const { done, value } = await reader.read()
    if (done) break
    buffer += decoder.decode(value, { stream: true })
    const frames = buffer.split('\n\n')
    buffer = frames.pop() ?? ''
    for (const frame of frames) {
      for (const line of frame.split('\n')) {
        if (!line.startsWith('data: ')) continue
        let ev: { type?: string; message?: string; run?: ZyraAgentRun } & Partial<ZyraAgentStep>
        try { ev = JSON.parse(line.slice(6)) } catch { continue }
        if (ev.type === 'step') onStep({ kind: ev.kind as ZyraAgentStep['kind'], tool: ev.tool, detail: ev.detail ?? '' })
        else if (ev.type === 'done' && ev.run) final = ev.run
        else if (ev.type === 'error') throw new Error(ev.message || 'Zyra agent failed')
      }
    }
  }
  if (!final) throw new Error('The agent stream ended without an answer')
  return final
}

/** An executed or failed AI action with its verification and undo state. */
export interface ZyraActionHistoryRow {
  id: string
  action_type: string
  label: string
  status: string
  requested_by: string
  approved_by?: string | null
  executed_at?: string | null
  vm_id?: string | null
  before_state: Record<string, unknown>
  verify: { status?: 'ok' | 'pending' | 'failed' | 'unknown'; detail?: string; checked_at?: string }
  undoable: boolean
  undone_at?: string | null
}

export const listZyraActionHistory = () => platformFetch<ZyraActionHistoryRow[]>('/api/v1/ai/actions/history')
export const verifyZyraAction = (id: string) =>
  platformFetch<{ status: string; detail: string }>(`/api/v1/ai/actions/${encodeURIComponent(id)}/verify`, { method: 'POST' })
export const undoZyraAction = (id: string) =>
  platformFetch<{ message?: string }>(`/api/v1/ai/actions/${encodeURIComponent(id)}/undo`, { method: 'POST' })

/** Trust ladder: per action class, ask a person every time or let autopilot run queued proposals itself. */
export interface ZyraTrustClass {
  action_type: string
  level: 'ask' | 'auto'
  max_per_run: number
  approved_streak: number
  total_approved: number
  total_rejected: number
  /** Zyra suggests making this automatic (a long run of approvals). */
  offer: boolean
}

export const listZyraTrust = () => platformFetch<ZyraTrustClass[]>('/api/v1/ai/trust')
export const setZyraTrust = (actionType: string, level: 'ask' | 'auto', maxPerRun?: number) =>
  platformFetch<ZyraTrustClass[]>(`/api/v1/ai/trust/${encodeURIComponent(actionType)}`, {
    method: 'PUT',
    body: JSON.stringify({ level, max_per_run: maxPerRun }),
  })

export const listAgentMarketplace = () => platformFetch<AgentPluginRow[]>('/api/v1/ai/marketplace/agents')
export const installAgentMarketplace = (slug: string) =>
  platformFetch<AgentPluginRow>(`/api/v1/ai/marketplace/agents/${encodeURIComponent(slug)}/install`, { method: 'POST' })
export const uninstallAgentMarketplace = (slug: string) =>
  platformFetch<AgentPluginRow>(`/api/v1/ai/marketplace/agents/${encodeURIComponent(slug)}/uninstall`, { method: 'POST' })

export const zyraAutonomousPlan = (goal: string, simulate = true, agent?: string) =>
  platformFetch<{ goal: string; agent_id: string; steps: Array<{ title: string; detail: string }> }>(
    '/api/v1/ai/zyra/plan',
    { method: 'POST', body: JSON.stringify({ goal, simulate, agent }) },
  )

export const zyraAutonomousExecute = (goal: string, agent?: string) =>
  platformFetch<{ message: string }>('/api/v1/ai/zyra/execute', {
    method: 'POST',
    body: JSON.stringify({ goal, agent }),
  })

// --- Infrastructure Graph Brain (AI-138–147) ---

interface InfraGraphNode {
  kind: string
  id: string
  name: string
  state?: string
  health_score?: number
}

export interface InfraGraph {
  nodes: InfraGraphNode[]
  edges: Array<{ from: string; to: string; label: string }>
  node_count: number
  edge_count: number
}

export interface PathResult {
  can_reach: boolean
  explanation: string
  hops: string[]
  blockers: Array<{ kind: string; message: string; remediation: string }>
  confidence: number
  evidence: Array<{ source: string; detail: string }>
}

export interface DiagnosisReport {
  vm_id: string
  vm_name: string
  symptom: string
  severity: string
  checks: Array<{ domain: string; status: string; detail: string }>
  findings: Array<{ severity: string; message: string; domain: string }>
  recommended_actions: string[]
}

export interface RightsizingRecommendation {
  vm_id: string
  vm_name: string
  current_memory_mib: number
  suggested_memory_mib: number
  savings_usd: number
  risk: string
  action: string
  detail: string
}

export interface ActiveIncident {
  id: string
  title: string
  summary: string
  severity: string
  status: string
  affected_resources: string[]
  root_cause?: string
  created_at: string
}

export interface NlOpsPlan {
  intent: string
  summary: string
  steps: Array<{ label: string; action_type: string; review: string; risk: string }>
  risk_score: number
  dry_run: boolean
  approval_required: boolean
  action_ids: string[]
  reply: string
}

export const getInfraGraph = (params?: { host_id?: string; vm_id?: string }) => {
  const qs = new URLSearchParams()
  if (params?.host_id) qs.set('host_id', params.host_id)
  if (params?.vm_id) qs.set('vm_id', params.vm_id)
  const q = qs.toString()
  return platformFetch<InfraGraph>(`/api/v1/ai/graph${q ? `?${q}` : ''}`)
}

export const explainInfraPath = (body: { from: string; to: string; port?: number }) =>
  platformFetch<PathResult>('/api/v1/ai/graph/path', { method: 'POST', body: JSON.stringify(body) })

export const queryInfraGraph = (query: string) =>
  platformFetch<{ query: string; hits: Array<{ kind: string; id: string; name: string; detail: string }> }>(
    '/api/v1/ai/graph/query',
    { method: 'POST', body: JSON.stringify({ query }) },
  )

export const getInfraGraphAt = (timestamp: string) =>
  platformFetch<{
    timestamp: string
    nodes: InfraGraphNode[]
    edges: Array<{ from: string; to: string; label: string }>
    diff_summary: string
    current_node_count: number
    node_delta: number
    added_nodes: string[]
    removed_nodes: string[]
  }>(`/api/v1/ai/graph/at/${encodeURIComponent(timestamp)}`)

export const explainInfraObject = (kind: string, id: string) =>
  platformFetch<{ kind: string; id: string; name: string; purpose: string; risks: string[]; health_score?: number }>(
    `/api/v1/ai/graph/object/${encodeURIComponent(kind)}/${encodeURIComponent(id)}`,
  )

export const troubleshootVm = (body: { vm_id?: string; vm_name?: string; symptom?: string }) =>
  platformFetch<DiagnosisReport>('/api/v1/ai/troubleshoot', { method: 'POST', body: JSON.stringify(body) })

export const getRightsizingReport = () =>
  platformFetch<{
    recommendations: RightsizingRecommendation[]
    idle_vm_count: number
    oversized_vm_count: number
    estimated_monthly_savings_usd: number
  }>('/api/v1/ai/rightsizing/report')

export const getActiveIncidents = () => platformFetch<ActiveIncident[]>('/api/v1/ai/incidents/active')

export const getIncidentRoom = (id: string) =>
  platformFetch<{
    incident: ActiveIncident
    timeline: TimelineEntry[]
    runbook_steps: string[]
    pending_approvals: number
    correlated_count: number
  }>(`/api/v1/ai/incidents/${encodeURIComponent(id)}/room`)

export const ackIncident = (id: string) =>
  platformFetch<{ acknowledged: boolean }>(`/api/v1/ai/incidents/${encodeURIComponent(id)}/ack`, { method: 'POST' })

export interface IncidentPostmortem {
  incident_id: string
  title: string
  markdown: string
}
export const generatePostmortem = (id: string) =>
  platformFetch<IncidentPostmortem>(`/api/v1/ai/incidents/${encodeURIComponent(id)}/postmortem`, { method: 'POST' })

export const simulateTwinBatch = (scenarios: Array<{ action: string; target_kind: string; target_id: string }>) =>
  platformFetch<{ results: Array<ImpactAnalysis & { estimated_downtime_sec: number; vms_at_risk: number }> }>(
    '/api/v1/ai/twin/simulate',
    { method: 'POST', body: JSON.stringify({ scenarios }) },
  )

export const runNlOps = (query: string, dryRun = true) =>
  platformFetch<NlOpsPlan>('/api/v1/ai/nl-ops', {
    method: 'POST',
    body: JSON.stringify({ query, dry_run: dryRun }),
  })

export const getMemoryChangesBefore = (params?: { incident_id?: string; hours_before?: number }) => {
  const qs = new URLSearchParams()
  if (params?.incident_id) qs.set('incident_id', params.incident_id)
  if (params?.hours_before) qs.set('hours_before', String(params.hours_before))
  const q = qs.toString()
  return platformFetch<{ summary: string; changes: Array<{ at: string; kind: string; summary: string; actor: string }> }>(
    `/api/v1/ai/memory/changes-before${q ? `?${q}` : ''}`,
  )
}

export const getTimelineReplay = (from: string, to: string, resource?: string) => {
  const qs = new URLSearchParams({ from, to })
  if (resource) qs.set('resource', resource)
  return platformFetch<{ entries: TimelineEntry[]; graph_changes: string[] }>(
    `/api/v1/ai/timeline/replay?${qs}`,
  )
}
