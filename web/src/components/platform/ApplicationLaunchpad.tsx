// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import {
  Archive,
  Boxes,
  Database,
  Globe,
  Landmark,
  Loader2,
  Monitor,
  Play,
  Square,
  Workflow,
} from 'lucide-react'
import ErrorBanner from '../../components/ErrorBanner'
import PlatformEmptyState from './PlatformEmptyState'
import {
  LaunchpadAppIcon,
  MacSheet,
  NewLaunchpadCard,
  PresetTemplateCard,
  gradientForName,
} from './mac/PlatformMacUi'
import {
  createApplication,
  getApplication,
  listApplications,
  listPlatformVms,
  runApplicationAction,
  type ApplicationGroup,
  type PlatformVm,
} from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses } from '../../utils/semanticColors'

const PRESETS = [
  { name: 'Finance Application', description: 'ERP, databases & reporting', icon: <Landmark className="w-5 h-5" /> },
  { name: 'Web Stack', description: 'Load balancer, app & cache VMs', icon: <Globe className="w-5 h-5" /> },
  { name: 'Database Cluster', description: 'Primary, replica & backup', icon: <Database className="w-5 h-5" /> },
] as const

function uniqueAppName(base: string, existing: string[]): string {
  const trimmed = base.trim() || 'Application Group'
  if (!existing.includes(trimmed)) return trimmed
  let i = 2
  while (existing.includes(`${trimmed} ${i}`)) i += 1
  return `${trimmed} ${i}`
}

export default function ApplicationLaunchpad() {
  const toast = useToastContext()
  const [rows, setRows] = useState<ApplicationGroup[]>([])
  const [details, setDetails] = useState<Record<string, string[]>>({})
  const [vms, setVms] = useState<PlatformVm[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState<string | null>(null)
  const [sheetOpen, setSheetOpen] = useState(false)
  const [activeApp, setActiveApp] = useState<ApplicationGroup | null>(null)
  const [newName, setNewName] = useState('Finance Application')
  const [selectedVmIds, setSelectedVmIds] = useState<Set<string>>(new Set())
  const [creating, setCreating] = useState(false)

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      const [apps, vmList] = await Promise.all([listApplications(), listPlatformVms()])
      setVms(vmList)
      setRows(apps)
      const det: Record<string, string[]> = {}
      await Promise.all(apps.map(async (a) => {
        try {
          const d = await getApplication(a.id)
          det[a.id] = d.vm_names
        } catch { det[a.id] = [] }
      }))
      setDetails(det)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const openCreate = (presetName?: string) => {
    const name = presetName ?? 'Finance Application'
    setNewName(name)
    setSelectedVmIds(new Set(vms.slice(0, 3).map((v) => v.id)))
    setActiveApp(null)
    setSheetOpen(true)
  }

  const toggleVm = (id: string) => {
    setSelectedVmIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  const createGroup = async () => {
    const trimmed = newName.trim()
    if (!trimmed) {
      toast.error('Enter an application name')
      return
    }
    setCreating(true)
    try {
      const name = uniqueAppName(trimmed, rows.map((r) => r.name))
      const app = await createApplication({
        name,
        description: 'Application group',
        vm_ids: [...selectedVmIds],
      })
      if (name !== trimmed) toast.success(`Created "${name}" — previous name was taken`)
      else toast.success('Application group created')
      setDetails((d) => ({ ...d, [app.id]: app.vm_names }))
      setSheetOpen(false)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setCreating(false)
    }
  }

  const runAction = async (appId: string, action: 'start' | 'stop' | 'backup') => {
    setBusy(appId + action)
    try {
      const r = await runApplicationAction(appId, action)
      toast.success(`${action} queued for ${r.task_ids.length} VM(s)`)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(null)
    }
  }

  const activeVmNames = useMemo(
    () => (activeApp ? details[activeApp.id] ?? [] : []),
    [activeApp, details],
  )

  if (loading && rows.length === 0 && !error) {
    return <p className="text-sm text-[var(--text-muted)] py-8 text-center" aria-busy="true">Loading applications…</p>
  }

  return (
    <div className="space-y-8 animate-fade-in">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex gap-2 text-xs ml-auto">
          <Link to="/platform/blueprints" className="btn-secondary text-xs flex items-center gap-1.5">
            <Workflow className="w-3.5 h-3.5" /> Blueprints
          </Link>
          <Link to="/platform/topology" className="btn-secondary text-xs flex items-center gap-1.5">
            <Boxes className="w-3.5 h-3.5" /> Topology
          </Link>
        </div>
      </div>

      {error && <ErrorBanner message={error} />}

      <section>
        <p className="text-xs font-medium text-[var(--text-muted)] mb-4 uppercase tracking-wider">Your applications</p>
        <div className="platform-launchpad-grid grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-7 gap-x-4 gap-y-8">
          <NewLaunchpadCard onClick={() => openCreate()} />
          {PRESETS.filter((p) => !rows.some((r) => r.name === p.name)).map((p) => (
            <LaunchpadAppIcon
              key={p.name}
              name={p.name}
              icon={p.icon}
              gradient={gradientForName(p.name)}
              onClick={() => openCreate(p.name)}
            />
          ))}
          {rows.map((app) => (
            <LaunchpadAppIcon
              key={app.id}
              name={app.name}
              icon={<Boxes className="w-7 h-7 sm:w-8 sm:h-8" />}
              gradient={gradientForName(app.name)}
              vmCount={(details[app.id] ?? []).length}
              onClick={() => { setActiveApp(app); setSheetOpen(true) }}
            />
          ))}
        </div>
        {rows.length === 0 && !error && (
          <PlatformEmptyState
            icon={Boxes}
            title="No application groups yet"
            subtitle="Bundle VMs into a Launchpad group to start, stop, and backup entire stacks together."
            action={
              <button type="button" className="btn-primary text-sm" onClick={() => openCreate()}>
                New application
              </button>
            }
          />
        )}
      </section>

      <MacSheet
        open={sheetOpen && !activeApp}
        onClose={() => setSheetOpen(false)}
        title="New Application"
        subtitle="Pick a template, name your group, and choose VMs — like creating a macOS app folder."
        wide
      >
        <div className="space-y-5">
          <div>
            <p className="text-xs font-medium text-[var(--text-muted)] mb-2">Quick templates</p>
            <div className="grid gap-2 sm:grid-cols-3">
              {PRESETS.map((p) => (
                <PresetTemplateCard
                  key={p.name}
                  name={p.name}
                  description={p.description}
                  icon={p.icon}
                  gradient={gradientForName(p.name)}
                  onClick={() => setNewName(p.name)}
                />
              ))}
            </div>
          </div>
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">Application name</span>
            <input
              className="input w-full mt-1.5"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              placeholder="Finance Application"
            />
            <span className="text-xs text-[var(--text-muted)] mt-1 block">Spaces are fine. Duplicates get a numeric suffix automatically.</span>
          </label>
          <div>
            <p className="text-sm text-[var(--text-muted)] mb-2">Include VMs ({selectedVmIds.size} selected)</p>
            {vms.length === 0 ? (
              <p className="text-sm text-[var(--text-muted)] rounded-xl border border-dashed border-[var(--apple-hairline)] p-4">
                No VMs yet. <Link to="/platform/vms" className={`hover:underline ${hubLinkClasses()}`}>Create a VM</Link> first, then return here.
              </p>
            ) : (
              <ul className="space-y-1.5 max-h-48 overflow-y-auto rounded-xl border border-white/[0.06] bg-[var(--apple-surface)] p-2">
                {vms.map((vm) => (
                  <li key={vm.id}>
                    <label className="flex items-center gap-3 p-2 rounded-lg hover:bg-white/[0.03] cursor-pointer">
                      <input
                        type="checkbox"
                        checked={selectedVmIds.has(vm.id)}
                        onChange={() => toggleVm(vm.id)}
                        className="rounded border-[var(--apple-hairline)]"
                      />
                      <Monitor className="w-4 h-4 text-[var(--text-muted)] shrink-0" />
                      <span className="text-sm text-[var(--text-primary)] flex-1 truncate">{vm.name}</span>
                      <span className="text-xs text-[var(--text-muted)] capitalize">{vm.observed_state}</span>
                    </label>
                  </li>
                ))}
              </ul>
            )}
          </div>
          <div className="flex gap-2 pt-2">
            <button type="button" className="btn-secondary text-sm flex-1" onClick={() => setSheetOpen(false)}>Cancel</button>
            <button type="button" className="btn-primary text-sm flex-1 flex items-center justify-center gap-2" disabled={creating} onClick={() => void createGroup()}>
              {creating ? <Loader2 className="w-4 h-4 animate-spin" /> : null}
              Create Application
            </button>
          </div>
        </div>
      </MacSheet>

      <MacSheet
        open={sheetOpen && !!activeApp}
        onClose={() => { setSheetOpen(false); setActiveApp(null) }}
        title={activeApp?.name ?? 'Application'}
        subtitle={activeApp?.description || 'Application group'}
      >
        {activeApp && (
          <div className="space-y-4">
            <div className={`mx-auto w-20 h-20 rounded-[22%] bg-gradient-to-br ${gradientForName(activeApp.name)} flex items-center justify-center text-white shadow-lg`}>
              <Boxes className="w-9 h-9" />
            </div>
            <div>
              <p className="text-xs text-[var(--text-muted)] mb-2">Virtual machines</p>
              {activeVmNames.length === 0 ? (
                <p className="text-sm text-[var(--text-muted)]">No VMs linked to this group yet.</p>
              ) : (
                <ul className="space-y-1 text-sm text-[var(--text-secondary)]">
                  {activeVmNames.map((v) => (
                    <li key={v} className="flex items-center gap-2">
                      <Monitor className="w-3.5 h-3.5 text-[var(--text-muted)]" /> {v}
                    </li>
                  ))}
                </ul>
              )}
            </div>
            <div className="grid grid-cols-2 gap-2 pt-2">
              <button type="button" className="btn-primary text-sm flex items-center justify-center gap-1.5" disabled={busy !== null} onClick={() => void runAction(activeApp.id, 'start')}>
                <Play className="w-3.5 h-3.5" /> Start all
              </button>
              <button type="button" className="btn-secondary text-sm flex items-center justify-center gap-1.5" disabled={busy !== null} onClick={() => void runAction(activeApp.id, 'stop')}>
                <Square className="w-3.5 h-3.5" /> Stop all
              </button>
              <button type="button" className="btn-secondary text-sm flex items-center justify-center gap-1.5 col-span-2" disabled={busy !== null} onClick={() => void runAction(activeApp.id, 'backup')}>
                <Archive className="w-3.5 h-3.5" /> Backup all VMs
              </button>
            </div>
            <Link to="/platform/topology" className="btn-secondary w-full text-center text-sm block" onClick={() => { setSheetOpen(false); setActiveApp(null) }}>
              View in topology
            </Link>
          </div>
        )}
      </MacSheet>
    </div>
  )
}
