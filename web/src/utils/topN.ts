// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** How many rows a long list shows before "Show N more". */
export const DEFAULT_TOP_N = 5

/**
 * The rows to show and how many are hidden behind the toggle. Optionally ranks first
 * (`compare`), because truncating an unranked list hides the important rows arbitrarily.
 * Never mutates its input; the sort is stable.
 */
export function visibleItems<T>(
  items: readonly T[],
  expanded: boolean,
  limit: number = DEFAULT_TOP_N,
  compare?: (a: T, b: T) => number,
): { shown: T[]; hidden: number } {
  const ranked = compare ? items.map((v, i) => ({ v, i })).sort((a, b) => compare(a.v, b.v) || a.i - b.i).map((x) => x.v) : [...items]
  if (expanded || ranked.length <= limit) return { shown: ranked, hidden: 0 }
  return { shown: ranked.slice(0, limit), hidden: ranked.length - limit }
}

const SEVERITY_RANK: Record<string, number> = { critical: 4, high: 3, error: 3, medium: 2, warning: 2, warn: 2, low: 1, info: 0 }

/** Compare two rows by severity, most severe first; unknown severities sort last. */
export function bySeverityDesc<T extends { severity?: string | null }>(a: T, b: T): number {
  return (SEVERITY_RANK[(b.severity ?? '').toLowerCase()] ?? -1) - (SEVERITY_RANK[(a.severity ?? '').toLowerCase()] ?? -1)
}
