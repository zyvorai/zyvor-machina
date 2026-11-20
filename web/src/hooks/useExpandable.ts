// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useId, useState } from 'react'
import { DEFAULT_TOP_N, visibleItems } from '../utils/topN'

/**
 * State + ranking for a long list that shows its top rows and expands on request.
 * Use directly (with `ExpandableToggle`) when the items must stay as direct children of a
 * specific parent — `<tbody>`/`<tr>`, `<ul>`/`<li>` — where `ExpandableList`'s own `<div>`
 * wrapper would be invalid HTML. `ExpandableList` uses this hook internally.
 */
export function useExpandable<T>(items: readonly T[], limit: number = DEFAULT_TOP_N, compare?: (a: T, b: T) => number) {
  const [expanded, setExpanded] = useState(false)
  const listId = useId()
  const { shown, hidden } = visibleItems(items, expanded, limit, compare)
  return { shown, hidden, expanded, toggle: () => setExpanded((v) => !v), listId, showToggle: items.length > limit }
}
