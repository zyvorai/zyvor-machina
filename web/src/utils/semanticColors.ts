// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** Semantic status → Tailwind utility classes (Machina color system v1). */

export function statusToneClass(tone: 'ok' | 'warn' | 'error' | 'info' | 'neutral'): string {
  switch (tone) {
    case 'ok':
      return 'text-[var(--machina-status-ok)]'
    case 'warn':
      return 'text-[var(--machina-status-warn)]'
    case 'error':
      return 'text-[var(--machina-status-error)]'
    case 'info':
      return 'text-[var(--machina-status-info)]'
    default:
      return 'text-[var(--machina-status-neutral)]'
  }
}

export function statusBgClass(tone: 'ok' | 'warn' | 'error' | 'info' | 'neutral'): string {
  switch (tone) {
    case 'ok':
      return 'bg-[var(--machina-status-ok)]'
    case 'warn':
      return 'bg-[var(--machina-status-warn)]'
    case 'error':
      return 'bg-[var(--machina-status-error)]'
    case 'info':
      return 'bg-[var(--machina-status-info)]'
    default:
      return 'bg-[var(--machina-status-neutral)]'
  }
}

export function taskStatusTone(status: string): 'ok' | 'warn' | 'error' | 'info' | 'neutral' {
  if (status === 'completed' || status === 'succeeded') return 'ok'
  if (status === 'failed' || status === 'error') return 'error'
  if (status === 'running' || status === 'pending') return 'info'
  return 'neutral'
}

export function hostStateTone(state: string, fenced?: boolean, maintenance?: boolean): 'ok' | 'warn' | 'error' | 'neutral' {
  if (maintenance) return 'warn'
  if (state === 'online' && !fenced) return 'ok'
  if (state === 'offline' || fenced) return 'error'
  return 'neutral'
}

export function statusBadgeClasses(tone: 'ok' | 'warn' | 'error' | 'info' | 'neutral'): string {
  const varName = {
    ok: '--machina-status-ok',
    warn: '--machina-status-warn',
    error: '--machina-status-error',
    info: '--machina-status-info',
    neutral: '--machina-status-neutral',
  }[tone]
  return `bg-[color-mix(in_srgb,var(${varName})_18%,transparent)] text-[var(${varName})]`
}

export function sessionBadgeClasses(extra = ''): string {
  return `px-1.5 py-0.5 rounded text-[10px] font-medium border ${statusBadgeClasses('warn')} border-[color-mix(in_srgb,var(--machina-status-warn)_20%,transparent)] ${extra}`.trim()
}

export function statusActionLinkClasses(tone: 'ok' | 'warn' | 'error' | 'info' | 'neutral', extra = ''): string {
  return `status-link ${statusToneClass(tone)} hover:underline ${extra}`.trim()
}

export function statusDestructiveButtonClasses(extra = ''): string {
  return `inline-flex items-center gap-1 px-3 py-2 rounded-lg border text-sm ${statusBadgeClasses('error')} ${extra}`.trim()
}

export function vmStateTone(state: string): 'ok' | 'warn' | 'error' | 'info' | 'neutral' {
  const s = state.toLowerCase().trim().replace(/_/g, ' ')
  if (s === 'running' || s === 'active' || s === 'poweredon' || s === 'powered on') return 'ok'
  if (s === 'failed' || s === 'crashed' || s === 'error') return 'error'
  if (
    s === 'paused' ||
    s === 'shutting down' ||
    s === 'missing' ||
    s === 'creating' ||
    s === 'building' ||
    s === 'provisioning' ||
    s === 'pending'
  ) {
    return 'warn'
  }
  if (s.includes('migrat') || s === 'suspended' || s === 'blocked') return 'info'
  if (
    s === 'shutoff' ||
    s === 'stopped' ||
    s === 'shut off' ||
    s === 'poweredoff' ||
    s === 'powered off' ||
    s === 'inactive'
  ) {
    return 'neutral'
  }
  return 'neutral'
}

export function instanceStatusTone(status: string): 'ok' | 'warn' | 'error' | 'info' | 'neutral' {
  const s = status.toUpperCase()
  if (s === 'ACTIVE' || s === 'UP') return 'ok'
  if (s === 'ERROR' || s === 'DOWN') return 'error'
  if (s === 'BUILD' || s === 'BUILDING' || s === 'SLEEPING') return 'info'
  return 'warn'
}

export function k8sPhaseTone(phase: string): 'ok' | 'warn' | 'error' | 'info' | 'neutral' {
  const p = phase.toLowerCase()
  if (p === 'running' || p === 'succeeded' || p === 'bound') return 'ok'
  if (p === 'failed' || p === 'error') return 'error'
  if (p === 'pending' || p === 'containercreating') return 'info'
  if (p === 'warning' || p === 'unknown') return 'warn'
  return 'neutral'
}

export function httpStatusTone(code: number): 'ok' | 'warn' | 'error' | 'neutral' {
  if (code >= 500) return 'error'
  if (code >= 400) return 'warn'
  if (code >= 200) return 'ok'
  return 'neutral'
}

export function riskTone(risk: string): 'ok' | 'warn' | 'error' | 'neutral' {
  const r = risk.toLowerCase()
  if (r === 'critical' || r === 'high') return 'error'
  if (r === 'warning' || r === 'warn' || r === 'medium') return 'warn'
  if (r === 'low' || r === 'info') return 'ok'
  return 'neutral'
}

export function migrationReadinessTone(status: string): 'ok' | 'warn' | 'error' {
  if (status === 'ready') return 'ok'
  if (status === 'check') return 'warn'
  return 'error'
}

export function webhookDeliveryTone(status: string): 'ok' | 'warn' | 'error' {
  if (status === 'delivered') return 'ok'
  if (status === 'failed') return 'error'
  return 'warn'
}

/** The one "status pill" shape to use for a standalone badge — bordered
 * full pill, matching utils/vmVisual.ts's .machina-vm-status treatment.
 * statusBadgeClasses() stays a bare bg+text tint (no border/radius/padding)
 * since other helpers below compose their own chrome on top of it. */
export function statusPillClasses(tone: 'ok' | 'warn' | 'error' | 'info' | 'neutral'): string {
  const varName = {
    ok: '--machina-status-ok',
    warn: '--machina-status-warn',
    error: '--machina-status-error',
    info: '--machina-status-info',
    neutral: '--machina-status-neutral',
  }[tone]
  return `px-2 py-1 rounded-full border ${statusBadgeClasses(tone)} border-[color-mix(in_srgb,var(${varName})_40%,transparent)]`
}

export function utilizationTone(percent: number): 'ok' | 'warn' | 'error' {
  if (percent > 90) return 'error'
  if (percent > 70) return 'warn'
  return 'ok'
}

export function utilizationBarClass(
  percent: number,
  thresholds: { warn: number; error: number } = { warn: 70, error: 90 },
): string {
  if (percent > thresholds.error) return statusBgClass('error')
  if (percent > thresholds.warn) return statusBgClass('warn')
  return statusBgClass('ok')
}

export function poolStateBadgeClasses(state: string): string {
  return `text-xs font-medium ${statusPillClasses(state === 'running' ? 'ok' : 'neutral')}`
}

export function userRoleTone(role: string): 'error' | 'info' | 'neutral' {
  if (role === 'admin') return 'error'
  if (role === 'operator') return 'info'
  return 'neutral'
}

export function statusSurfaceClasses(tone: 'ok' | 'warn' | 'error' | 'info' | 'neutral', extra = ''): string {
  const varName = {
    ok: '--machina-status-ok',
    warn: '--machina-status-warn',
    error: '--machina-status-error',
    info: '--machina-status-info',
    neutral: '--machina-status-neutral',
  }[tone]
  return [
    extra,
    `border-[color-mix(in_srgb,var(${varName})_30%,transparent)]`,
    `bg-[color-mix(in_srgb,var(${varName})_10%,transparent)]`,
    `text-[color-mix(in_srgb,var(${varName})_75%,white)]`,
  ].filter(Boolean).join(' ')
}

export function statusChipClasses(tone: 'ok' | 'warn' | 'error' | 'info' | 'neutral', extra = ''): string {
  return statusSurfaceClasses(tone, `rounded-full px-3 py-1 ${extra}`.trim())
}

export function checkStatusTone(status: string): 'ok' | 'warn' | 'error' | 'neutral' {
  if (status === 'pass') return 'ok'
  if (status === 'warn') return 'warn'
  if (status === 'fail') return 'error'
  return 'neutral'
}

export function serviceStateTone(state: string): 'ok' | 'warn' | 'error' | 'neutral' {
  if (state === 'active') return 'ok'
  if (state === 'failed') return 'error'
  if (state === 'activating' || state === 'deactivating') return 'warn'
  return 'neutral'
}

export function prereqTone(ok: boolean | null, missing: 'warn' | 'error' | 'neutral' = 'warn'): 'ok' | 'warn' | 'error' | 'neutral' {
  if (ok === null) return 'neutral'
  if (ok) return 'ok'
  return missing
}

export function jobStatusTone(status: string): 'ok' | 'warn' | 'error' | 'neutral' {
  if (status === 'running') return 'warn'
  if (status === 'completed') return 'ok'
  if (status === 'failed') return 'error'
  return 'neutral'
}

export function journalPriorityTone(priority: string): 'ok' | 'warn' | 'error' | 'info' | 'neutral' {
  if (['emerg', 'alert', 'crit', 'err'].includes(priority)) return 'error'
  if (priority === 'warning') return 'warn'
  if (priority === 'notice') return 'info'
  if (priority === 'debug') return 'neutral'
  return 'neutral'
}

export function connectionStatusTone(status: string): 'ok' | 'warn' | 'error' | 'neutral' {
  if (status === 'connected') return 'ok'
  if (status === 'error') return 'error'
  return 'warn'
}

export function notificationChannelTone(type: string): 'info' | 'neutral' {
  if (type === 'email' || type === 'webhook' || type === 'telegram') return 'info'
  return 'neutral'
}

export function statusBorderClass(tone: 'ok' | 'warn' | 'error' | 'info' | 'neutral'): string {
  const varName = {
    ok: '--machina-status-ok',
    warn: '--machina-status-warn',
    error: '--machina-status-error',
    info: '--machina-status-info',
    neutral: '--machina-status-neutral',
  }[tone]
  return `border-[color-mix(in_srgb,var(${varName})_30%,transparent)]`
}

export function toastSemanticTone(type: 'success' | 'error' | 'warning' | 'info'): 'ok' | 'warn' | 'error' | 'info' {
  if (type === 'success') return 'ok'
  if (type === 'error') return 'error'
  if (type === 'warning') return 'warn'
  return 'info'
}

export function navActiveChipClasses(extra = ''): string {
  return `${statusBadgeClasses('info')} ring-1 ring-[color-mix(in_srgb,var(--machina-status-info)_35%,transparent)] ${extra}`.trim()
}

export function tabActiveClasses(extra = ''): string {
  return `border-[var(--machina-status-info)] ${statusToneClass('info')} ${extra}`.trim()
}

/** Platform hub / cross-link navigation (info tone). */
export function hubLinkClasses(extra = ''): string {
  return statusActionLinkClasses('info', extra)
}
