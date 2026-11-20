// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'

export interface SocOverview {
  open_alerts: number
  events_24h: number
  critical_alerts: number
}

export interface SocEvent {
  id: string
  occurred_at: string
  source: string
  category: string
  severity: string
  host_id?: string
  vm_id?: string
  actor?: string
  summary: string
}

export interface SocAlert {
  id: string
  rule_id?: string
  title: string
  severity: string
  status: string
  assigned_to?: string
  first_seen: string
  last_seen: string
  event_count: number
}

export interface SocRule {
  id: string
  name: string
  description: string
  enabled: boolean
  severity: string
  query_json: Record<string, unknown>
  throttle_minutes: number
  builtin: boolean
}

export interface SocIntegration {
  id: string
  integration_type: string
  name: string
  enabled: boolean
  config: Record<string, unknown>
  last_success_at?: string | null
  last_error?: string | null
}

export interface SocPlaybook {
  id: string
  name: string
  description: string
  enabled: boolean
  trigger_json: Record<string, unknown>
  steps_json: SocPlaybookStep[]
}

export type SocPlaybookStep =
  | { type: 'webhook'; url?: string; url_from_setting?: string; body?: Record<string, unknown> }
  | { type: 'notify' }

export interface SocPlaybookStepDraft {
  type: 'webhook' | 'notify'
  url: string
  useGlobalWebhook: boolean
}

interface MitreTag {
  id: string
  name: string
}

interface SocLinkedEvent {
  id: string
  occurred_at: string
  source: string
  category: string
  severity: string
  summary: string
  ecs_json: Record<string, unknown>
}

interface SocPlaybookRunDetail {
  id: string
  playbook_id: string
  playbook_name?: string
  status: string
  started_at: string
  finished_at?: string
  step_results: unknown[]
  error?: string
}

export interface SocAlertDetail extends SocAlert {
  rule_name?: string
  dedupe_key: string
  detail_json: Record<string, unknown>
  mitre_tags: MitreTag[]
  linked_events: SocLinkedEvent[]
  playbook_runs: SocPlaybookRunDetail[]
}

interface SocSettings {
  webhook_url: string
}

export interface SocPlaybookRun {
  id: string
  playbook_id: string
  alert_id?: string
  status: string
  started_at: string
  finished_at?: string
}

interface SocIngestCycle {
  ingest: { firewall: number; audit: number; platform: number; native_bpf: number }
  alerts_fired: number
  forwarded: number
}

export interface AsmSummary {
  exposure_score: number
  firewall_targets: number
  high_risk_nodes: number
  open_port_findings: Array<{ kind: string; resource: string; detail: string; severity: string }>
  recommendations: string[]
}

export const getSocOverview = () => platformFetch<SocOverview>('/api/v1/soc/overview')

export const getSocEvents = (limit = 100) =>
  platformFetch<SocEvent[]>(`/api/v1/soc/events?limit=${limit}`)

export const getSocAlerts = (params?: { status?: string; limit?: number }) => {
  const qs = new URLSearchParams()
  if (params?.status) qs.set('status', params.status)
  if (params?.limit) qs.set('limit', String(params.limit))
  const q = qs.toString()
  return platformFetch<SocAlert[]>(`/api/v1/soc/alerts${q ? `?${q}` : ''}`)
}

export const getSocAlert = (id: string) => platformFetch<SocAlertDetail>(`/api/v1/soc/alerts/${id}`)

export const patchSocAlert = (id: string, body: { status?: string; assigned_to?: string }) =>
  platformFetch<SocAlert>(`/api/v1/soc/alerts/${id}`, { method: 'PATCH', body: JSON.stringify(body) })

export const getSocRules = () => platformFetch<SocRule[]>('/api/v1/soc/rules')

export const patchSocRule = (id: string, body: { enabled?: boolean }) =>
  platformFetch<SocRule>(`/api/v1/soc/rules/${id}`, { method: 'PATCH', body: JSON.stringify(body) })

export const testSocRule = (id: string, hours = 24) =>
  platformFetch<{ match_count: number; would_fire: boolean }>(
    `/api/v1/soc/rules/${id}/test?hours=${hours}`,
    { method: 'POST' },
  )

export const getAsmSummary = () => platformFetch<AsmSummary>('/api/v1/soc/asm/summary')

export const getSplunkIntegration = () =>
  platformFetch<SocIntegration>('/api/v1/soc/integrations/splunk')

export const putSplunkIntegration = (body: {
  url: string
  token: string
  index?: string
  sourcetype_events?: string
  sourcetype_alerts?: string
  host?: string
  enabled?: boolean
}) =>
  platformFetch<SocIntegration>('/api/v1/soc/integrations/splunk', {
    method: 'PUT',
    body: JSON.stringify(body),
  })

export const testSplunkIntegration = () =>
  platformFetch<{ ok: boolean; message: string }>('/api/v1/soc/integrations/splunk/test', {
    method: 'POST',
  })

export const getSocIntegrations = () => platformFetch<SocIntegration[]>('/api/v1/soc/integrations')

export const patchSocIntegration = (
  integrationType: string,
  body: { enabled?: boolean; config_json?: Record<string, unknown> },
) =>
  platformFetch<SocIntegration>(`/api/v1/soc/integrations/${integrationType}`, {
    method: 'PATCH',
    body: JSON.stringify(body),
  })

export const testSocIntegration = (integrationType: string) =>
  platformFetch<{ ok: boolean; message: string }>(
    `/api/v1/soc/integrations/${integrationType}/test`,
    { method: 'POST' },
  )

export const replaySocForward = (hours = 24) =>
  platformFetch<{ forwarded: number }>(`/api/v1/soc/forward/replay?hours=${hours}`, { method: 'POST' })

export const runSocIngestCycle = () =>
  platformFetch<SocIngestCycle>('/api/v1/soc/ingest/run', { method: 'POST' })

export const getSocPlaybooks = () => platformFetch<SocPlaybook[]>('/api/v1/soc/playbooks')

export const getSocPlaybookRuns = (limit = 50) =>
  platformFetch<SocPlaybookRun[]>(`/api/v1/soc/playbook-runs?limit=${limit}`)

export const getSocPlaybook = (id: string) => platformFetch<SocPlaybook>(`/api/v1/soc/playbooks/${id}`)

export const createSocPlaybook = (body: {
  name: string
  description?: string
  enabled?: boolean
  trigger_json?: Record<string, unknown>
  steps_json?: SocPlaybookStep[]
}) =>
  platformFetch<SocPlaybook>('/api/v1/soc/playbooks', { method: 'POST', body: JSON.stringify(body) })

export const patchSocPlaybook = (
  id: string,
  body: {
    description?: string
    enabled?: boolean
    trigger_json?: Record<string, unknown>
    steps_json?: SocPlaybookStep[]
  },
) =>
  platformFetch<SocPlaybook>(`/api/v1/soc/playbooks/${id}`, {
    method: 'PATCH',
    body: JSON.stringify(body),
  })

export const deleteSocPlaybook = (id: string) =>
  platformFetch<{ deleted: boolean }>(`/api/v1/soc/playbooks/${id}`, { method: 'DELETE' })

export const getSocSettings = () => platformFetch<SocSettings>('/api/v1/soc/settings')

export const patchSocSettings = (body: { webhook_url?: string }) =>
  platformFetch<SocSettings>('/api/v1/soc/settings', { method: 'PATCH', body: JSON.stringify(body) })
