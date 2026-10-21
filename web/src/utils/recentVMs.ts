// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

const KEY = 'machina_recent_vms'

export function addRecentVM(name: string) {
  const list = getRecentVMs().filter(n => n !== name)
  list.unshift(name)
  try { localStorage.setItem(KEY, JSON.stringify(list.slice(0, 5))) } catch { /* storage unavailable or full */ }
}

export function getRecentVMs(): string[] {
  try {
    const parsed = JSON.parse(localStorage.getItem(KEY) || '[]')
    return Array.isArray(parsed) ? parsed.filter((n): n is string => typeof n === 'string') : []
  } catch { return [] }
}

export function removeRecentVMs(names: string[]) {
  if (!names.length) return
  const drop = new Set(names)
  const next = getRecentVMs().filter((n) => !drop.has(n))
  try { localStorage.setItem(KEY, JSON.stringify(next)) } catch { /* storage unavailable or full */ }
}
