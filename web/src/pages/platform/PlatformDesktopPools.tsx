// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { MonitorCog, Plus, Trash2 } from 'lucide-react'
import ConfirmDialog from '../../components/ConfirmDialog'
import OperatingSurfaceLayout from '../../components/platform/OperatingSurfaceLayout'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import PlatformPageChrome, { PlatformBackLink } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import { listPlatformVms, type PlatformVm } from '../../api/platform'
import {
  checkoutDesktop,
  createDesktopPool,
  deleteDesktopPool,
  getDesktopPool,
  listDesktopPools,
  reconcileDesktopPool,
  releaseDesktop,
  scaleDesktopPool,
  type DesktopPool,
  type DesktopPoolDetail,
} from '../../api/platformFleetFeatures'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

export default function PlatformDesktopPools() {
  const toast = useToastContext()
  const [pools, setPools] = useState<DesktopPool[]>([])
  const [vms, setVms] = useState<PlatformVm[]>([])
  const [selected, setSelected] = useState<DesktopPoolDetail | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [name, setName] = useState('')
  const [template, setTemplate] = useState('')
  const [size, setSize] = useState(5)
  const [persistent, setPersistent] = useState(false)
  const [newSize, setNewSize] = useState(5)
  const [confirmDelete, setConfirmDelete] = useState(false)

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      const [p, v] = await Promise.all([listDesktopPools(), listPlatformVms()])
      setPools(p)
      setVms(v)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  // Clones finish in the background and the controller reconciles every 30 s: keep the view fresh.
  useEffect(() => {
    if (!selected) return undefined
    const t = setInterval(() => {
      void getDesktopPool(selected.id).then(setSelected).catch(() => undefined)
    }, 10000)
    return () => clearInterval(t)
  }, [selected])

  const open = async (id: string) => {
    try {
      const d = await getDesktopPool(id)
      setSelected(d)
      setNewSize(d.size)
    } catch (e: unknown) { toast.error(formatUserError(e)) }
  }

  const act = async (label: string, fn: () => Promise<unknown>) => {
    try {
      const res = await fn()
      toast.success(label)
      if (res && typeof res === 'object' && 'members' in res) setSelected(res as DesktopPoolDetail)
      setPools(await listDesktopPools())
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const stopped = vms.filter((v) => v.observed_state !== 'running')

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/operations" label="Operations" />}
      title="Desktop pools"
      subtitle="A template VM cloned into numbered desktops, one per user. Non-persistent pools re-clone a desktop when it is released."
      icon={<MonitorCog className="w-6 h-6 text-[var(--text-muted)]" />}
      loading={loading && pools.length === 0}
    >
      <OperatingSurfaceLayout testId="platform-desktop-pools-page">
        <MacGlassPanel title="New pool">
          <div className="grid gap-3 md:grid-cols-2 max-w-3xl">
            <input className="input text-sm" aria-label="Pool name" placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} />
            <select className="input text-sm" aria-label="Template VM" value={template} onChange={(e) => setTemplate(e.target.value)}>
              <option value="">Template VM (must be stopped)…</option>
              {stopped.map((v) => <option key={v.id} value={v.id}>{v.name}</option>)}
            </select>
            <input className="input text-sm" type="number" min={1} max={100} aria-label="Pool size" value={size} onChange={(e) => setSize(Number(e.target.value))} />
            <label className="flex items-center gap-2 text-sm text-[var(--text-secondary)]">
              <input type="checkbox" checked={persistent} onChange={(e) => setPersistent(e.target.checked)} /> Persistent (keep the user's disk)
            </label>
          </div>
          <button
            type="button"
            className="btn-primary text-sm mt-3 flex items-center gap-1.5"
            disabled={!name.trim() || !template}
            onClick={() => void act('Pool created', () => createDesktopPool({ name: name.trim(), template_vm_id: template, size, persistent }))}
          >
            <Plus className="w-4 h-4" /> Create pool
          </button>
        </MacGlassPanel>

        <MacGlassPanel title="Pools" subtitle={`${pools.length} pool(s)`}>
          {pools.length === 0 && !loading ? (
            <PlatformEmptyState icon={MonitorCog} title="No pools yet" subtitle="Build a golden VM, shut it down, then make a pool from it." />
          ) : (
            <ul className="divide-y divide-white/[0.04] -mx-1">
              {pools.map((p) => (
                <MacListRow key={p.id} title={p.name} subtitle={`${p.size} desktops · ${p.persistent ? 'persistent' : 'non-persistent'} · ${p.clone_mode} clones`} onClick={() => void open(p.id)} />
              ))}
            </ul>
          )}
        </MacGlassPanel>

        {selected && (
          <MacGlassPanel
            title={selected.name}
            subtitle={`${selected.members.filter((m) => m.assigned_user).length}/${selected.members.length} in use`}
            action={
              <div className="flex gap-2">
                <button type="button" className="btn-secondary text-xs" onClick={() => void act('Checked out', () => checkoutDesktop(selected.id))}>Check out for me</button>
                <button type="button" className="btn-secondary text-xs" onClick={() => void act('Released', () => releaseDesktop(selected.id))}>Release mine</button>
                <button type="button" className="btn-secondary text-xs" onClick={() => void act('Reconciled', () => reconcileDesktopPool(selected.id))}>Reconcile</button>
                <button type="button" className="btn-secondary text-xs p-1.5" aria-label="Delete pool" onClick={() => setConfirmDelete(true)}>
                  <Trash2 className="w-3.5 h-3.5" />
                </button>
              </div>
            }
          >
            <ul className="divide-y divide-white/[0.04] -mx-1">
              {selected.members.map((m) => (
                <MacListRow
                  key={m.id}
                  title={m.vm_name}
                  subtitle={`${m.state}${m.assigned_user ? ` · ${m.assigned_user}` : ''}${m.vm_state ? ` · VM ${m.vm_state}` : ''}${m.message ? ` · ${m.message}` : ''}`}
                />
              ))}
            </ul>
            <div className="flex gap-3 mt-3 items-center">
              <input className="input text-sm w-24" type="number" min={1} max={100} aria-label="New pool size" value={newSize} onChange={(e) => setNewSize(Number(e.target.value))} />
              <button type="button" className="btn-secondary text-sm" onClick={() => void act('Pool resized', () => scaleDesktopPool(selected.id, newSize))}>Resize</button>
            </div>
          </MacGlassPanel>
        )}
      </OperatingSurfaceLayout>
      <ConfirmDialog
        open={confirmDelete}
        title="Delete pool"
        message={`Delete pool "${selected?.name}" and its desktop VMs? Users lose their desktops.`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setConfirmDelete(false)}
        onConfirm={async () => {
          if (selected) {
            await act('Pool deleted', () => deleteDesktopPool(selected.id, true))
            setSelected(null)
          }
          setConfirmDelete(false)
        }}
      />
    </PlatformPageChrome>
  )
}
