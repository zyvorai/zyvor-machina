// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'

export type VmBuilderResult = {
  summary: string
  vm_name: string
  vm_spec: Record<string, unknown>
  vcpus: number
  memory_gib: number
  estimated_monthly_usd: number
  network: string
  os_hint: string
}

export const buildVmFromPrompt = (body: { prompt: string; name?: string }) =>
  platformFetch<VmBuilderResult>('/api/v1/ai/vm-builder', {
    method: 'POST',
    body: JSON.stringify(body),
  })
