// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Numeric 0-100 health score from a VM health report. The controller returns `score` as a label
 * ("healthy" | "warning" | …) and the number in `score_numeric`; older payloads put a number in `score`.
 */
export function vmHealthScore(h: { score?: string | number | null; score_numeric?: number | string | null } | null | undefined): number | null {
  if (!h) return null
  for (const v of [h.score_numeric, h.score]) {
    if (v == null || v === '') continue
    const n = typeof v === 'number' ? v : Number.parseFloat(v)
    if (Number.isFinite(n)) return Math.max(0, Math.min(100, Math.round(n)))
  }
  return null
}
