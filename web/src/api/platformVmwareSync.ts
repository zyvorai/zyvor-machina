// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'

export const syncVmwareInventory = () =>
  platformFetch<{
    synced: boolean
    imported: number
    message: string
    scope: string
    migration_advisor: string
  }>('/api/v1/vmware/sync', { method: 'POST', body: '{}' })
