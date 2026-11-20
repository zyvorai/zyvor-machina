// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { vmDelete } from '../api/platform'
import { purgeVmShortcuts } from './vmShortcuts'
import { toastQueuedOperation } from './platformTaskToast'
import type { PlatformDesktopTier } from './platformDesktopTier'

type VmDeleteTarget = {
  id: string
  name: string
  inventory_source?: string | null
}

type ToastApi = {
  success: (message: string, duration?: number) => string
}

/** Queue or complete a platform VM delete and surface the right toast. */
export async function queuePlatformVmDelete(
  vm: VmDeleteTarget,
  toast: ToastApi,
  tier: PlatformDesktopTier,
) {
  if (vm.inventory_source === 'kubevirt') {
    throw new Error('KubeVirt guests must be deleted from the cluster')
  }
  const r = await vmDelete(vm.id, true)
  purgeVmShortcuts([vm.name])
  if (r.status === 'completed' || !r.task_id) {
    toast.success(`Removed ${vm.name}`)
  } else {
    toastQueuedOperation(toast, `Deleting ${vm.name}`, r.task_id, tier)
  }
  return r
}
