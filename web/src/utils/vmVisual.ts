// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** VM state visuals (tone + badge classes). */

type VmSemanticKind =
  | 'running'
  | 'stopped'
  | 'paused'
  | 'creating'
  | 'failed'
  | 'migrating'
  | 'suspended'
  | 'unknown'

/** Normalize libvirt + platform API state strings to a visual kind. */
export function vmSemanticKind(state: string | undefined | null): VmSemanticKind {
  const s = (state ?? '').toLowerCase().trim().replace(/_/g, ' ')
  if (!s) return 'unknown'
  if (s === 'running' || s === 'active' || s === 'poweredon' || s === 'powered on') return 'running'
  if (s === 'paused') return 'paused'
  if (
    s === 'creating' ||
    s === 'building' ||
    s === 'provisioning' ||
    s === 'pending' ||
    s === 'shutting down' ||
    s === 'missing'
  ) {
    return 'creating'
  }
  if (s === 'failed' || s === 'crashed' || s === 'error') return 'failed'
  if (s.includes('migrat')) return 'migrating'
  if (s === 'suspended' || s === 'blocked') return 'suspended'
  if (
    s === 'shutoff' ||
    s === 'stopped' ||
    s === 'shut off' ||
    s === 'poweredoff' ||
    s === 'powered off' ||
    s === 'inactive'
  ) {
    return 'stopped'
  }
  return 'unknown'
}

/** iPhone 17–aligned: Sage / Mist Blue / Cosmic Orange — no purple/indigo neon. */
const VM_LAUNCHPAD_GRADIENTS: Record<VmSemanticKind, string> = {
  running: 'from-emerald-600 to-emerald-800',
  stopped: 'from-stone-400 to-stone-600',
  paused: 'from-amber-500 to-orange-700',
  creating: 'from-[hsl(200_85%_52%)] to-[hsl(200_70%_25%)]',
  failed: 'from-red-500 to-red-800',
  migrating: 'from-[hsl(200_85%_52%)] to-[hsl(220_70%_30%)]',
  suspended: 'from-[hsl(200_70%_40%)] to-slate-700',
  unknown: 'from-stone-400 to-stone-700',
}

export function vmLaunchpadGradient(state: string | undefined | null): string {
  return VM_LAUNCHPAD_GRADIENTS[vmSemanticKind(state)]
}

export function vmStatusBadgeClasses(
  state: string | undefined | null,
  variant: 'soft' | 'solid' = 'soft',
): string {
  const kind = vmSemanticKind(state)
  const base = `machina-vm-status machina-vm-status--${kind}`
  return variant === 'solid' ? `${base} machina-vm-status--solid` : base
}

/** Safe GiB label for VM memory — avoids NaN when API omits memory_mib. */
export function formatVmMemoryGiB(mib: number | undefined | null): string {
  const n = Number(mib)
  if (!Number.isFinite(n) || n <= 0) return '—'
  return `${Math.round(n / 1024)} GiB`
}
