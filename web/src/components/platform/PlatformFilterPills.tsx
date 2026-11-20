// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { navActiveChipClasses } from '../../utils/semanticColors'

interface FilterPill {
  id: string
  label: string
  count?: number
}

export default function PlatformFilterPills({
  options,
  value,
  onChange,
}: {
  options: FilterPill[]
  value: string
  onChange: (id: string) => void
}) {
  return (
    <div className="flex flex-wrap gap-2">
      {options.map((o) => (
        <button
          key={o.id}
          type="button"
          onClick={() => onChange(o.id)}
          className={`px-3 py-1.5 rounded-full text-xs font-medium transition ${
            value === o.id
              ? `${navActiveChipClasses()} px-3 py-1.5 rounded-full text-xs font-medium border`
              : 'bg-[var(--apple-surface)] text-[var(--text-muted)] border border-white/[0.06] hover:border-white/10'
          }`}
        >
          {o.label}
          {o.count != null && <span className="ml-1 opacity-60">{o.count}</span>}
        </button>
      ))}
    </div>
  )
}
