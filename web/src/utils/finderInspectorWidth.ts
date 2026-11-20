// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

const KEY = 'machina-finder-inspector-width'
export const FINDER_INSPECTOR_MIN = 220
export const FINDER_INSPECTOR_MAX = 420
const DEFAULT = 280

export function loadFinderInspectorWidth(): number {
  try {
    const n = parseInt(localStorage.getItem(KEY) ?? '', 10)
    if (Number.isFinite(n)) return Math.min(FINDER_INSPECTOR_MAX, Math.max(FINDER_INSPECTOR_MIN, n))
  } catch { /* ignore */ }
  return DEFAULT
}

export function saveFinderInspectorWidth(width: number) {
  try { localStorage.setItem(KEY, String(width)) } catch { /* storage unavailable or full */ }
}
