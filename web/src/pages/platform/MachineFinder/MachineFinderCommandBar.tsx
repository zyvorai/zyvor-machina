// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Link } from 'react-router'
import { Plus, RefreshCw, Search, Sparkles } from 'lucide-react'
import { dispatchOpenSpotlight } from '../../../utils/platformJarvisShell'
import type { MachineFinderState } from './useMachineFinder'

type Props = {
  state: MachineFinderState
}

export default function MachineFinderCommandBar({ state }: Props) {
  const {
    search,
    setSearch,
    statsSubtitle,
    load,
    fleetGuestBusy,
    runFleetGuestQuery,
    setWizardOpen,
    setWizardInitial,
    setWindowsOpen,
  } = state

  return (
    <header className="machine-finder-command-bar apple-page-header border-b border-[var(--apple-hairline)] pb-8" data-testid="machine-finder-command-bar">
      <div className="min-w-0 flex-1 max-w-3xl">
        <p className="apple-eyebrow">VM Center</p>
        <h1 className="page-title">Machine Finder</h1>
        <p className="page-lede">{statsSubtitle}</p>
      </div>

      <div className="apple-page-actions flex-1 sm:justify-end min-w-0">
        <div className="relative flex-1 min-w-[14rem] max-w-md">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-[var(--text-faint)] pointer-events-none" />
          <input
            id="machine-finder-search"
            name="machine_search"
            type="search"
            aria-label="Search machines"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
                dispatchOpenSpotlight(search.trim() || undefined)
              }
            }}
            placeholder="Search machines… (⌘K)"
            className="input-field w-full pl-9 pr-3 py-2.5 text-sm"
            style={{ paddingLeft: '2.25rem' }}
            data-testid="machine-finder-search"
          />
        </div>
        <button type="button" className="btn-secondary text-sm inline-flex items-center gap-1" disabled={fleetGuestBusy} onClick={() => void runFleetGuestQuery()}>
          {fleetGuestBusy ? <RefreshCw className="w-4 h-4 animate-spin" /> : <Sparkles className="w-4 h-4" />}
          Analyze
        </button>
        <button type="button" className="btn-secondary text-sm" onClick={() => state.setLens('migration')}>
          Migrate
        </button>
        <Link to="/platform/vm-builder" className="btn-secondary text-sm hidden lg:inline-flex">AI Builder</Link>
        <button type="button" className="btn-secondary text-sm hidden sm:inline-flex" onClick={() => setWindowsOpen(true)}>Windows</button>
        <button type="button" className="btn-secondary text-xs p-2.5" onClick={() => void load()} aria-label="Refresh"><RefreshCw className="w-4 h-4" /></button>
        <button
          type="button"
          className="btn-primary text-sm inline-flex items-center gap-1"
          data-testid="machine-finder-new-vm"
          onClick={() => { setWizardInitial(undefined); setWizardOpen(true) }}
        >
          <Plus className="w-4 h-4" /> New VM
        </button>
      </div>
    </header>
  )
}
