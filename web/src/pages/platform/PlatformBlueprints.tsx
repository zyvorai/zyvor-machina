// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import ConfirmDialog from '../../components/ConfirmDialog'
import { Link } from 'react-router'
import { Play, Plus, Trash2, Workflow } from 'lucide-react'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import { AppleStoryHeader } from '../../components/platform/apple/AppleStoryKit'
import DetailTabs from '../../components/platform/DetailTabs'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import { usePlatformTabState } from '../../hooks/usePlatformTabState'
import {
  createBlueprint,
  deleteBlueprint,
  getFleetShortcuts,
  listBlueprints,
  listPlatformVms,
  runBlueprint,
  type Blueprint,
  type FleetShortcutsOverview,
} from '../../api/platform'
import { aiGenerateBlueprint } from '../../api/ai'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses } from '../../utils/semanticColors'

type TabId = 'launchpad' | 'studio'

const BLUEPRINT_TABS = [
  { id: 'launchpad' as const, label: 'Launchpad' },
  { id: 'studio' as const, label: 'Studio' },
]

export default function PlatformBlueprints() {
  const toast = useToastContext()
  const [tab, setTab] = usePlatformTabState<TabId>(BLUEPRINT_TABS.map((t) => t.id), { defaultTab: 'launchpad' })

  const [fleet, setFleet] = useState<FleetShortcutsOverview | null>(null)
  const [rows, setRows] = useState<Blueprint[]>([])
  const [error, setError] = useState<string | null>(null)
  const [running, setRunning] = useState<string | null>(null)
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null)
  const [name, setName] = useState('Nightly backup')
  const [actions, setActions] = useState('backup')
  const [nlPrompt, setNlPrompt] = useState('Nightly backup for all production VMs')
  const [preview, setPreview] = useState<{ name: string; description: string; actions: string[] } | null>(null)
  const [generating, setGenerating] = useState(false)

  const load = useCallback(async () => {
    setError(null)
    try {
      const [f, bps] = await Promise.all([getFleetShortcuts(), listBlueprints()])
      setFleet(f)
      setRows(bps)
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const createFromVms = async () => {
    try {
      const vms = await listPlatformVms()
      const actionList = actions.split(',').map((a) => a.trim()).filter(Boolean)
      await createBlueprint({
        name,
        description: 'Automation blueprint',
        actions: actionList,
        vm_ids: vms.slice(0, 5).map((v) => v.id),
      })
      toast.success('Shortcut created')
      await load()
      setTab('launchpad')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const runShortcut = async (id: string) => {
    setRunning(id)
    try {
      const r = await runBlueprint(id)
      toast.success(`Queued ${r.task_ids.length} task(s)`)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setRunning(null)
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/operations" label="Operations" />}
      title="Blueprint Studio"
      subtitle={
        fleet
          ? `${fleet.summary} · ${fleet.blueprint_count} shortcuts · ${fleet.total_vms_covered} VMs covered · ${fleet.executions_24h} runbooks (24h)`
          : 'macOS Shortcuts-style automation — run blueprints across VM sets.'
      }
      icon={<Workflow className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={
        <div className="flex items-center gap-2">
          <button type="button" className="btn-primary text-sm flex items-center gap-1.5" onClick={() => setTab('studio')}>
            <Plus className="w-4 h-4" /> New blueprint
          </button>
          <PlatformRefreshButton onClick={() => void load()} />
        </div>
      }
      contentClassName="space-y-4"
    >
      <DetailTabs primary={BLUEPRINT_TABS} active={tab} onChange={setTab} />

      {tab === 'launchpad' && (
        <>
          <AppleStoryHeader
            as="h2"
            eyebrow="Launchpad"
            title="Shortcuts"
            lede="Run automation across VM sets — each shortcut queues tasks per machine."
            cta={
              <button type="button" className="btn-primary text-sm flex items-center gap-1.5" onClick={() => setTab('studio')}>
                <Plus className="w-4 h-4" /> New shortcut
              </button>
            }
          />
          {!fleet ? (
            <p className="text-sm text-[var(--text-muted)] py-4">Loading shortcuts…</p>
          ) : (
            <ul className="divide-y divide-[var(--apple-hairline)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] overflow-hidden">
              {(fleet.shortcuts.length ? fleet.shortcuts : rows.map((bp) => ({
                id: bp.id,
                name: bp.name,
                description: bp.description,
                actions: bp.actions ?? [],
                vm_count: bp.vm_ids?.length ?? 0,
                action_count: (bp.actions ?? []).length,
              }))).map((bp) => (
                <li key={bp.id} className="flex items-center gap-3 px-4 py-3.5 hover:bg-[var(--apple-fill-tertiary)]/60 transition">
                  <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-[var(--apple-fill-tertiary)] text-[var(--text-secondary)]">
                    <Workflow className="w-5 h-5" />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block text-sm font-medium text-[var(--text-primary)]">{bp.name}</span>
                    <span className="block text-xs text-[var(--text-muted)] mt-0.5">
                      {(bp.actions ?? []).join(', ') || bp.description || 'No description'} · {bp.vm_count} VM{bp.vm_count === 1 ? '' : 's'}
                    </span>
                  </span>
                  <button
                    type="button"
                    className="btn-primary text-xs flex items-center gap-1 shrink-0"
                    disabled={running === bp.id}
                    onClick={() => void runShortcut(bp.id)}
                  >
                    <Play className={`w-3.5 h-3.5 ${running === bp.id ? 'animate-pulse' : ''}`} />
                    {running === bp.id ? 'Running…' : 'Run'}
                  </button>
                </li>
              ))}
            </ul>
          )}
          <div className="flex flex-wrap gap-3 mt-4">
            <Link to="/platform/reports?tab=runbooks" className={`text-sm ${hubLinkClasses()}`}>Operations runbooks →</Link>
          </div>
        </>
      )}

      {tab === 'studio' && (
        <>
          <MacGlassPanel title="Generate from description">
            <textarea className="input min-h-20" aria-label="Describe your blueprint" value={nlPrompt} onChange={(e) => setNlPrompt(e.target.value)} placeholder="Backup all Windows VMs every night" />
            <div className="flex gap-2 flex-wrap">
              <button type="button" className="btn-primary text-sm" disabled={generating} onClick={async () => {
                setGenerating(true)
                try {
                  const r = await aiGenerateBlueprint(nlPrompt)
                  setPreview({ name: r.name, description: r.description, actions: r.actions })
                  setName(r.name)
                  setActions(r.actions.join(','))
                } catch (e: unknown) { toast.error(formatUserError(e)) }
                finally { setGenerating(false) }
              }}>{generating ? 'Generating…' : 'Preview blueprint'}</button>
              {preview && (
                <button type="button" className="btn-secondary text-sm" onClick={() => void createFromVms()}>Save to Launchpad</button>
              )}
            </div>
            {preview && (
              <p className="text-xs text-[var(--text-muted)]">{preview.description} · actions: {preview.actions.join(', ')}</p>
            )}
          </MacGlassPanel>
          <MacGlassPanel title="Manual shortcut">
            <label className="text-sm">Name<input className="input block mt-1" value={name} onChange={(e) => setName(e.target.value)} /></label>
            <label className="text-sm">Actions (comma-separated)<input className="input block mt-1" value={actions} onChange={(e) => setActions(e.target.value)} placeholder="start,stop,backup" /></label>
            <button type="button" className="btn-primary text-sm flex items-center gap-2" onClick={() => void createFromVms()}><Plus className="w-4 h-4" /> Create from VMs</button>
          </MacGlassPanel>
          <div className="space-y-4">
            {rows.map((bp) => (
              <MacGlassPanel key={bp.id} title={bp.name}>
                <p className="text-xs text-[var(--text-muted)]">{bp.description || 'No description'}</p>
                <p className="text-xs text-[var(--text-muted)]">{(bp.actions ?? []).join(', ')} · {bp.vm_ids?.length ?? 0} VMs</p>
                <div className="flex gap-2">
                  <button type="button" className="btn-primary text-xs flex items-center gap-1" onClick={() => void runShortcut(bp.id)}>
                    <Play className="w-3 h-3" /> Run
                  </button>
                  <button type="button" className="btn-destructive text-xs flex items-center gap-1" onClick={() => setConfirmDeleteId(bp.id)}><Trash2 className="w-3 h-3" /> Delete</button>
                </div>
              </MacGlassPanel>
            ))}
          </div>
          {rows.length === 0 && !error && <p className="text-[var(--text-muted)] text-sm">Create a shortcut in Studio to populate the Launchpad.</p>}
        </>
      )}
      <ConfirmDialog
        open={confirmDeleteId !== null}
        title="Delete Blueprint"
        message={`Delete "${rows.find((b) => b.id === confirmDeleteId)?.name}"? This cannot be undone.`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setConfirmDeleteId(null)}
        onConfirm={async () => {
          try { await deleteBlueprint(confirmDeleteId!); toast.success('Deleted'); await load() }
          catch (e: unknown) { toast.error(formatUserError(e)) }
          finally { setConfirmDeleteId(null) }
        }}
      />
    </PlatformPageChrome>
  )
}
