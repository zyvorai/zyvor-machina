// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Controller inventory sync (distinct from daemon `kubevirt.ts` export bundles).

import { platformFetch } from './platform'

export const syncKubevirtInventory = () =>
  platformFetch<{ synced: boolean; cluster_id?: string; message: string }>('/api/v1/kubevirt/sync', {
    method: 'POST',
    body: '{}',
  })
