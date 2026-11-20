// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

const KEY = 'machina_pinned_vms'
// Unbounded before this fix — a user (or a "pin all" bulk action) could pin
// every VM in a large fleet, growing this localStorage entry without limit.
const MAX_PINNED_VMS = 100

export function getPinnedVMs(): string[] {
  try {
    const parsed = JSON.parse(localStorage.getItem(KEY) || '[]')
    return Array.isArray(parsed) ? parsed.filter((n): n is string => typeof n === 'string') : []
  } catch { return [] }
}

export function isPinned(name: string): boolean {
  return getPinnedVMs().includes(name)
}

export function togglePin(name: string) {
  const list = getPinnedVMs()
  const idx = list.indexOf(name)
  if (idx >= 0) {
    list.splice(idx, 1)
  } else {
    list.push(name)
    if (list.length > MAX_PINNED_VMS) list.splice(0, list.length - MAX_PINNED_VMS)
  }
  localStorage.setItem(KEY, JSON.stringify(list))
}

export function removePinnedVMs(names: string[]) {
  if (!names.length) return
  const drop = new Set(names)
  const next = getPinnedVMs().filter((n) => !drop.has(n))
  localStorage.setItem(KEY, JSON.stringify(next))
}
