// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import UnderlineTabs from './UnderlineTabs'

/** Two-level tabs: a category row above a scrollable underline row of that category's tabs. */
export default function GroupedTabs<T extends string>({
  groups,
  value,
  onChange,
  label = 'Sections',
}: {
  groups: Array<{ name: string; tabs: readonly T[] }>
  value: T
  onChange: (id: T) => void
  label?: string
}) {
  const active = groups.find((g) => g.tabs.includes(value)) ?? groups[0]
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap gap-1.5" role="group" aria-label={`${label} categories`}>
        {groups.map((g) => {
          const on = g === active
          return (
            <button
              key={g.name}
              type="button"
              aria-pressed={on}
              onClick={() => onChange(g.tabs[0])}
              className={`rounded-full px-3.5 py-1.5 text-[13px] font-medium transition ${
                on
                  ? 'bg-[color-mix(in_srgb,var(--apple-link,#0071e3)_14%,transparent)] text-[var(--apple-link,#0071e3)]'
                  : 'bg-[var(--apple-fill-tertiary)]/60 text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
              }`}
            >
              {g.name}
            </button>
          )
        })}
      </div>
      <UnderlineTabs tabs={active.tabs.map((t) => ({ id: t, label: t }))} value={value} onChange={onChange} label={label} />
    </div>
  )
}
