// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * The "Show N more / Show fewer" button for a `useExpandable` list. A bare button (no wrapping
 * element), so the caller places it wherever is valid for the surrounding markup — a `<tr>`/`<td>`
 * for a table, an `<li>` for a list, or directly for a div-based grid.
 */
export function ExpandableToggle({
  expanded,
  hidden,
  listId,
  onToggle,
  noun = 'items',
  className = 'btn-secondary text-sm',
}: {
  expanded: boolean
  hidden: number
  listId: string
  onToggle: () => void
  noun?: string
  className?: string
}) {
  return (
    <button type="button" className={className} aria-expanded={expanded} aria-controls={listId} onClick={onToggle}>
      {expanded ? `Show fewer ${noun}` : `Show ${hidden} more ${noun}`}
    </button>
  )
}
