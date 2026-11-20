// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ToastAction } from '../hooks/useToast'
import type { PlatformDesktopTier } from './platformDesktopTier'
import { tasksHubHref } from './platformHubLinks'

function taskToastAction(taskId: string, tier: PlatformDesktopTier): ToastAction {
  return {
    label: 'View task',
    href: `${tasksHubHref(tier)}?task=${encodeURIComponent(taskId)}`,
  }
}

export function toastQueuedOperation(
  toast: {
    success: (message: string, duration?: number, action?: ToastAction) => string
  },
  label: string,
  taskId: string,
  tier: PlatformDesktopTier,
) {
  toast.success(label, 10_000, taskToastAction(taskId, tier))
}
