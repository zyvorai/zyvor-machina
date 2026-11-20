// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { readJsonObject } from './client'

const API = '/api/v1'

export interface IntegrationsStatus {
  kubevirt: {
    exec_enabled: boolean
    default_namespace: string
    default_storage_class: string
    routes: Record<string, string>
  }
  automation: {
    worker_interval_secs: number
    last_tick_unix: number | null
    alert_rules_total: number
    alert_rules_enabled: number
    alerts_unacknowledged: number
    routes: Record<string, string>
  }
  k8s: {
    kubeconfig_auto_selected: string | null
    kubectl_args_prefix: string[]
    inventory_history_enabled: boolean
    routes: Record<string, string>
  }
  run_as_user: {
    enabled: boolean
    mode: string
    impersonation_active: boolean
    status_url: string
  }
}

export const getIntegrationsStatus = () =>
  readJsonObject<IntegrationsStatus>(`${API}/integrations/status`)
