// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { GitBranch, Plus, Trash2 } from 'lucide-react'
import ConfirmDialog from '../../components/ConfirmDialog'
import OperatingSurfaceLayout from '../../components/platform/OperatingSurfaceLayout'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import PlatformPageChrome, { PlatformBackLink } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import {
  advanceMigrationWave,
  cancelMigrationWave,
  createMigrationWave,
  deleteMigrationWave,
  getMigrationWave,
  listMigrationWaves,
  pauseMigrationWave,
  setWaveCutover,
  startMigrationWave,
  type MigrationWave,
  type MigrationWaveDetail,
} from '../../api/platformFleetFeatures'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

const EXAMPLE = JSON.stringify(
  [{ name: 'web-01', config: { command: 'vsphere', vm: '/dc1/vm/web-01', output_dir: '/var/lib/libvirt/images' } }],
  null,
  2,
)

/** Parse the items box: a JSON array of {name, config}. Returns an error string instead of throwing. */
export function parseWaveItems(text: string): { items: { name: string; config: Record<string, unknown> }[] } | { error: string } {
  let parsed: unknown
  try {
    parsed = JSON.parse(text)
  } catch {
    return { error: 'Items must be valid JSON.' }
  }
  if (!Array.isArray(parsed) || parsed.length === 0) return { error: 'Items must be a non-empty JSON array.' }
  const items: { name: string; config: Record<string, unknown> }[] = []
  for (const it of parsed as unknown[]) {
    const o = it as { name?: unknown; config?: unknown }
    if (typeof o?.name !== 'string' || !o.name.trim()) return { error: 'Every item needs a name.' }
    if (typeof o.config !== 'object' || o.config === null || Array.isArray(o.config)) {
      return { error: `Item "${o.name}" needs a config object.` }
    }
    items.push({ name: o.name.trim(), config: o.config as Record<string, unknown> })
  }
  return { items }
}

export default function PlatformMigrationWaves() {
  const toast = useToastContext()
  const [waves, setWaves] = useState<MigrationWave[]>([])
  const [selected, setSelected] = useState<MigrationWaveDetail | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [name, setName] = useState('')
  const [plannedAt, setPlannedAt] = useState('')
  const [itemsText, setItemsText] = useState(EXAMPLE)
  const [parallel, setParallel] = useState(3)
  const [confirmDelete, setConfirmDelete] = useState(false)

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      setWaves(await listMigrationWaves())
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  // A started wave advances on the controller; refresh the open one while it is moving.
  useEffect(() => {
    if (!selected || !(selected.auto_advance || selected.status === 'running')) return undefined
    const t = setInterval(() => {
      void getMigrationWave(selected.id).then(setSelected).catch(() => undefined)
    }, 10000)
    return () => clearInterval(t)
  }, [selected])

  const open = async (id: string) => {
    try { setSelected(await getMigrationWave(id)) } catch (e: unknown) { toast.error(formatUserError(e)) }
  }

  const act = async (label: string, fn: () => Promise<unknown>) => {
    try {
      const res = await fn()
      toast.success(label)
      if (res && typeof res === 'object' && 'items' in res) setSelected(res as MigrationWaveDetail)
      setWaves(await listMigrationWaves())
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const create = () => {
    const parsed = parseWaveItems(itemsText)
    if ('error' in parsed) {
      toast.error(parsed.error)
      return
    }
    void act('Wave planned', () =>
      createMigrationWave({ name: name.trim(), planned_at: plannedAt.trim(), items: parsed.items }))
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/migration" label="Migration" />}
      title="Migration waves"
      subtitle="Plan a batch of VMs for Transiva, start them a few at a time, and track a cutover checklist."
      icon={<GitBranch className="w-6 h-6 text-[var(--text-muted)]" />}
      loading={loading && waves.length === 0}
    >
      <OperatingSurfaceLayout testId="platform-migration-waves-page">
        <MacGlassPanel title="Plan a wave" subtitle="Each item is a VM name and the Transiva migration config for it. Configs are stored encrypted and never shown again.">
          <div className="grid gap-3 max-w-3xl">
            <input className="input text-sm" aria-label="Wave name" placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} />
            <input className="input text-sm" aria-label="Planned for" placeholder="Planned for (free text, optional)" value={plannedAt} onChange={(e) => setPlannedAt(e.target.value)} />
            <textarea className="input text-xs font-mono" aria-label="Wave items" rows={8} value={itemsText} onChange={(e) => setItemsText(e.target.value)} />
          </div>
          <button type="button" className="btn-primary text-sm mt-3 flex items-center gap-1.5" disabled={!name.trim()} onClick={create}>
            <Plus className="w-4 h-4" /> Plan wave
          </button>
        </MacGlassPanel>

        <MacGlassPanel title="Waves" subtitle={`${waves.length} wave(s)`}>
          {waves.length === 0 && !loading ? (
            <PlatformEmptyState icon={GitBranch} title="No waves yet" subtitle="Plan one above, then start it." />
          ) : (
            <ul className="divide-y divide-white/[0.04] -mx-1">
              {waves.map((w) => (
                <MacListRow key={w.id} title={w.name} subtitle={`${w.status}${w.auto_advance ? ' · advancing' : ''}${w.planned_at ? ` · ${w.planned_at}` : ''}`} onClick={() => void open(w.id)} />
              ))}
            </ul>
          )}
        </MacGlassPanel>

        {selected && (
          <MacGlassPanel
            title={selected.name}
            subtitle={`${selected.status}${selected.auto_advance ? ` · advancing, ${selected.max_parallel} at a time` : ''}`}
            action={
              <div className="flex gap-2 items-center">
                <input className="input text-xs w-16" type="number" min={1} max={20} aria-label="At a time" value={parallel} onChange={(e) => setParallel(Number(e.target.value))} />
                <button type="button" className="btn-primary text-xs" onClick={() => void act('Wave started', () => startMigrationWave(selected.id, parallel))}>Start</button>
                <button type="button" className="btn-secondary text-xs" onClick={() => void act('Wave paused', () => pauseMigrationWave(selected.id))}>Pause</button>
                <button type="button" className="btn-secondary text-xs" onClick={() => void act('Advanced one pass', () => advanceMigrationWave(selected.id))}>Advance once</button>
                <button type="button" className="btn-secondary text-xs" onClick={() => void act('Wave cancelled', () => cancelMigrationWave(selected.id))}>Cancel</button>
                <button type="button" className="btn-secondary text-xs p-1.5" aria-label="Delete wave" onClick={() => setConfirmDelete(true)}>
                  <Trash2 className="w-3.5 h-3.5" />
                </button>
              </div>
            }
          >
            <ul className="divide-y divide-white/[0.04] -mx-1">
              {selected.items.map((it) => (
                <MacListRow
                  key={it.id}
                  title={it.name}
                  subtitle={`${it.status}${it.phase ? ` · ${it.phase}` : ''}${it.status === 'running' ? ` · ${it.progress_percent}%` : ''}${it.message ? ` · ${it.message}` : ''}${it.cutover !== 'none' ? ` · cutover: ${it.cutover}` : ''}`}
                  badge={
                    it.status === 'completed' ? (
                      <select
                        className="input text-xs"
                        aria-label={`Cutover for ${it.name}`}
                        value={it.cutover}
                        onChange={(e) => void act('Cutover updated', () => setWaveCutover(selected.id, it.id, e.target.value, it.cutover_note))}
                      >
                        <option value="none">cutover: none</option>
                        <option value="ready">cutover: ready</option>
                        <option value="cut_over">cutover: done</option>
                      </select>
                    ) : undefined
                  }
                />
              ))}
            </ul>
            <p className="text-xs text-[var(--text-muted)] mt-2">The cutover choice is a record only: Machina does not stop the source VM or change DNS.</p>
          </MacGlassPanel>
        )}
      </OperatingSurfaceLayout>
      <ConfirmDialog
        open={confirmDelete}
        title="Delete wave"
        message={`Delete wave "${selected?.name}"? Cancel it first if migrations are running.`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setConfirmDelete(false)}
        onConfirm={async () => {
          if (selected) {
            await act('Wave deleted', () => deleteMigrationWave(selected.id))
            setSelected(null)
          }
          setConfirmDelete(false)
        }}
      />
    </PlatformPageChrome>
  )
}
