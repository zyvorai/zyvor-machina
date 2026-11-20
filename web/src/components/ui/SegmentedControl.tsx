// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useRef, type KeyboardEvent, type ReactNode } from 'react'

type SegmentedOption<V extends string> = {
  value: V
  label: string
  icon?: ReactNode
}

type Props<V extends string> = {
  value: V
  onChange: (value: V) => void
  options: readonly SegmentedOption<V>[]
  /** Names the group for assistive tech, e.g. "View". */
  ariaLabel: string
  className?: string
}

/**
 * A compact one-of-N switch (view mode, range, scope). Radiogroup semantics with roving
 * tabindex: Tab lands on the selected option and the arrow keys move the selection, as native
 * radios do. For wizard-style "pick a path" choices use ChoiceCard instead; those are tiles.
 */
export function SegmentedControl<V extends string>({ value, onChange, options, ariaLabel, className = '' }: Props<V>) {
  const refs = useRef<(HTMLButtonElement | null)[]>([])

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>, index: number) => {
    let next = -1
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') next = (index + 1) % options.length
    else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') next = (index - 1 + options.length) % options.length
    else if (e.key === 'Home') next = 0
    else if (e.key === 'End') next = options.length - 1
    if (next < 0) return
    e.preventDefault()
    onChange(options[next].value)
    refs.current[next]?.focus()
  }

  return (
    <div role="radiogroup" aria-label={ariaLabel} className={`nl-segmented ${className}`.trim()}>
      {options.map((opt, i) => {
        const selected = opt.value === value
        return (
          <button
            key={opt.value}
            ref={(el) => {
              refs.current[i] = el
            }}
            type="button"
            role="radio"
            aria-checked={selected}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(opt.value)}
            onKeyDown={(e) => onKeyDown(e, i)}
          >
            {opt.icon}
            <span>{opt.label}</span>
          </button>
        )
      })}
    </div>
  )
}
