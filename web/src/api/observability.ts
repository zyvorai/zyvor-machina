// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { readJsonObject, apiPut } from './client'

const API = '/api/v1'

export interface ObservabilitySettingsView {
  otlp: {
    enabled: boolean
    endpoint: string
    interval_secs: number
    export_metrics: boolean
    export_logs: boolean
    export_traces: boolean
    authorization: string
    authorization_set: boolean
  }
  metrics_history: {
    remote_write_url: string
    remote_write_authorization: string
    remote_write_authorization_set: boolean
  }
  audit: {
    sign_lines: boolean
  }
  config_path: string
}

interface ObservabilitySettingsPatch {
  otlp_enabled?: boolean
  otlp_endpoint?: string
  otlp_interval_secs?: number
  otlp_export_metrics?: boolean
  otlp_export_logs?: boolean
  otlp_export_traces?: boolean
  otlp_authorization?: string
  metrics_history_remote_write_url?: string
  metrics_history_remote_write_authorization?: string
  audit_sign_lines?: boolean
}

export function getObservabilitySettings() {
  return readJsonObject<ObservabilitySettingsView>(`${API}/system/observability-settings`)
}

export function putObservabilitySettings(patch: ObservabilitySettingsPatch) {
  return apiPut<{
    status: string
    restart_recommended: boolean
    workers_reloaded?: boolean
    note: string
    settings: ObservabilitySettingsView
  }>(`${API}/system/observability-settings`, patch)
}

interface AuditVerifyReport {
  total_lines: number
  signed_valid: number
  signed_invalid: number
  unsigned: number
  invalid_samples: string[]
}

export function verifyAuditLog(maxLines = 50000) {
  return readJsonObject<AuditVerifyReport>(
    `${API}/audit/verify?max_lines=${encodeURIComponent(String(maxLines))}`,
  )
}
