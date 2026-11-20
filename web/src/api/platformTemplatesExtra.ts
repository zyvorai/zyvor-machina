// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Template git sync, approval, publish-from-VM (Phase A golden images).

import { platformFetch, type PlatformTemplate } from './platform'

export const syncGitTemplates = () =>
  platformFetch<{ synced: number }>('/api/v1/templates/sync-git', { method: 'POST', body: '{}' })

/** CI/Git webhook endpoint — pass token when MACHINA_TEMPLATES_SYNC_TOKEN is configured on the controller. */
export const syncGitTemplatesWebhook = (token?: string) =>
  platformFetch<{ synced: number; source?: string }>('/api/v1/templates/sync-git/webhook', {
    method: 'POST',
    body: '{}',
    headers: token ? { 'X-Machina-Template-Sync-Token': token } : undefined,
  })

export const approvePlatformTemplate = (name: string, version: string, approval_status: string) =>
  platformFetch<PlatformTemplate>(
    `/api/v1/templates/${encodeURIComponent(name)}/${encodeURIComponent(version)}/approval`,
    { method: 'PATCH', body: JSON.stringify({ approval_status }) },
  )

export const publishVmAsTemplate = (
  vmId: string,
  body: {
    template_name: string
    version: string
    category?: string
    workload?: string
    description?: string
    marketplace?: boolean
    project?: string
  },
) =>
  platformFetch<PlatformTemplate>(`/api/v1/vms/${vmId}/publish-template`, {
    method: 'POST',
    body: JSON.stringify(body),
  })
