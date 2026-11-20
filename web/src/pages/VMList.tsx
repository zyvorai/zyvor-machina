// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, useCallback, useRef } from 'react'
import { Link } from 'react-router'
import {
  listVMs, startVM, stopVM, shutdownVM, pauseVM, resumeVM,
  VmInfo, vmDetailRoute, vmConsoleRoute, vmScopeKey,
} from '../api/vm'
import { deleteVmWithNvramRetry } from '../utils/deleteVmWithNvramRetry'
import { purgeVmShortcuts } from '../utils/vmShortcuts'
import { getStateBadgeClasses } from '../utils/vm'
import { useToastContext } from '../contexts/ToastContext'
import { copyText } from '../utils/copyText'
import { useWebSocketContext } from '../contexts/WebSocketContext'
import ConfirmDialog from '../components/ConfirmDialog'
import { getAllTags, getVmTags } from '../api/extras'
import { Play, Square, Power, Pause, RotateCcw, Trash2, RefreshCw, Terminal, Tag, LayoutGrid, LayoutList, X, Download, Star, Server, Copy, Monitor } from 'lucide-react'
import VmResolvedSshConnectDialog from '../components/vm/VmResolvedSshConnectDialog'
import { SegmentedControl } from '../components/ui/SegmentedControl'
import { downloadJSON, downloadCSV } from '../utils/export'
import { isPinned, togglePin } from '../utils/pinnedVMs'
import EmptyState from '../components/EmptyState'
import PageLayout from '../components/PageLayout'
import { TahoeToolbar } from '../components/platform/tahoe/TahoeListKit'
import { formatUserError } from '../utils/apiError'
import { libvirtErrorHints } from '../utils/libvirtHints'
import { sessionBadgeClasses, statusActionLinkClasses, statusBadgeClasses, statusToneClass } from '../utils/semanticColors'
import { usePlatformInfo } from '../contexts/PlatformInfoContext'
import { useExpandable } from '../hooks/useExpandable'
import { ExpandableToggle } from '../components/ui/ExpandableToggle'

export default function VMList() {
  const [vms, setVMs] = useState<VmInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [deleteTarget, setDeleteTarget] = useState<{ name: string; libvirt_connection?: string } | null>(null)
  const [vmTagsMap, setVmTagsMap] = useState<Record<string, string[]>>({})
  const [allTagNames, setAllTagNames] = useState<string[]>([])
  const [tagFilter, setTagFilter] = useState('')
  const [selectedVMs, setSelectedVMs] = useState<Set<string>>(new Set())
  const [batchDeleteConfirm, setBatchDeleteConfirm] = useState(false)
  const [viewMode, setViewMode] = useState<'table' | 'grid'>(() => (localStorage.getItem('vmlist-view') as 'table' | 'grid') || 'table')
  const [pinnedRefresh, setPinnedRefresh] = useState(0)
  const [sshVm, setSshVm] = useState<VmInfo | null>(null)
  const toast = useToastContext()
  const { info } = usePlatformInfo()
  const { subscribe } = useWebSocketContext()
  const lastLoadErrorToastAt = useRef(0)

  const load = useCallback(async () => {
    try {
      setLoadError(null)
      const vmList = await listVMs()
      setVMs(vmList)
      // Load tags for all VMs
      const tagMap: Record<string, string[]> = {}
      await Promise.all(vmList.map(async (vm) => {
        try {
          const t = await getVmTags(vm.name)
          tagMap[vmScopeKey(vm)] = t.tags
        } catch { /* optional */ }
      }))
      setVmTagsMap(tagMap)
      // Load all unique tag names
      try { const counts = await getAllTags(); setAllTagNames(Object.keys(counts).sort()) } catch { /* optional */ }
    } catch (e: unknown) {
      const msg = formatUserError(e)
      setLoadError(msg)
      const now = Date.now()
      if (now - lastLoadErrorToastAt.current > 12_000) {
        lastLoadErrorToastAt.current = now
        toast.error(`Failed to load VMs: ${msg}`)
      }
    } finally {
      setLoading(false)
    }
  }, [toast])

  useEffect(() => { load() }, [load])
  useEffect(() => {
    const unsubscribe = subscribe(() => load())
    return () => unsubscribe()
  }, [subscribe, load])

  const action = async (
    vm: VmInfo,
    fn: (n: string, c?: string | null) => Promise<void>,
    label: string,
  ) => {
    try {
      await fn(vm.name, vm.libvirt_connection)
      toast.success(`${label} '${vm.name}' OK`)
      load()
    } catch (e: unknown) {
      toast.error(`${label} '${vm.name}' failed: ${formatUserError(e)}`)
    }
  }

  const handleDelete = async () => {
    if (!deleteTarget) return
    const t = deleteTarget
    setDeleteTarget(null)
    try {
      await deleteVmWithNvramRetry(t.name, undefined, undefined, t.libvirt_connection)
      toast.success(`Deleted '${t.name}'`)
      load()
    } catch (e: unknown) {
      toast.error(`Delete failed: ${formatUserError(e)}`)
    }
  }

  const filtered = vms.filter((v) => {
    const name = (v.name ?? '').toString()
    const state = (v.state ?? 'unknown').toString()
    const q = search.toLowerCase()
    const matchesSearch =
      name.toLowerCase().includes(q) || state.toLowerCase().includes(q)
    const matchesTag = !tagFilter || (vmTagsMap[vmScopeKey(v)] || []).includes(tagFilter)
    return matchesSearch && matchesTag
  })

  const sorted = [...filtered].sort((a, b) => {
    const ap = isPinned(vmScopeKey(a)) ? 0 : 1
    const bp = isPinned(vmScopeKey(b)) ? 0 : 1
    return ap - bp
  })

  // Select-all/export/etc. above operate on the full filtered/sorted set (search already narrows
  // it); only the table/grid rendering itself is capped, so a large host isn't one giant DOM dump.
  const vmList = useExpandable(sorted, 50)

  const toggleSelect = (key: string) => {
    setSelectedVMs(prev => {
      const next = new Set(prev)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })
  }

  const toggleAll = () => {
    if (selectedVMs.size === filtered.length) setSelectedVMs(new Set())
    else setSelectedVMs(new Set(filtered.map(vmScopeKey)))
  }

  const batchRun = async (fn: (name: string, c?: string | null) => Promise<void>, label: string) => {
    const results = await Promise.allSettled(
      Array.from(selectedVMs).map((key) => {
        const vm = vms.find((v) => vmScopeKey(v) === key)
        if (!vm) return Promise.reject(new Error('VM not found'))
        return fn(vm.name, vm.libvirt_connection)
      }),
    )
    const ok = results.filter(r => r.status === 'fulfilled').length
    const fail = results.filter(r => r.status === 'rejected').length
    if (ok > 0) toast.success(`${label}: ${ok} succeeded`)
    if (fail > 0) toast.error(`${label}: ${fail} failed`)
    setSelectedVMs(new Set())
    load()
  }

  const handleBatchDelete = async () => {
    setBatchDeleteConfirm(false)
    const results = await Promise.allSettled(
      Array.from(selectedVMs).map((key) => {
        const vm = vms.find((v) => vmScopeKey(v) === key)
        if (!vm) return Promise.reject(new Error('VM not found'))
        return deleteVmWithNvramRetry(vm.name, undefined, undefined, vm.libvirt_connection)
      }),
    )
    const ok = results.filter((r) => r.status === 'fulfilled').length
    const fail = results.filter((r) => r.status === 'rejected').length
    if (ok > 0) {
      purgeVmShortcuts(
        Array.from(selectedVMs)
          .filter((_, i) => results[i].status === 'fulfilled')
          .map((key) => vms.find((v) => vmScopeKey(v) === key)?.name)
          .filter((n): n is string => Boolean(n)),
      )
      toast.success(`Delete: ${ok} succeeded`)
    }
    if (fail > 0) toast.error(`Delete: ${fail} failed`)
    setSelectedVMs(new Set())
    load()
  }

  useEffect(() => { setSelectedVMs(new Set()) }, [search, tagFilter])
  useEffect(() => { localStorage.setItem('vmlist-view', viewMode) }, [viewMode])

  return (
    <PageLayout
      loading={loading}
      eyebrow="Hypervisor"
      title="Virtual machines"
      subtitle={
        <>
          QEMU/KVM guests on this hypervisor host (libvirt).
          {(info?.libvirt?.extra_uris?.length ?? 0) > 0 && (
            <> Federated read-only hosts: {info!.libvirt!.extra_uris!.join(', ')}.</>
          )}
          {' '}Use VM details for optional KubeVirt bundle / cluster actions when configured.
        </>
      }
      error={loadError}
      errorTitle="Failed to load virtual machines"
      errorTone="red"
      errorHints={loadError ? libvirtErrorHints(loadError) : undefined}
      technicalDetail={loadError}
      onErrorRetry={() => void load()}
      onErrorDismiss={() => setLoadError(null)}
      actions={
        <>
          <button onClick={() => downloadJSON(filtered, 'vms.json')} className="p-2 hover:bg-[var(--surface-hover)] rounded transition" title="Export JSON" aria-label="Export JSON"><Download className="w-4 h-4" /></button>
          <button onClick={() => downloadCSV(filtered as unknown as Record<string, unknown>[], 'vms.csv')} className="p-2 hover:bg-[var(--surface-hover)] rounded transition" title="Export CSV" aria-label="Export CSV"><Download className={`w-4 h-4 ${statusToneClass('ok')}`} /></button>
          <SegmentedControl
            ariaLabel="View"
            value={viewMode}
            onChange={setViewMode}
            options={[
              { value: 'table', label: 'Table', icon: <LayoutList className="w-4 h-4" /> },
              { value: 'grid', label: 'Grid', icon: <LayoutGrid className="w-4 h-4" /> },
            ]}
          />
          <button onClick={load} className="p-2 hover:bg-[var(--surface-hover)] rounded transition" title="Refresh" aria-label="Refresh VM list">
            <RefreshCw className="w-4 h-4" />
          </button>
          <Link to="/create" className="btn-primary text-sm">+ Create VM</Link>
        </>
      }
    >
      <TahoeToolbar
        search={search}
        onSearchChange={setSearch}
        placeholder="Search VMs…"
        trailing={
          <>
            {allTagNames.length > 0 && (
              <div className="flex items-center gap-1.5 pr-1">
                <Tag className="w-4 h-4 text-[var(--text-muted)]" />
                <select
                  value={tagFilter}
                  onChange={(e) => setTagFilter(e.target.value)}
                  aria-label="Filter by tag"
                  className="bg-transparent border-0 text-sm py-1 px-2 focus:outline-none text-[var(--text-secondary)]"
                >
                  <option value="">All tags</option>
                  {allTagNames.map(t => <option key={t} value={t}>{t}</option>)}
                </select>
              </div>
            )}
            {(search || tagFilter) && (
              <span aria-live="polite" className="text-sm text-[var(--text-muted)] shrink-0 pr-2">
                {filtered.length} VM{filtered.length !== 1 ? 's' : ''}
              </span>
            )}
          </>
        }
      />

      {filtered.length === 0 ? (
        <EmptyState
          icon={<Server className="w-6 h-6" />}
          title={search || tagFilter ? 'No VMs match your filters' : 'No guests on this host'}
          description={
            search || tagFilter
              ? 'Try a different search or clear the tag filter.'
              : 'Create a new VM or import an existing disk image to define a libvirt domain.'
          }
          primaryAction={
            !search && !tagFilter ? (
              <Link to="/create" className="btn-primary text-sm">
                Create VM
              </Link>
            ) : undefined
          }
          secondaryAction={
            !search && !tagFilter ? (
              <Link to="/import" className="px-4 py-2 rounded-lg border border-[var(--apple-hairline)] text-[var(--text-secondary)] hover:bg-[var(--apple-fill-tertiary)] text-sm">
                Import VM
              </Link>
            ) : undefined
          }
        />
      ) : viewMode === 'table' ? (
        <div className="card overflow-x-auto">
          <table className="w-full" aria-label="Virtual machines">
            <thead>
              <tr className="border-b border-[var(--apple-hairline)] text-left text-sm text-[var(--text-muted)]">
                <th scope="col" className="px-3 py-3 w-8">
                  <input type="checkbox" aria-label="Select all VMs" checked={selectedVMs.size === filtered.length && filtered.length > 0} onChange={toggleAll} className="rounded border-[var(--apple-hairline)] bg-[var(--apple-surface)]" />
                </th>
                <th scope="col" className="px-3 sm:px-6 py-3">Name</th>
                <th scope="col" className="px-3 sm:px-6 py-3">State</th>
                <th scope="col" className="px-3 sm:px-6 py-3 hidden md:table-cell">vCPUs</th>
                <th scope="col" className="px-3 sm:px-6 py-3 hidden md:table-cell">Memory</th>
                <th scope="col" className="px-3 sm:px-6 py-3 text-right">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-[var(--apple-hairline)]/50" id={vmList.listId}>
              {vmList.shown.map((vm) => (
                <tr key={vmScopeKey(vm)} className="hover:bg-[var(--surface-hover)]/50 transition">
                  <td className="px-3 py-4">
                    <input type="checkbox" aria-label={`Select ${vm.name}`} checked={selectedVMs.has(vmScopeKey(vm))} onChange={() => toggleSelect(vmScopeKey(vm))} className="rounded border-[var(--apple-hairline)] bg-[var(--apple-surface)]" />
                  </td>
                  <td className="px-3 sm:px-6 py-4">
                    <div className="flex items-center gap-2 flex-wrap">
                      <button onClick={(e) => { e.preventDefault(); togglePin(vmScopeKey(vm)); setPinnedRefresh(n => n + 1) }} aria-label={isPinned(vmScopeKey(vm)) ? 'Unpin' : 'Pin'} className="p-1 hover:bg-yellow-600/20 rounded transition" title={isPinned(vmScopeKey(vm)) ? 'Unpin' : 'Pin'}>
                        <Star className={`w-3.5 h-3.5 ${isPinned(vmScopeKey(vm)) ? `${statusToneClass('warn')} fill-[var(--machina-status-warn)]` : 'text-[var(--text-muted)]'}`} />
                      </button>
                      <Link to={vmDetailRoute(vm.name, vm.libvirt_connection)} className={`font-medium inline-flex items-center min-h-9 ${statusActionLinkClasses('info')}`}>{vm.name}</Link>
                      {vm.libvirt_connection === 'session' && (
                        <span className={sessionBadgeClasses()}>session</span>
                      )}
                      {vm.backend === 'fluxvm' && (
                        <span className={sessionBadgeClasses()} title="Managed by fluxvm-api">FluxVM{vm.fluxvm_backend ? ` · ${vm.fluxvm_backend}` : ''}</span>
                      )}
                      {(vmTagsMap[vmScopeKey(vm)] || []).map(t => (
                        <span key={t} className={`px-1.5 py-0.5 rounded-full text-[10px] font-medium ${statusBadgeClasses('info')}`}>{t}</span>
                      ))}
                    </div>
                  </td>
                  <td className="px-3 sm:px-6 py-4">
                    <span className={getStateBadgeClasses(vm.state)}>{vm.state}</span>
                  </td>
                  <td className="px-3 sm:px-6 py-4 hidden md:table-cell text-[var(--text-secondary)]">{vm.vcpus}</td>
                  <td className="px-3 sm:px-6 py-4 hidden md:table-cell text-[var(--text-secondary)]">{vm.memory_mb} MB</td>
                  <td className="px-3 sm:px-6 py-4">
                    <div className="flex items-center justify-end gap-1">
                      {vm.state === 'running' && (
                        <>
                          <Link to={vmConsoleRoute(vm.name, vm.libvirt_connection)} className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition" title="Console" aria-label="Console">
                            <Monitor className="w-4 h-4 text-[var(--text-secondary)]" />
                          </Link>
                          <button type="button" onClick={() => setSshVm(vm)} className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition" title="SSH" aria-label="SSH">
                            <Terminal className="w-4 h-4 text-[var(--text-secondary)]" />
                          </button>
                          {vm.guest_ip && (
                            <button
                              type="button"
                              onClick={async () => { if (await copyText(vm.guest_ip!)) toast.success('Guest IP copied'); else toast.error('Copy failed — check clipboard permissions') }}
                              className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition"
                              title="Copy guest IP"
                              aria-label="Copy guest IP"
                            >
                              <Copy className="w-4 h-4 text-[var(--text-secondary)]" />
                            </button>
                          )}
                        </>
                      )}
                      {vm.state === 'shutoff' && (
                        <button onClick={() => action(vm, startVM, 'Start')} className={`p-1.5 rounded transition hover:bg-[color-mix(in_srgb,var(--machina-status-ok)_25%,transparent)]`} title="Start" aria-label="Start">
                          <Play className={`w-4 h-4 ${statusToneClass('ok')}`} />
                        </button>
                      )}
                      {vm.state === 'running' && (
                        <>
                          <button onClick={() => action(vm, shutdownVM, 'Shutdown')} className={`p-1.5 rounded transition hover:bg-[color-mix(in_srgb,var(--machina-status-warn)_25%,transparent)]`} title="Shutdown" aria-label="Shutdown">
                            <Power className={`w-4 h-4 ${statusToneClass('warn')}`} />
                          </button>
                          <button onClick={() => action(vm, stopVM, 'Stop')} className={`p-1.5 rounded transition hover:bg-[color-mix(in_srgb,var(--machina-status-error)_25%,transparent)]`} title="Force Stop" aria-label="Force Stop">
                            <Square className={`w-4 h-4 ${statusToneClass('error')}`} />
                          </button>
                          <button onClick={() => action(vm, pauseVM, 'Pause')} className={`p-1.5 rounded transition hover:bg-[color-mix(in_srgb,var(--machina-status-info)_25%,transparent)]`} title="Pause" aria-label="Pause">
                            <Pause className={`w-4 h-4 ${statusToneClass('info')}`} />
                          </button>
                        </>
                      )}
                      {vm.state === 'paused' && (
                        <button onClick={() => action(vm, resumeVM, 'Resume')} className={`p-1.5 rounded transition hover:bg-[color-mix(in_srgb,var(--machina-status-ok)_25%,transparent)]`} title="Resume" aria-label="Resume">
                          <RotateCcw className={`w-4 h-4 ${statusToneClass('ok')}`} />
                        </button>
                      )}
                      <button onClick={() => setDeleteTarget({ name: vm.name, libvirt_connection: vm.libvirt_connection })} className={`p-1.5 rounded transition hover:bg-[color-mix(in_srgb,var(--machina-status-error)_25%,transparent)]`} title="Delete" aria-label="Delete">
                        <Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {vmList.showToggle && (
            <div className="p-3 border-t border-[var(--apple-hairline)]">
              <ExpandableToggle expanded={vmList.expanded} hidden={vmList.hidden} listId={vmList.listId} onToggle={vmList.toggle} noun="VMs" />
            </div>
          )}
        </div>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(20rem,1fr))] gap-4" id={vmList.listId}>
          {vmList.shown.map((vm) => (
            <div key={vmScopeKey(vm)} className="card p-5 hover:border-white/15 transition-all">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2 min-w-0">
                  <input type="checkbox" aria-label={`Select ${vm.name}`} checked={selectedVMs.has(vmScopeKey(vm))} onChange={() => toggleSelect(vmScopeKey(vm))} className="rounded border-[var(--apple-hairline)] bg-[var(--apple-surface)] shrink-0" />
                  <button onClick={(e) => { e.preventDefault(); togglePin(vmScopeKey(vm)); setPinnedRefresh(n => n + 1) }} aria-label={isPinned(vmScopeKey(vm)) ? 'Unpin' : 'Pin'} className="p-1 hover:bg-yellow-600/20 rounded transition" title={isPinned(vmScopeKey(vm)) ? 'Unpin' : 'Pin'}>
                    <Star className={`w-3.5 h-3.5 ${isPinned(vmScopeKey(vm)) ? `${statusToneClass('warn')} fill-[var(--machina-status-warn)]` : 'text-[var(--text-muted)]'}`} />
                  </button>
                  <Link to={vmDetailRoute(vm.name, vm.libvirt_connection)} className={`font-semibold truncate ${statusActionLinkClasses('info')}`}>{vm.name}</Link>
                  {vm.libvirt_connection === 'session' && (
                    <span className={sessionBadgeClasses('shrink-0')}>session</span>
                  )}
                  {vm.backend === 'fluxvm' && (
                    <span className={sessionBadgeClasses('shrink-0')} title="Managed by fluxvm-api">FluxVM{vm.fluxvm_backend ? ` · ${vm.fluxvm_backend}` : ''}</span>
                  )}
                </div>
                <span className={`shrink-0 ${getStateBadgeClasses(vm.state)}`}>{vm.state}</span>
              </div>
              <div className="space-y-1 text-sm text-[var(--text-secondary)] mb-3">
                <div className="flex justify-between"><span className="text-[var(--text-muted)]">vCPUs</span><span>{vm.vcpus}</span></div>
                <div className="flex justify-between"><span className="text-[var(--text-muted)]">Memory</span><span>{vm.memory_mb} MB</span></div>
              </div>
              {(vmTagsMap[vmScopeKey(vm)] || []).length > 0 && (
                <div className="flex flex-wrap gap-1 mb-3">
                  {(vmTagsMap[vmScopeKey(vm)] || []).map(t => (
                    <span key={t} className={`px-1.5 py-0.5 rounded-full text-[10px] font-medium ${statusBadgeClasses('info')}`}>{t}</span>
                  ))}
                </div>
              )}
              <div className="flex items-center gap-1 pt-3 border-t border-[var(--apple-hairline)]">
                {vm.state === 'running' && (
                  <>
                    <Link to={vmConsoleRoute(vm.name, vm.libvirt_connection)} className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition" title="VNC" aria-label="VNC"><Monitor className="w-4 h-4 text-[var(--text-secondary)]" /></Link>
                    <button type="button" onClick={() => setSshVm(vm)} className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition" title="SSH" aria-label="SSH"><Terminal className="w-4 h-4 text-[var(--text-secondary)]" /></button>
                    {vm.guest_ip && (
                      <button type="button" onClick={async () => { if (await copyText(vm.guest_ip!)) toast.success('Guest IP copied'); else toast.error('Copy failed — check clipboard permissions') }} className="p-1.5 hover:bg-[var(--surface-hover)] rounded transition" title="Copy IP" aria-label="Copy IP"><Copy className="w-4 h-4 text-[var(--text-secondary)]" /></button>
                    )}
                    <button onClick={() => action(vm, shutdownVM, 'Shutdown')} className={`p-1.5 rounded transition hover:bg-[color-mix(in_srgb,var(--machina-status-warn)_25%,transparent)]`} title="Shutdown" aria-label="Shutdown"><Power className={`w-4 h-4 ${statusToneClass('warn')}`} /></button>
                    <button onClick={() => action(vm, stopVM, 'Stop')} className={`p-1.5 rounded transition hover:bg-[color-mix(in_srgb,var(--machina-status-error)_25%,transparent)]`} title="Force Stop" aria-label="Force Stop"><Square className={`w-4 h-4 ${statusToneClass('error')}`} /></button>
                    <button onClick={() => action(vm, pauseVM, 'Pause')} className={`p-1.5 rounded transition hover:bg-[color-mix(in_srgb,var(--machina-status-info)_25%,transparent)]`} title="Pause" aria-label="Pause"><Pause className={`w-4 h-4 ${statusToneClass('info')}`} /></button>
                  </>
                )}
                {vm.state === 'shutoff' && (
                  <button onClick={() => action(vm, startVM, 'Start')} className={`p-1.5 rounded transition hover:bg-[color-mix(in_srgb,var(--machina-status-ok)_25%,transparent)]`} title="Start" aria-label="Start"><Play className={`w-4 h-4 ${statusToneClass('ok')}`} /></button>
                )}
                {vm.state === 'paused' && (
                  <button onClick={() => action(vm, resumeVM, 'Resume')} className={`p-1.5 rounded transition hover:bg-[color-mix(in_srgb,var(--machina-status-ok)_25%,transparent)]`} title="Resume" aria-label="Resume"><RotateCcw className={`w-4 h-4 ${statusToneClass('ok')}`} /></button>
                )}
                <div className="flex-1" />
                <button onClick={() => setDeleteTarget({ name: vm.name, libvirt_connection: vm.libvirt_connection })} className={`p-1.5 rounded transition hover:bg-[color-mix(in_srgb,var(--machina-status-error)_25%,transparent)]`} title="Delete" aria-label="Delete"><Trash2 className={`w-4 h-4 ${statusToneClass('error')}`} /></button>
              </div>
            </div>
          ))}
        </div>
      )}
      {viewMode === 'grid' && vmList.showToggle && (
        <div className="mt-4">
          <ExpandableToggle expanded={vmList.expanded} hidden={vmList.hidden} listId={vmList.listId} onToggle={vmList.toggle} noun="VMs" />
        </div>
      )}

      {selectedVMs.size > 0 && (
        <div className="fixed bottom-6 left-1/2 -translate-x-1/2 z-40 bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] rounded-xl shadow-2xl px-4 py-3 flex items-center gap-3 animate-fade-in">
          <span className="text-sm font-medium">{selectedVMs.size} selected</span>
          <div className="w-px h-5 bg-[var(--surface-hover)]" />
          <button onClick={() => batchRun(startVM, 'Start')} className={`px-3 py-1.5 rounded-lg text-xs font-medium transition ${statusBadgeClasses('ok')}`}>Start</button>
          <button onClick={() => batchRun(shutdownVM, 'Shutdown')} className={`px-3 py-1.5 rounded-lg text-xs font-medium transition ${statusBadgeClasses('warn')}`}>Shutdown</button>
          <button onClick={() => batchRun(stopVM, 'Stop')} className={`px-3 py-1.5 rounded-lg text-xs font-medium transition ${statusBadgeClasses('error')}`}>Stop</button>
          <button onClick={() => setBatchDeleteConfirm(true)} className={`px-3 py-1.5 rounded-lg text-xs font-medium transition ${statusBadgeClasses('error')}`}>Delete</button>
          <button onClick={() => setSelectedVMs(new Set())} className="p-1.5 hover:bg-[var(--surface-hover)] rounded-lg transition" title="Clear selection" aria-label="Clear selection"><X className="w-4 h-4 text-[var(--text-muted)]" /></button>
        </div>
      )}

      <ConfirmDialog
        open={!!deleteTarget}
        title="Delete VM"
        message={`This will stop '${deleteTarget?.name ?? ''}' if it is running, then remove its libvirt definition. If you already deleted disk files on the host, the server still drops the VM record. Disks under libvirt storage are not removed unless you use separate storage tools.`}
        confirmLabel="Delete"
        typeToMatch={deleteTarget?.name ?? ''}
        onConfirm={handleDelete}
        onCancel={() => setDeleteTarget(null)}
      />

      <ConfirmDialog
        open={batchDeleteConfirm}
        title="Delete VMs"
        message={`This will permanently delete ${selectedVMs.size} VMs (stop if running, then undefine). Type DELETE to confirm. Missing backend disk files are tolerated when cleaning up definitions.`}
        confirmLabel="Delete All"
        typeToMatch="DELETE"
        typeToMatchLabel="Type DELETE (all caps) to confirm bulk delete:"
        onConfirm={handleBatchDelete}
        onCancel={() => setBatchDeleteConfirm(false)}
      />

      {sshVm && (
        <VmResolvedSshConnectDialog
          open
          vmName={sshVm.name}
          guestIp={sshVm.guest_ip ?? ''}
          defaultUser="root"
          onClose={() => setSshVm(null)}
          onNotify={(m) => toast.success(m)}
        />
      )}
    </PageLayout>
  )
}
