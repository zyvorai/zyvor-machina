// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useRef } from 'react'
import { X, Keyboard, Info, Layers } from 'lucide-react'
import { useFocusTrap } from '../hooks/useFocusTrap'
import { helpShortcuts } from './helpShortcuts'
import ZyvorAbout from './ZyvorAbout'
import PlatformAboutHelp from './platform/PlatformAboutHelp'
import { MacSegmentedControl } from './platform/mac/PlatformMacUi'

export type HelpTab = 'shortcuts' | 'about' | 'platform'

type HelpDialogProps = {
  open: boolean
  tab: HelpTab
  onClose: () => void
  onTabChange: (tab: HelpTab) => void
}

const TABS: { id: HelpTab; label: string; icon: React.ReactNode }[] = [
  { id: 'shortcuts', label: 'Shortcuts', icon: <Keyboard className="w-4 h-4" aria-hidden /> },
  { id: 'platform', label: 'Platform', icon: <Layers className="w-4 h-4" aria-hidden /> },
  { id: 'about', label: 'About', icon: <Info className="w-4 h-4" aria-hidden /> },
]

function Kbd({ children }: { children: string }) {
  return (
    <kbd className="px-1.5 py-0.5 bg-[var(--surface-hover)] border border-[var(--apple-hairline)] rounded text-xs font-mono text-[var(--text-secondary)] min-w-[1.5rem] text-center">
      {children}
    </kbd>
  )
}

export default function HelpDialog({ open, tab, onClose, onTabChange }: HelpDialogProps) {
  const dialogRef = useRef<HTMLDivElement>(null)
  useFocusTrap(dialogRef, open)

  if (!open) return null

  return (
    <div
      className="fixed inset-0 z-[60] liquid-glass-modal-backdrop animate-fade-in flex items-start justify-center pt-[8vh] px-4"
      onClick={onClose}
      onKeyDown={(e) => {
        if (e.key === 'Escape') onClose()
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label="Help"
        className="liquid-glass-modal-panel w-full max-w-lg overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-5 py-4 border-b border-white/[0.06]">
          <h2 className="text-lg font-semibold text-[var(--text-primary)]">Help</h2>
          <button
            type="button"
            onClick={onClose}
            className="p-1 hover:bg-white/5 rounded-lg transition text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
            aria-label="Close help"
          >
            <X className="w-4 h-4" strokeWidth={1.75} aria-hidden="true" />
          </button>
        </div>

        <div className="px-2 pt-1 pb-2" aria-label="Help sections">
          <MacSegmentedControl
            options={TABS.map((t) => ({ value: t.id, label: t.label, icon: t.icon }))}
            value={tab}
            onChange={onTabChange}
          />
        </div>

        <div className="max-h-[min(70vh,32rem)] overflow-y-auto p-5">
          {tab === 'shortcuts' ? (
            <div role="tabpanel">
              <div className="space-y-3">
                {helpShortcuts.map((s) => (
                  <div key={s.description} className="flex items-center justify-between gap-3">
                    <span className="text-sm text-[var(--text-secondary)]">{s.description}</span>
                    <div className="flex items-center gap-1 shrink-0">
                      {s.keys.map((k, i) => (
                        <span key={`${s.description}-${k}-${i}`} className="flex items-center gap-1">
                          {i > 0 && <span className="text-[var(--text-faint)] text-xs">+</span>}
                          <Kbd>{k}</Kbd>
                        </span>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
              <p className="text-xs text-[var(--text-muted)] mt-4 pt-3 border-t border-[var(--apple-hairline)]">
                Shortcuts are disabled when typing in input fields. Open{' '}
                <strong className="text-[var(--text-muted)]">Help → Platform</strong> for Zyvor Platform guidance.
              </p>
            </div>
          ) : tab === 'platform' ? (
            <div role="tabpanel">
              <PlatformAboutHelp />
            </div>
          ) : (
            <div role="tabpanel">
              <ZyvorAbout />
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
