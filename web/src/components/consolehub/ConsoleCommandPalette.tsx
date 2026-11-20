// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Search } from 'lucide-react'
import { useKeyboardShortcut } from '../../hooks/useKeyboardShortcut'
import { isInputFocused } from '../../hooks/useKeyboardShortcut'
import type { ViewportMode } from './ConsoleViewportContext'

type ConsolePaletteAction = {
  id: string
  label: string
  keywords?: string
  run: () => void
}

type Props = {
  open: boolean
  onClose: () => void
  actions: ConsolePaletteAction[]
}

export default function ConsoleCommandPalette({ open, onClose, actions }: Props) {
  const [query, setQuery] = useState('')
  const [index, setIndex] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return actions
    return actions.filter(
      (a) =>
        a.label.toLowerCase().includes(q)
        || a.keywords?.toLowerCase().includes(q),
    )
  }, [actions, query])

  useEffect(() => {
    if (open) {
      setQuery('')
      setIndex(0)
      setTimeout(() => inputRef.current?.focus(), 50)
    }
  }, [open])

  useEffect(() => {
    setIndex(0)
  }, [query])

  const run = useCallback(
    (action: ConsolePaletteAction) => {
      action.run()
      onClose()
    },
    [onClose],
  )

  if (!open) return null

  return (
    <>
      <button type="button" className="fixed inset-0 z-[90] bg-black/50 backdrop-blur-sm" aria-label="Close" onClick={onClose} />
      <div
        className="fixed top-[15%] left-1/2 -translate-x-1/2 z-[95] w-[min(92vw,32rem)] rounded-xl border border-white/10 bg-slate-950/95 shadow-2xl overflow-hidden"
        role="dialog"
        aria-modal="true"
        aria-label="Console command palette"
        data-testid="console-command-palette"
      >
        <div className="flex items-center gap-2 px-3 py-2 border-b border-white/10">
          <Search className="w-4 h-4 text-[var(--text-muted)]" />
          <input
            ref={inputRef}
            aria-label="Search commands"
            className="flex-1 bg-transparent text-sm text-[var(--text-primary)] outline-none placeholder:text-[var(--text-muted)]"
            placeholder="What do you want to do?"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') onClose()
              if (e.key === 'ArrowDown') {
                e.preventDefault()
                setIndex((i) => Math.min(i + 1, filtered.length - 1))
              }
              if (e.key === 'ArrowUp') {
                e.preventDefault()
                setIndex((i) => Math.max(i - 1, 0))
              }
              if (e.key === 'Enter' && filtered[index]) run(filtered[index])
            }}
          />
        </div>
        <ul className="max-h-64 overflow-y-auto py-1">
          {filtered.length === 0 ? (
            <li className="px-3 py-2 text-xs text-[var(--text-muted)]">No matching actions</li>
          ) : (
            filtered.map((a, i) => (
              <li key={a.id}>
                <button
                  type="button"
                  className={`w-full text-left px-3 py-2 text-sm ${i === index ? 'bg-emerald-900/30 text-emerald-100' : 'text-[var(--text-primary)] hover:bg-white/5'}`}
                  onMouseEnter={() => setIndex(i)}
                  onClick={() => run(a)}
                >
                  {a.label}
                </button>
              </li>
            ))
          )}
        </ul>
      </div>
    </>
  )
}

// Tracks whether a console command palette is currently claiming ⌘/Ctrl+K on
// this page, so the global CommandPalette can defer instead of both opening at
// once (useKeyboardShortcut treats Ctrl and Meta interchangeably).
let consolePaletteShortcutOwners = 0
export function consolePaletteShortcutActive(): boolean {
  return consolePaletteShortcutOwners > 0
}

export function useConsoleCommandPaletteShortcut(onOpen: () => void, enabled: boolean) {
  useEffect(() => {
    if (!enabled) return
    consolePaletteShortcutOwners += 1
    return () => {
      consolePaletteShortcutOwners -= 1
    }
  }, [enabled])
  useKeyboardShortcut({
    key: 'k',
    meta: true,
    handler: (e) => {
      if (!enabled || isInputFocused()) return
      e.preventDefault()
      onOpen()
    },
  })
}

export function buildDefaultConsoleActions(opts: {
  onSwitchLens: (lens: string) => void
  onCtrlAltDel: () => void
  onSetMode: (mode: ViewportMode) => void
  onExposeSsh?: () => void
  onSnapshot?: () => void
  onOpenAi?: () => void
  onOpenOpsShelf?: () => void
  onOpenStudio?: () => void
  vmDetailHref?: string
}): ConsolePaletteAction[] {
  const out: ConsolePaletteAction[] = [
    { id: 'shell', label: 'Open shell', keywords: 'ssh terminal', run: () => opts.onSwitchLens('shell') },
    { id: 'serial', label: 'Open serial', keywords: 'console login', run: () => opts.onSwitchLens('serial') },
    { id: 'cad', label: 'Send Ctrl+Alt+Delete', keywords: 'ctrl alt del', run: opts.onCtrlAltDel },
    { id: 'fit', label: 'Resize display — Fit', run: () => opts.onSetMode('fit') },
    { id: 'fill', label: 'Resize display — Fill', run: () => opts.onSetMode('fill') },
    { id: 'native', label: 'Resize display — Native 1:1', run: () => opts.onSetMode('native') },
  ]
  if (opts.onExposeSsh) {
    out.push({ id: 'expose-ssh', label: 'Expose SSH on hypervisor', run: opts.onExposeSsh })
  }
  if (opts.onSnapshot) {
    out.push({ id: 'snapshot', label: 'Create snapshot', run: opts.onSnapshot })
  }
  if (opts.onOpenAi) {
    out.push({ id: 'ai', label: 'Ask Machina AI', keywords: 'diagnose black display', run: opts.onOpenAi })
  }
  if (opts.onOpenOpsShelf) {
    out.push({ id: 'ops', label: 'Open Ops Shelf', run: opts.onOpenOpsShelf })
  }
  if (opts.onOpenStudio) {
    out.push({ id: 'studio', label: 'Switch to Machina Studio', run: opts.onOpenStudio })
  }
  if (opts.vmDetailHref) {
    out.push({
      id: 'mount-iso',
      label: 'Mount ISO (VM settings)',
      keywords: 'cdrom iso',
      run: () => { window.location.href = `${opts.vmDetailHref}?tab=disks` },
    })
  }
  return out
}
