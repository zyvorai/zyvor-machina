// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'

export const syncProxmoxInventory = () =>
  platformFetch<{ synced: boolean; imported: number; message: string; scope: string }>(
    '/api/v1/proxmox/sync',
    { method: 'POST', body: '{}' },
  )
