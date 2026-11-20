// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { readJsonObject } from './client'

const API = '/api/v1'

export interface HttpTraceSpan {
  trace_id: string
  span_id: string
  method: string
  route: string
  status: number
  duration_ms: number
  timestamp_ms: number
}

interface MetricsTracesResponse {
  traces: HttpTraceSpan[]
  count: number
}

export const getMetricsTraces = (limit = 32) =>
  readJsonObject<MetricsTracesResponse>(`${API}/metrics/traces?limit=${limit}`)
