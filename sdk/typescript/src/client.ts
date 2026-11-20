// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

export type MachinaClientOptions = {
  baseUrl: string
  token?: string
  username?: string
  password?: string
}

export class MachinaClient {
  private baseUrl: string
  private headers: Record<string, string>

  constructor(opts: MachinaClientOptions) {
    this.baseUrl = opts.baseUrl.replace(/\/$/, '')
    const headers: Record<string, string> = { 'Content-Type': 'application/json' }
    if (opts.token) {
      headers.Authorization = `Bearer ${opts.token}`
    } else if (opts.username && opts.password) {
      headers.Authorization = `Basic ${btoa(`${opts.username}:${opts.password}`)}`
    }
    this.headers = headers
  }

  private async fetch<T>(path: string, init?: RequestInit): Promise<T> {
    const res = await fetch(`${this.baseUrl}${path}`, {
      ...init,
      headers: { ...this.headers, ...(init?.headers as Record<string, string> | undefined) },
    })
    if (!res.ok) {
      throw new Error(`Machina API ${path} failed: HTTP ${res.status}`)
    }
    return res.json() as Promise<T>
  }

  health() {
    return this.fetch<{ status: string }>('/api/v1/health')
  }

  listHosts() {
    return this.fetch<Array<{ id: string; hostname: string; state: string }>>('/api/v1/hosts')
  }

  listVms(managed?: boolean) {
    const q = managed === undefined ? '' : `?managed=${managed}`
    return this.fetch<Array<{ id: string; name: string; observed_state: string }>>(`/api/v1/vms${q}`)
  }

  developerOverview() {
    return this.fetch<{ summary: string; openapi_url: string }>('/api/v1/developer/overview')
  }

  observabilityOverview() {
    return this.fetch<{ summary: string; slos: Array<{ name: string; current_pct: number; status: string }> }>(
      '/api/v1/observability/overview',
    )
  }

  operationsOverview() {
    return this.fetch<{ summary: string; runbook_count: number }>('/api/v1/operations/overview')
  }

  executeRunbook(incident: string, context: Record<string, unknown> = {}) {
    return this.fetch<{ summary: string; steps: string[] }>(
      `/api/v1/operations/runbooks/${encodeURIComponent(incident)}/execute`,
      { method: 'POST', body: JSON.stringify({ context }) },
    )
  }
}

export { MachinaClient as default }
