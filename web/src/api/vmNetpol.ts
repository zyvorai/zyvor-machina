// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// VM network policies (CiliumNetworkPolicy schema), VM labels and packet
// flows. `host` talks to this daemon; `fleet` to the controller, which pushes
// the compiled policy to every host's machina-bpfd.

import { apiDelete, apiDeleteJson, apiGet, apiGetText, apiPost, apiPut, readJsonItemsList } from './client'
import { PLATFORM_CONTROLLER_PROXY } from './platform'

export type NetpolScope = 'host' | 'fleet'

export function netpolBase(scope: NetpolScope): string {
  return scope === 'fleet' ? `${PLATFORM_CONTROLLER_PROXY}/api/v1` : '/api/v1'
}

export interface NetpolIssue {
  path: string
  message: string
}

export interface VmNetworkPolicy {
  name: string
  kind: string
  description?: string | null
  labels: Record<string, string>
  annotations: Record<string, string>
  specs: unknown[]
  yaml: string
  selected_vms: string[]
  /** Fleet only. */
  enabled?: boolean
  generation?: number
  updated_at?: string
}

export interface NetpolEndpoint {
  name: string
  host?: string | null
  identity: number
  labels: Record<string, string>
  addresses: string[]
  ingress_enforced: boolean
  egress_enforced: boolean
  policies: string[]
}

export interface NetpolSelector {
  policy: string
  path: string
  selector: string
  vms: string[]
}

export interface NetpolPreview {
  valid: boolean
  errors: NetpolIssue[]
  warnings: NetpolIssue[]
  compile_warnings: string[]
  policies: VmNetworkPolicy[]
  rules: number
  endpoints: NetpolEndpoint[]
  selectors: NetpolSelector[]
}

export interface NetpolSync {
  at?: string | null
  ok: boolean
  error?: string | null
  skipped?: string | null
  vms: number
  rules: number
  peers: number
  warnings: string[]
}

export interface NetpolApplyResult {
  applied: string[]
  warnings: NetpolIssue[]
  sync?: NetpolSync | NetpolSync[] | Record<string, unknown>
}

export interface TraceEndpoint {
  input: string
  vm?: string | null
  identity: number
  kind: string
}

export interface TraceSide {
  direction: string
  vm?: string | null
  enforced: boolean
  verdict: 'allowed' | 'denied' | 'default-deny' | 'l7-denied' | 'auth-failed' | 'no-policy' | 'not-a-vm' | string
  rule?: string | null
  /** `required` / `test-always-fail`. */
  auth?: string | null
  /** L7 verdict for the request, or which L7 rules apply. */
  l7?: string | null
}

export interface TraceResult {
  allowed: boolean
  from: TraceEndpoint
  to: TraceEndpoint
  protocol: string
  port: number
  egress: TraceSide
  ingress: TraceSide
  summary: string
}

export interface TraceQuery {
  from: string
  to: string
  protocol?: string
  port?: number
  icmp_type?: number
  http_method?: string
  http_path?: string
  http_host?: string
  http_headers?: string[]
  server_name?: string
  dns_name?: string
  kafka_api_key?: string
  kafka_topic?: string
  kafka_client_id?: string
  kafka_api_version?: number
}

export interface NetpolHostStatus {
  host_id: string
  hostname: string
  synced_at?: string | null
  ok?: boolean
  error?: string | null
  vms?: number
  rules?: number
  peers?: number
  reachable?: boolean
  owner?: string | null
  enforcing?: boolean
  taps?: number
  missing?: string[]
  cilium?: string | null
}

export interface NetpolStatus {
  policies: number
  managed_by: 'local' | 'controller' | string
  /** Daemon only. */
  last_sync?: NetpolSync | null
  edge?: {
    owner: string
    enforcing: boolean
    flow_log: boolean
    taps: unknown[]
    peers?: number
    fqdn_rules?: number
    fqdn_cache?: number
    l7_rules?: number
    auth_entries?: number
  } | null
  enforcement?: { mode?: string; lease_remaining_secs?: number | null } | null
  bpfd_available?: boolean
  cilium?: string | null
  /** Controller only. */
  hosts?: NetpolHostStatus[]
}

export interface VmFlowRecord {
  ts: string
  host?: string | null
  iface: string
  vm: string
  direction: 'ingress' | 'egress' | string
  src: string
  src_port: number
  dst: string
  dst_port: number
  src_vm?: string | null
  dst_vm?: string | null
  src_labels: Record<string, string>
  dst_labels: Record<string, string>
  src_identity: number
  dst_identity: number
  proto: string
  tcp_flags: string
  icmp_type?: number | null
  bytes: number
  verdict: 'FORWARDED' | 'DROPPED' | 'AUDIT' | string
  drop_reason?: string | null
  policy?: string | null
  /** `http`, `kafka`, `tls` or `dns` when bpfd parsed the request. */
  l7_type?: string | null
  l7?: string | null
}

export interface FlowFilter {
  vm?: string
  from_vm?: string
  to_vm?: string
  label?: string
  ip?: string
  cidr?: string
  port?: string
  protocol?: string
  verdict?: string
  drop_reason?: string
  policy?: string
  direction?: string
  host?: string
}

export function flowQuery(f: FlowFilter, extra: Record<string, string | number> = {}): string {
  const q = new URLSearchParams()
  for (const [k, v] of Object.entries({ ...f, ...extra })) {
    if (v !== undefined && v !== null && String(v).trim() !== '') q.set(k, String(v).trim())
  }
  const s = q.toString()
  return s ? `?${s}` : ''
}

const P = '/vm-network-policies'

export async function listVmNetpols(scope: NetpolScope): Promise<{ items: VmNetworkPolicy[]; warnings: string[] }> {
  const r = await apiGet<{ items?: VmNetworkPolicy[]; warnings?: string[] }>(`${netpolBase(scope)}${P}`)
  return { items: r.items ?? [], warnings: r.warnings ?? [] }
}

export const getVmNetpolYaml = (scope: NetpolScope, name: string) =>
  apiGetText(`${netpolBase(scope)}${P}/${encodeURIComponent(name)}?format=yaml`)

export type EvidenceFormat = 'json' | 'md'

export interface EvidenceOptions {
  /** Fleet only: one project's VMs (project members may export it). */
  project?: string
  /** Matrix probes, e.g. `tcp/22,udp/53` (default tcp/22, tcp/443). */
  probes?: string
}

export function evidenceQuery(format: EvidenceFormat, o: EvidenceOptions = {}): string {
  const q = new URLSearchParams()
  if (format === 'md') q.set('format', 'md')
  if (o.project) q.set('project', o.project)
  if (o.probes?.trim()) q.set('probes', o.probes.trim())
  const qs = q.toString()
  return qs ? `?${qs}` : ''
}

/** Segmentation evidence, as served (the JSON keeps its exact bytes so the digest verifies). */
export const getEvidence = (scope: NetpolScope, format: EvidenceFormat, o: EvidenceOptions = {}) =>
  apiGetText(`${netpolBase(scope)}${P}/evidence${evidenceQuery(format, scope === 'fleet' ? o : { probes: o.probes })}`)

export function evidenceFilename(format: EvidenceFormat, now = new Date(), project?: string): string {
  const stamp = now.toISOString().slice(0, 19).replace(/[-:]/g, '')
  const tag = project ? `-${project.replace(/[^A-Za-z0-9_.-]+/g, '_')}` : ''
  return `segmentation-evidence${tag}-${stamp}.${format}`
}

export interface StoredEvidence {
  name: string
  bytes: number
  modified: string
}

/** Fleet only: reports the controller exported on its schedule. */
export const listStoredEvidence = () =>
  apiGet<{ dir: string; items: StoredEvidence[] }>(`${netpolBase('fleet')}${P}/evidence/archive`)

export const getStoredEvidence = (name: string, format: EvidenceFormat) =>
  apiGetText(`${netpolBase('fleet')}${P}/evidence/archive/${encodeURIComponent(name)}${format === 'md' ? '?format=md' : ''}`)

export const validateVmNetpol = (scope: NetpolScope, yaml: string) =>
  apiPost<NetpolPreview>(`${netpolBase(scope)}${P}/validate`, { yaml })

export const applyVmNetpol = (scope: NetpolScope, yaml: string) =>
  apiPost<NetpolApplyResult>(`${netpolBase(scope)}${P}`, { yaml })

export const deleteVmNetpol = (scope: NetpolScope, name: string) =>
  apiDelete(`${netpolBase(scope)}${P}/${encodeURIComponent(name)}`)

export const setVmNetpolEnabled = (name: string, enabled: boolean) =>
  apiPut<{ enabled: boolean }>(`${netpolBase('fleet')}${P}/${encodeURIComponent(name)}/enabled`, { enabled })

export const traceVmNetpol = (scope: NetpolScope, q: TraceQuery) =>
  apiPost<TraceResult>(`${netpolBase(scope)}${P}/trace`, q)

export const listNetpolEndpoints = async (scope: NetpolScope) =>
  (await readJsonItemsList<NetpolEndpoint>(`${netpolBase(scope)}${P}/endpoints`)).items

export const listNetpolSelectors = async (scope: NetpolScope) =>
  (await readJsonItemsList<NetpolSelector>(`${netpolBase(scope)}${P}/selectors`)).items

export const getNetpolStatus = (scope: NetpolScope) => apiGet<NetpolStatus>(`${netpolBase(scope)}${P}/status`)

export const syncFleetNetpol = () => apiPost<unknown>(`${netpolBase('fleet')}${P}/sync`)

/** A `toFQDNs` name → address binding learned from a DNS reply to a VM. */
export interface FqdnEntry {
  name: string
  address: string
  /** 0 = not in force (no current rule matches). */
  identity: number
  vm: string
  expires_in_secs: number
  patterns: string[]
  /** Controller only. */
  hostname?: string
}

export const listFqdnCache = async (scope: NetpolScope) =>
  (await readJsonItemsList<FqdnEntry>(`${netpolBase(scope)}${P}/fqdn-cache`)).items

/** One mutual-authentication entry (identity pair). */
export interface AuthEntry {
  subject: string
  subject_identity: number
  peer: string
  peer_identity: number
  mode: 'required' | 'test-always-fail' | string
  state: string
  expires_in_secs: number
  /** Controller only. */
  hostname?: string
}

export const listAuthTable = async (scope: NetpolScope) =>
  (await readJsonItemsList<AuthEntry>(`${netpolBase(scope)}${P}/auth`)).items

export const listFlows = async (scope: NetpolScope, f: FlowFilter, limit = 500) =>
  (await readJsonItemsList<VmFlowRecord>(`${netpolBase(scope)}/flows${flowQuery(f, { limit })}`)).items

export const flowStreamUrl = (scope: NetpolScope, f: FlowFilter, last = 100) =>
  `${netpolBase(scope)}/flows/stream${flowQuery(f, { last })}`

/** One normalised L7 request on a flow edge. */
export interface VmFlowL7Stat {
  kind: string
  request: string
  count: number
  denied: number
  /** `2xx` / `4xx` / `5xx` counts — proxied HTTP only. */
  status?: Record<string, number>
  latency_n: number
  latency_ms_total: number
  latency_ms_max: number
}

/** Flows folded by source, destination, direction, port and verdict (7-day history). */
export interface VmFlowEdge {
  host?: string | null
  src: string
  dst: string
  src_vm?: string | null
  dst_vm?: string | null
  /** `host`, `world`, `remote-node` or a CIDR when the side is not a VM. */
  src_entity?: string | null
  dst_entity?: string | null
  src_labels?: Record<string, string>
  dst_labels?: Record<string, string>
  direction: string
  proto: string
  port: number
  verdict: string
  drop_reason?: string | null
  policy?: string | null
  count: number
  bytes: number
  first_seen: string
  last_seen: string
  l7?: VmFlowL7Stat[]
}

export interface VmFlowAlert {
  ts: string
  host?: string | null
  kind: 'port_scan' | 'host_sweep' | 'deny_burst' | 'new_peer' | 'threat_domain' | 'new_domain' | string
  severity: 'low' | 'medium' | 'high' | string
  src: string
  src_vm?: string | null
  dst?: string | null
  detail: string
  count: number
}

export const listFlowEdges = async (scope: NetpolScope, vm?: string) =>
  (await readJsonItemsList<VmFlowEdge>(`${netpolBase(scope)}/flows/edges${flowQuery({ vm })}`)).items

export const resetFlowEdges = (scope: NetpolScope) => apiDelete(`${netpolBase(scope)}/flows/edges`)

export const listFlowAlerts = async (scope: NetpolScope, limit = 200) =>
  (await readJsonItemsList<VmFlowAlert>(`${netpolBase(scope)}/flows/alerts${flowQuery({}, { limit })}`)).items

export interface LearnOptions {
  vm?: string
  selector?: Record<string, string>
  group_by?: string
  min_count?: number
  l7?: boolean
  lock_unobserved?: boolean
}

export interface LearnResult {
  yaml: string
  policies: Array<{ name: string; vms: string[]; ingress_rules: number; egress_rules: number }>
  edges_used: number
  edges_skipped: number
  notes: string[]
}

export const learnVmNetpol = (scope: NetpolScope, o: LearnOptions) =>
  apiPost<LearnResult>(`${netpolBase(scope)}${P}/learn`, o)

export interface ReplayChange {
  src: string
  dst: string
  proto: string
  port: number
  request?: string | null
  flows: number
  last_seen: string
  before: string
  after: string
}

export interface ReplayResult {
  evaluated: number
  unchanged: number
  /** Endpoints the tracer could not resolve, e.g. VMs deleted since. */
  not_evaluated?: number
  would_break: ReplayChange[]
  would_allow: ReplayChange[]
  flows_breaking: number
}

export const replayVmNetpol = (scope: NetpolScope, yaml: string) =>
  apiPost<ReplayResult>(`${netpolBase(scope)}${P}/replay`, { yaml })

export interface NetpolDraft {
  yaml: string
  /** `rules` (the sentence parser) or `llm` (Zyvor). */
  source: 'rules' | 'llm'
  notes: string[]
  unparsed: string[]
  preview: NetpolPreview
  replay: ReplayResult
}

export interface DraftPending {
  id: string
  label: string
  requested_by: string
  created_at: string
  object_ref: { yaml: string; prompt?: string; policies?: string[] }
}

export const DRAFT_EXAMPLES = [
  'Only web servers can reach the db on port 5432',
  'Allow web-1 to reach db-1 on ssh',
  'Block db-1 from reaching the internet',
  'web-1 can reach github.com on https',
  'Isolate app=db',
]

/** Host: the sentence parser. Fleet: Zyvor's LLM when configured, else the parser. */
export const draftVmNetpol = (scope: NetpolScope, prompt: string, rulesOnly = false) =>
  apiPost<NetpolDraft>(
    `${netpolBase(scope)}${P}/draft`,
    scope === 'fleet' ? { prompt, rules_only: rulesOnly } : { prompt },
  )

export const proposeVmNetpol = (yaml: string, prompt: string) =>
  apiPost<{ pending: DraftPending }>(`${netpolBase('fleet')}${P}/draft/propose`, { yaml, prompt })

export async function listDraftPending(): Promise<DraftPending[]> {
  const r = await apiGet<{ pending?: DraftPending[] }>(`${netpolBase('fleet')}${P}/draft`)
  return r.pending ?? []
}

export type ProjectIsolation = 'inherit' | 'isolated' | 'open'

export interface EgressAllow {
  /** CIDR, IP, domain, `*.domain` or `world`. */
  to: string
  ports?: string[]
}

export interface ProjectNet {
  project: string
  isolation: ProjectIsolation
  allow_host: boolean
  egress_restricted: boolean
  egress_allow: EgressAllow[]
  /** Host name or id → egress IP on that host: IPv4, IPv6, or both comma-separated. */
  egress_ips: Record<string, string>
  /** Block internet egress on hosts without one of the egress IPs. */
  egress_ip_required?: boolean
  updated_by?: string
  updated_at?: string
}

export interface ProjectRow {
  project: string
  explicit: boolean
  settings: ProjectNet
  isolated: boolean
  allow_host: boolean
  vms: string[]
  policies: string[]
  /** The project spans hosts whose VM subnets are NATed per host. */
  cross_host_nat?: { project: string; hosts: string[]; subnets: string[] } | null
  egress_gaps?: EgressGap[]
}

export interface EgressGap {
  project: string
  vm: string
  host: string
  blocked: boolean
}

export interface ProjectPreview {
  project: string
  describe: string
  summary: string
  replay: ReplayResult
}

export interface ProjectPending {
  pending: { id: string; label?: string; requested_by?: string }
  preview: string
}

export function isProjectPending(r: unknown): r is ProjectPending {
  return typeof r === 'object' && r !== null && 'pending' in r
}

export interface EgressHostStatus {
  hostname: string
  host_id?: string
  active: boolean
  rules: Array<{ project: string; egress_ip: string; sources: string[] }>
  exclude: string[]
  skipped: string[]
  error?: string
}

/** Fleet only. */
export const listProjects = () =>
  apiGet<{ default: ProjectNet; items: ProjectRow[]; warnings?: string[] }>(`${netpolBase('fleet')}${P}/projects`)

const projectPath = (project: string) => `${netpolBase('fleet')}${P}/projects/${encodeURIComponent(project)}`

/** Applies, or with `propose` (or when the controller requires approval) returns a pending approval. */
export const setProject = (project: string, s: Omit<ProjectNet, 'project'>, propose = false) =>
  apiPut<{ project: ProjectNet } | ProjectPending>(`${projectPath(project)}${propose ? '?propose=1' : ''}`, s)

export const resetProject = (project: string, propose = false) =>
  apiDeleteJson<unknown>(`${projectPath(project)}${propose ? '?propose=1' : ''}`)

/** What a change would do to recorded traffic; nothing is applied. */
export const previewProject = (project: string, s: Omit<ProjectNet, 'project'>) =>
  apiPost<ProjectPreview>(`${projectPath(project)}/preview`, { project, ...s })

export const listEgressIps = () =>
  apiGet<{ items: EgressHostStatus[]; errors: Array<{ hostname: string; error: string }> }>(`${netpolBase('fleet')}${P}/egress-ips`)

export function describeEgress(s: ProjectNet): string {
  if (!s.egress_restricted) return 'Any destination'
  if (s.egress_allow.length === 0) return 'Only the project, host and DNS'
  return s.egress_allow.map((e) => (e.ports?.length ? `${e.to} (${e.ports.join(', ')})` : e.to)).join(', ')
}

export function splitPorts(s: string): string[] {
  return s.split(/[\s,]+/).map((x) => x.trim()).filter(Boolean)
}

export interface VmQuarantineAllow {
  direction: 'ingress' | 'egress'
  /** `host`, `world`, `any` or a VM name. */
  peer: string
  proto?: string
  port?: number
}

export interface VmQuarantine {
  vm: string
  since: string
  until: string
  remaining_secs: number
  allow: VmQuarantineAllow[]
  reason?: string
  by?: string
  /** Empty: the VM is not running on that host. */
  taps: string[]
  hostname?: string
}

export interface QuarantineOptions {
  secs: number
  allow_host_ssh?: boolean
  allow?: VmQuarantineAllow[]
  reason?: string
}

export const QUARANTINE_DURATIONS: Array<{ secs: number; label: string }> = [
  { secs: 900, label: '15 minutes' },
  { secs: 3600, label: '1 hour' },
  { secs: 4 * 3600, label: '4 hours' },
  { secs: 24 * 3600, label: '24 hours' },
]

export const listQuarantines = async (scope: NetpolScope) =>
  (await readJsonItemsList<VmQuarantine>(`${netpolBase(scope)}${P}/quarantines`)).items

/** Quarantines are keyed by VM name on both the daemon and the controller. */
export const quarantineVm = (scope: NetpolScope, vm: string, o: QuarantineOptions) =>
  apiPost<unknown>(`${netpolBase(scope)}/vms/${encodeURIComponent(vm)}/quarantine`, o)

export const releaseQuarantine = (scope: NetpolScope, vm: string) =>
  apiDelete(`${netpolBase(scope)}/vms/${encodeURIComponent(vm)}/quarantine`)

/** One row per VM: a fleet quarantine is held by every host; `hosts` are those running it. */
export function groupQuarantines(items: VmQuarantine[]): Array<VmQuarantine & { hosts: string[] }> {
  const by = new Map<string, VmQuarantine & { hosts: string[] }>()
  for (const q of items) {
    const g = by.get(q.vm) ?? { ...q, taps: [], hosts: [] }
    if (q.taps.length > 0) {
      g.taps = [...g.taps, ...q.taps]
      if (q.hostname) g.hosts.push(q.hostname)
    }
    if (q.remaining_secs > g.remaining_secs) Object.assign(g, { remaining_secs: q.remaining_secs, until: q.until })
    by.set(q.vm, g)
  }
  return [...by.values()].sort((a, b) => a.vm.localeCompare(b.vm))
}

export function describeAllow(a: VmQuarantineAllow): string {
  const proto = a.proto || 'any'
  return `${a.direction} ${a.peer} ${proto}${a.port ? `/${a.port}` : ''}`
}

export function formatRemaining(secs: number): string {
  if (secs <= 0) return 'expiring'
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  if (h > 0) return `${h}h ${m}m left`
  if (m > 0) return `${m}m left`
  return `${secs}s left`
}

export interface JitGrant {
  name: string
  /** A VM name or `host`. */
  from: string
  to: string
  /** 0 = every port. */
  port: number
  protocol: string
  expires_at: string
  remaining_secs: number
  reason?: string
  granted_by?: string
}

export interface JitRequest {
  from: string
  to: string
  port?: number
  protocol?: string
  secs: number
  reason?: string
}

export interface JitPending {
  id: string
  label: string
  requested_by: string
  created_at: string
  object_ref: JitRequest
}

export const JIT_DURATIONS: Array<{ secs: number; label: string }> = [
  { secs: 900, label: '15 minutes' },
  { secs: 3600, label: '1 hour' },
  { secs: 4 * 3600, label: '4 hours' },
  { secs: 8 * 3600, label: '8 hours' },
]

export async function listJit(scope: NetpolScope): Promise<{ items: JitGrant[]; pending: JitPending[] }> {
  const r = await apiGet<{ items?: JitGrant[]; pending?: JitPending[] }>(`${netpolBase(scope)}${P}/jit`)
  return { items: r.items ?? [], pending: r.pending ?? [] }
}

/** Host: an admin grant. Fleet: an approval request unless `grant` (admins). */
export const requestJit = (scope: NetpolScope, req: JitRequest, grant = false) =>
  apiPost<{ granted?: JitGrant; policy?: string; pending?: JitPending }>(
    `${netpolBase(scope)}${P}/jit`,
    scope === 'fleet' ? { ...req, grant } : req,
  )

export const approveJit = (id: string) =>
  apiPost<unknown>(`${PLATFORM_CONTROLLER_PROXY}/api/v1/ai/actions/${encodeURIComponent(id)}/execute`, {})

export const rejectJit = (id: string) =>
  apiPost<unknown>(`${PLATFORM_CONTROLLER_PROXY}/api/v1/ai/actions/${encodeURIComponent(id)}/reject`, {})

export const revokeJit = (scope: NetpolScope, name: string) => deleteVmNetpol(scope, name)

export function describeJit(g: Pick<JitGrant, 'from' | 'to' | 'port' | 'protocol'>): string {
  const proto = (g.protocol || 'TCP').toLowerCase()
  return g.port ? `${g.from} → ${g.to}:${g.port}/${proto}` : `${g.from} → ${g.to} (every port)`
}

export interface ThreatFeed {
  name: string
  /** URL it is fetched from; empty for an inline list. */
  source: string
  block: boolean
  /** Host: count. Fleet rows carry `domain_count`. */
  domains?: number
  domain_count?: number
  updated?: string
  updated_at?: string
}

export interface ThreatBlock {
  address: string
  domain: string
  feed: string
  vm: string
  expires_in_secs: number
  hostname?: string
}

export interface ThreatStatus {
  feeds: ThreatFeed[]
  blocked: ThreatBlock[]
  watched_vms: number
  errors?: Array<{ hostname: string; error: string }>
}

/** One source: a URL, a domain list or feed text (hosts / Adblock / one per line). */
export interface ThreatFeedInput {
  url?: string
  domains?: string[]
  text?: string
  block: boolean
}

export async function listThreatFeeds(scope: NetpolScope): Promise<ThreatStatus> {
  const r = await apiGet<Partial<ThreatStatus>>(`${netpolBase(scope)}${P}/threat-feeds`)
  return { feeds: r.feeds ?? [], blocked: r.blocked ?? [], watched_vms: r.watched_vms ?? 0, errors: r.errors ?? [] }
}

export const setThreatFeed = (scope: NetpolScope, name: string, body: ThreatFeedInput) =>
  apiPut<unknown>(`${netpolBase(scope)}${P}/threat-feeds/${encodeURIComponent(name)}`, body)

export const refreshThreatFeed = (scope: NetpolScope, name: string) =>
  apiPost<unknown>(`${netpolBase(scope)}${P}/threat-feeds/${encodeURIComponent(name)}/refresh`, {})

export const removeThreatFeed = (scope: NetpolScope, name: string) =>
  apiDelete(`${netpolBase(scope)}${P}/threat-feeds/${encodeURIComponent(name)}`)

export const threatDomainCount = (f: ThreatFeed) => f.domains ?? f.domain_count ?? 0

export const THREAT_FEED_NAME = /^[A-Za-z0-9._-]{1,64}$/

/** Domains typed one per line / comma separated. */
export function splitDomains(text: string): string[] {
  return text
    .split(/[\s,]+/)
    .map((d) => d.trim())
    .filter(Boolean)
}

/** Daemon: VM name. Controller: VM id. */
export async function getVmLabels(scope: NetpolScope, vm: string): Promise<Record<string, string>> {
  const r = await apiGet<{ labels?: Record<string, string> }>(`${netpolBase(scope)}/vms/${encodeURIComponent(vm)}/labels`)
  return r.labels ?? {}
}

export async function setVmLabels(scope: NetpolScope, vm: string, labels: Record<string, string>): Promise<Record<string, string>> {
  const r = await apiPut<{ labels?: Record<string, string> }>(`${netpolBase(scope)}/vms/${encodeURIComponent(vm)}/labels`, { labels })
  return r.labels ?? labels
}

/** Kubernetes label syntax, same rules the servers enforce. */
export function labelError(key: string, value: string): string | null {
  const name = /^[A-Za-z0-9]([-A-Za-z0-9_.]{0,61}[A-Za-z0-9])?$/
  const parts = key.split('/')
  if (parts.length > 2 || !key) return 'invalid key'
  const [prefix, n] = parts.length === 2 ? parts : [null, parts[0]]
  if (prefix !== null && (!prefix || prefix.length > 253 || !/^[a-z0-9]([-a-z0-9.]*[a-z0-9])?$/.test(prefix))) return 'invalid key prefix'
  if (!name.test(n)) return 'invalid key name'
  if (value && !name.test(value)) return 'invalid value'
  return null
}

export const NETPOL_TEMPLATES: Array<{ id: string; label: string; yaml: string }> = [
  {
    id: 'web-db',
    label: 'Web → DB on 5432',
    yaml: `apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: db-from-web
spec:
  description: Only web VMs reach the database, on 5432/TCP
  endpointSelector:
    matchLabels:
      app: db
  ingress:
    - fromEndpoints:
        - matchLabels:
            app: web
      toPorts:
        - ports:
            - port: "5432"
              protocol: TCP
`,
  },
  {
    id: 'default-deny',
    label: 'Default deny (ingress + egress)',
    yaml: `apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: default-deny
spec:
  description: Isolate every VM labelled env=prod; add allow policies on top
  endpointSelector:
    matchLabels:
      env: prod
  ingress:
    - {}
  egress:
    - {}
`,
  },
  {
    id: 'egress-dns-https',
    label: 'Egress: DNS + HTTPS to world',
    yaml: `apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: egress-dns-https
spec:
  endpointSelector:
    matchLabels:
      tier: frontend
  egress:
    - toEntities:
        - world
      toPorts:
        - ports:
            - port: "53"
              protocol: UDP
            - port: "443"
              protocol: TCP
`,
  },
  {
    id: 'egress-fqdn',
    label: 'Egress: only named services (toFQDNs)',
    yaml: `apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: egress-fqdn
spec:
  description: Build VMs reach only GitHub and the package mirrors, by name
  endpointSelector:
    matchLabels:
      role: build
  egress:
    - toEntities:
        - world
      toPorts:
        - ports:
            - port: "53"
              protocol: UDP
    - toFQDNs:
        - matchName: github.com
        - matchPattern: "*.githubusercontent.com"
        - matchPattern: "**.debian.org"
      toPorts:
        - ports:
            - port: "443"
              protocol: TCP
`,
  },
  {
    id: 'l7-http',
    label: 'L7: HTTP methods and paths',
    yaml: `apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: api-read-only
spec:
  description: Web VMs may only GET /api/v1/* and POST /login on the API VMs
  endpointSelector:
    matchLabels:
      app: api
  ingress:
    - fromEndpoints:
        - matchLabels:
            app: web
      toPorts:
        - ports:
            - port: "80"
              protocol: TCP
          rules:
            http:
              - method: GET
                path: "/api/v1/.*"
              - method: POST
                path: /login
                headers:
                  - "X-Requested-With: machina"
`,
  },
  {
    id: 'l7-kafka',
    label: 'L7: Kafka produce to one topic',
    yaml: `apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: orders-producer
spec:
  endpointSelector:
    matchLabels:
      app: kafka
  ingress:
    - fromEndpoints:
        - matchLabels:
            app: checkout
      toPorts:
        - ports:
            - port: "9092"
              protocol: TCP
          rules:
            kafka:
              - role: produce
                topic: orders
`,
  },
  {
    id: 'l7-dns-tls',
    label: 'L7: DNS names + TLS SNI',
    yaml: `apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: egress-dns-sni
spec:
  description: Resolve only *.example.com; HTTPS only to api.example.com (by SNI)
  endpointSelector:
    matchLabels:
      tier: frontend
  egress:
    - toEntities:
        - world
      toPorts:
        - ports:
            - port: "53"
              protocol: ANY
          rules:
            dns:
              - matchPattern: "*.example.com"
        - ports:
            - port: "443"
              protocol: TCP
          serverNames:
            - api.example.com
`,
  },
  {
    id: 'services',
    label: 'Egress to services (toServices)',
    yaml: `apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: web-to-services
spec:
  description: Web VMs reach the shop load balancer and the data-tier services
  endpointSelector:
    matchLabels:
      app: web
  egress:
    - toServices:
        - k8sService:
            serviceName: shop-lb
            namespace: shop
    - toServices:
        - k8sServiceSelector:
            selector:
              matchLabels:
                tier: data
      toPorts:
        - ports:
            - port: "5432"
              protocol: TCP
`,
  },
  {
    id: 'tls-intercept',
    label: 'TLS interception + header rewrites',
    yaml: `apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: web-to-api-intercepted
spec:
  description: Web VMs' HTTPS to the API is decrypted, checked and tagged
  endpointSelector:
    matchLabels:
      app: web
  egress:
    - toEndpoints:
        - matchLabels:
            app: api
      toPorts:
        - ports:
            - port: "443"
              protocol: TCP
          # Files on each host: /etc/machina/netpol-secrets/<namespace>/<name>/
          terminatingTLS:
            secret: {namespace: shop, name: api-intercept}
          originatingTLS:
            secret: {namespace: shop, name: api-upstream}
            trustedCA: ca.crt
          rules:
            http:
              - path: "/v1/.*"
                headerMatches:
                  - {name: X-Team, value: web, mismatch: REPLACE}
                  - {name: X-Debug, value: "0", mismatch: DELETE}
`,
  },
  {
    id: 'cidr-group',
    label: 'CIDR group + toGroups',
    yaml: `apiVersion: cilium.io/v2alpha1
kind: CiliumCIDRGroup
metadata:
  name: partner-nets
  labels:
    tier: partner
  annotations:
    machina.io/group-id: sg-partners
spec:
  externalCIDRs:
    - 198.51.100.0/24
    - 2001:db8:100::/48
---
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: partners-https
spec:
  endpointSelector:
    matchLabels:
      app: gateway
  egress:
    - toGroups:
        - aws:
            securityGroupsIds: [sg-partners]
      toPorts:
        - ports:
            - port: "443"
              protocol: TCP
    - toCIDRSet:
        - cidrGroupRef: partner-nets
      toPorts:
        - ports:
            - port: "22"
              protocol: TCP
`,
  },
  {
    id: 'auth',
    label: 'Mutual authentication',
    yaml: `apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: db-mutual-auth
spec:
  description: Only authenticated web VMs reach the database
  endpointSelector:
    matchLabels:
      app: db
  ingress:
    - fromEndpoints:
        - matchLabels:
            app: web
      authentication:
        mode: required
      toPorts:
        - ports:
            - port: "5432"
              protocol: TCP
`,
  },
  {
    id: 'deny-metadata',
    label: 'Deny a CIDR (egressDeny)',
    yaml: `apiVersion: cilium.io/v2
kind: CiliumClusterwideNetworkPolicy
metadata:
  name: deny-metadata
spec:
  description: No VM may reach the link-local metadata range or 10.66.0.0/16
  endpointSelector: {}
  egressDeny:
    - toCIDRSet:
        - cidr: 169.254.0.0/16
        - cidr: 10.66.0.0/16
`,
  },
  {
    id: 'icmp',
    label: 'Allow ping from the host',
    yaml: `apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: ping-from-host
spec:
  endpointSelector: {}
  ingress:
    - fromEntities:
        - host
      icmps:
        - fields:
            - type: EchoRequest
              family: IPv4
`,
  },
  {
    id: 'ssh-range',
    label: 'SSH from a CIDR, except one subnet',
    yaml: `apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: ssh-from-office
spec:
  endpointSelector:
    matchExpressions:
      - key: role
        operator: In
        values: [bastion, admin]
  ingress:
    - fromCIDRSet:
        - cidr: 192.168.0.0/16
          except:
            - 192.168.99.0/24
      toPorts:
        - ports:
            - port: "22"
              protocol: TCP
`,
  },
]
