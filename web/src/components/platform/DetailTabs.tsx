// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { ChevronDown } from 'lucide-react'

type DetailTabDef<T extends string> = {
  id: T
  label: string
  group?: string
}

function tabButtonClass(active: boolean): string {
  return `relative px-3 min-h-11 text-[13px] whitespace-nowrap transition-colors after:absolute after:left-3 after:right-3 after:-bottom-px after:h-0.5 after:rounded-full ${
    active
      ? 'text-[var(--text-primary)] font-medium after:bg-[var(--apple-link,#0066cc)]'
      : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)] after:bg-transparent'
  }`
}

export default function DetailTabs<T extends string>({
  primary,
  more = [],
  active,
  onChange,
}: {
  primary: DetailTabDef<T>[]
  more?: DetailTabDef<T>[]
  active: T
  onChange: (tab: T) => void
}) {
  const [moreOpen, setMoreOpen] = useState(false)
  const stickyRef = useRef<HTMLDivElement>(null)
  const moreRef = useRef<HTMLDivElement>(null)
  const moreIds = new Set(more.map((t) => t.id))
  const moreActive = moreIds.has(active)
  const activeMoreLabel = more.find((t) => t.id === active)?.label

  useEffect(() => {
    if (!moreOpen) return
    const onDoc = (e: MouseEvent) => {
      if (moreRef.current && !moreRef.current.contains(e.target as Node)) setMoreOpen(false)
    }
    document.addEventListener('mousedown', onDoc)
    return () => document.removeEventListener('mousedown', onDoc)
  }, [moreOpen])

  let lastGroup = ''

  const selectTab = (tab: T) => {
    onChange(tab)
    requestAnimationFrame(() => {
      stickyRef.current?.scrollIntoView({ block: 'start', behavior: 'auto' })
    })
  }

  const handleKeyDown = (e: KeyboardEvent<HTMLButtonElement>, currentId: T) => {
    const idx = primary.findIndex((t) => t.id === currentId)
    if (idx === -1) return
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
      e.preventDefault()
      const next = primary[(idx + 1) % primary.length]
      if (next) selectTab(next.id)
    } else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
      e.preventDefault()
      const prev = primary[(idx - 1 + primary.length) % primary.length]
      if (prev) selectTab(prev.id)
    } else if (e.key === 'Home') {
      e.preventDefault()
      if (primary[0]) selectTab(primary[0].id)
    } else if (e.key === 'End') {
      e.preventDefault()
      const last = primary[primary.length - 1]
      if (last) selectTab(last.id)
    }
  }

  return (
    <div ref={stickyRef} className="platform-detail-tabs-sticky" id="platform-detail-tabs">
      <div className="flex flex-wrap items-center">
        <div role="tablist" aria-label="Detail sections" className="contents">
          {primary.map((tab) => (
            <button
              key={tab.id}
              type="button"
              role="tab"
              aria-selected={active === tab.id}
              tabIndex={active === tab.id ? 0 : -1}
              onClick={() => selectTab(tab.id)}
              onKeyDown={(e) => handleKeyDown(e, tab.id)}
              className={tabButtonClass(active === tab.id)}
            >
              {tab.label}
            </button>
          ))}
        </div>
        {more.length > 0 && (
        <div className="relative" ref={moreRef}>
          <button
            type="button"
            onClick={() => setMoreOpen((o) => !o)}
            className={`${tabButtonClass(moreActive)} inline-flex items-center gap-1`}
            aria-expanded={moreOpen}
            aria-haspopup="menu"
          >
            {moreActive && activeMoreLabel ? activeMoreLabel : 'More'}
            <ChevronDown className={`w-3.5 h-3.5 transition-transform ${moreOpen ? 'rotate-180' : ''}`} />
          </button>
          {moreOpen && (
            <div
              role="menu"
              className="absolute left-0 top-full z-30 mt-1 min-w-[12rem] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/80 bg-[var(--apple-surface-elevated)] backdrop-blur-md py-1 shadow-xl"
            >
              {more.map((tab) => {
                const showGroup = tab.group && tab.group !== lastGroup
                if (tab.group) lastGroup = tab.group
                return (
                  <div key={tab.id}>
                    {showGroup && (
                      <p className="px-3 pt-2 pb-1 text-[10px] uppercase tracking-wider text-[var(--text-muted)]">{tab.group}</p>
                    )}
                    <button
                      type="button"
                      role="menuitem"
                      onClick={() => {
                        selectTab(tab.id)
                        setMoreOpen(false)
                      }}
                      className={`w-full text-left px-3 py-2 text-sm ${
                        active === tab.id ? 'bg-[var(--accent-soft)] text-[var(--accent)]' : 'text-[var(--text-secondary)] hover:bg-[var(--apple-surface)]'
                      }`}
                    >
                      {tab.label}
                    </button>
                  </div>
                )
              })}
            </div>
          )}
        </div>
        )}
      </div>
    </div>
  )
}
