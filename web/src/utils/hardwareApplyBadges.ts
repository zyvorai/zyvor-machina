// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** Backend hardware apply badge identifiers (snake_case). */
type HardwareApplyBadgeId =
  | 'live'
  | 'restart_required'
  | 'windows_recommended'
  | 'migration_unsafe'
  | 'advanced'
  | 'host_support_required'

export type HardwareApplyBadgeMeta = {
  id: HardwareApplyBadgeId
  label: string
  title: string
  tone: 'ok' | 'warn' | 'info' | 'danger'
}

const BADGE_REGISTRY: Record<HardwareApplyBadgeId, HardwareApplyBadgeMeta> = {
  live: {
    id: 'live',
    label: 'Live',
    title: 'Can be applied while the VM is running',
    tone: 'ok',
  },
  restart_required: {
    id: 'restart_required',
    label: 'Restart required',
    title: 'Shut down the VM to apply this change',
    tone: 'warn',
  },
  windows_recommended: {
    id: 'windows_recommended',
    label: 'Windows recommended',
    title: 'Recommended for Windows 10/11 guests',
    tone: 'info',
  },
  migration_unsafe: {
    id: 'migration_unsafe',
    label: 'Migration unsafe',
    title: 'Blocks or complicates live migration',
    tone: 'danger',
  },
  advanced: {
    id: 'advanced',
    label: 'Advanced',
    title: 'Advanced configuration — review before applying',
    tone: 'info',
  },
  host_support_required: {
    id: 'host_support_required',
    label: 'Host support required',
    title: 'Hypervisor or host firmware must support this feature',
    tone: 'warn',
  },
}

function normalizeHardwareBadge(raw: string): HardwareApplyBadgeId | null {
  const key = raw.trim().toLowerCase().replace(/[\s-]+/g, '_') as HardwareApplyBadgeId
  return key in BADGE_REGISTRY ? key : null
}

export function resolveHardwareBadges(raw: string[] | undefined | null): HardwareApplyBadgeMeta[] {
  if (!raw?.length) return []
  const seen = new Set<HardwareApplyBadgeId>()
  const out: HardwareApplyBadgeMeta[] = []
  for (const item of raw) {
    const id = normalizeHardwareBadge(item)
    if (!id || seen.has(id)) continue
    seen.add(id)
    out.push(BADGE_REGISTRY[id])
  }
  return out
}

export function badgeToneClasses(tone: HardwareApplyBadgeMeta['tone']): string {
  switch (tone) {
    case 'ok':
      return 'border-emerald-500/40 text-emerald-300 bg-emerald-500/10'
    case 'warn':
      return 'border-amber-500/40 text-amber-300 bg-amber-500/10'
    case 'danger':
      return 'border-rose-500/40 text-rose-300 bg-rose-500/10'
    default:
      return 'border-[var(--accent)]/40 text-[var(--link)] bg-[var(--accent-soft)]'
  }
}
