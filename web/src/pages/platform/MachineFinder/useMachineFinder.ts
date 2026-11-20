// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useLocation, useNavigate, useSearchParams } from 'react-router'
import { batchVmDelete, batchVmSnapshot, precheckVmSnapshot } from '../../../api/platformVmLibvirt'
import {
  adoptPlatformVm,
  batchVmGuestIps,
  batchVmPower,
  createFromTemplate,
  createPlatformVm,
  getFleetFinder,
  listPlatformHosts,
  listPlatformVms,
  vmPower,
  createVmSnapshot,
  type CreatePlatformVmBody,
  type FleetFinderOverview,
  type PlatformHost,
  type PlatformVm,
} from '../../../api/platform'
import { fleetGuestQuery, getAiSecurity, type FleetGuestQueryReport, type SecurityReport } from '../../../api/ai'
import { useAi } from '../../../contexts/AiContext'
import { useToastContext } from '../../../contexts/ToastContext'
import { usePlatformDesktopTier } from '../../../hooks/usePlatformDesktopTier'
import { usePlatformInfo } from '../../../contexts/PlatformInfoContext'
import { formatUserError } from '../../../utils/apiError'
import { pruneMissingPlatformVms } from '../../../api/platformVmLifecycle'
import { purgeVmShortcuts } from '../../../utils/vmShortcuts'
import { toastQueuedOperation } from '../../../utils/platformTaskToast'
import { queuePlatformVmDelete } from '../../../utils/platformVmDelete'
import {
  cloudInitUserForOs,
  sizeToSpec,
  type VmWizardInitial,
  type VmWizardPayload,
} from '../../../components/platform/SimpleCreateVmWizard'
import { CLIENT_ONLY_FOLDERS, type MachineFinderLens, type MachineFinderOverlay } from './machineFinderTypes'

const VALID_LENSES = new Set<MachineFinderLens>(['grid', 'gallery', 'table', 'topology', 'timeline', 'heatmap', 'migration'])
const VALID_OVERLAYS = new Set<MachineFinderOverlay>([
  'default', 'health', 'backup', 'network', 'security', 'gpu', 'cost', 'migration',
])

function vmHasGpu(vm: PlatformVm): boolean {
  const tags = (vm.tags ?? []).join(' ').toLowerCase()
  return tags.includes('gpu') || tags.includes('nvidia') || tags.includes('cuda') || tags.includes('vgpu')
}

function parseLens(raw: string | null): MachineFinderLens {
  if (raw && VALID_LENSES.has(raw as MachineFinderLens)) return raw as MachineFinderLens
  // 'grid' is the canonical default lens — setLens() below strips ?lens= from the URL specifically
  // when switching back to grid, so a bare /platform/vms URL must resolve to grid to round-trip.
  return 'grid'
}

function parseOverlay(raw: string | null): MachineFinderOverlay {
  if (raw && VALID_OVERLAYS.has(raw as MachineFinderOverlay)) return raw as MachineFinderOverlay
  return 'default'
}

export function useMachineFinder() {
  const toast = useToastContext()
  const { openCopilot, setContextVmIds, setContextSummary } = useAi()
  const [tier] = usePlatformDesktopTier()
  const { lastEvent, refreshKey } = usePlatformInfo()
  const navigate = useNavigate()
  const location = useLocation()
  const [searchParams, setSearchParams] = useSearchParams()

  const folder = searchParams.get('folder') || 'all'
  const tag = searchParams.get('tag') || ''
  const project = searchParams.get('project') || ''
  const source = searchParams.get('source') || ''
  const lens = parseLens(searchParams.get('lens'))
  const overlay = parseOverlay(searchParams.get('overlay'))

  const [vms, setVms] = useState<PlatformVm[]>([])
  const [finder, setFinder] = useState<FleetFinderOverview | null>(null)
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [error, setError] = useState<{ message: string; error_code?: string; remediation?: string } | null>(null)
  const [wizardOpen, setWizardOpen] = useState(false)
  const [wizardInitial, setWizardInitial] = useState<VmWizardInitial | undefined>()
  const [windowsOpen, setWindowsOpen] = useState(false)
  const [dragVmId, setDragVmId] = useState<string | null>(null)
  const [migrateModal, setMigrateModal] = useState<{ vm: PlatformVm; destId: string; destName: string } | null>(null)
  const [dropHost, setDropHost] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [selectedVmId, setSelectedVmId] = useState<string | null>(null)
  const [selectedVmIds, setSelectedVmIds] = useState<Set<string>>(new Set())
  const [batchDeleteOpen, setBatchDeleteOpen] = useState(false)
  const [guestIpHints, setGuestIpHints] = useState<Record<string, string>>({})
  const [batchDeleteBusy, setBatchDeleteBusy] = useState(false)
  const [batchPowerBusy, setBatchPowerBusy] = useState(false)
  const [sshVm, setSshVm] = useState<PlatformVm | null>(null)
  const [pruneBusy, setPruneBusy] = useState(false)
  const [confirmPrune, setConfirmPrune] = useState(false)
  const [deleteVmTarget, setDeleteVmTarget] = useState<PlatformVm | null>(null)
  const [fleetGuestReport, setFleetGuestReport] = useState<FleetGuestQueryReport | null>(null)
  const [fleetGuestBusy, setFleetGuestBusy] = useState(false)
  const [aiSecurity, setAiSecurity] = useState<SecurityReport | null>(null)
  const deleteTaskToastRef = useRef<string | null>(null)

  const hostMap = useMemo(() => new Map(hosts.map((h) => [h.id, h.hostname])), [hosts])
  const vmById = useMemo(() => new Map(vms.map((v) => [v.id, v])), [vms])
  const hasGpuVms = useMemo(() => vms.some(vmHasGpu), [vms])

  const filteredVms = useMemo(() => {
    let list = vms
    if (folder === 'guest-gaps' || folder === 'guest_agent_missing') {
      list = list.filter((v) => {
        const s = v.guest_tools_status?.toLowerCase()
        return v.inventory_source !== 'kubevirt' && s !== 'healthy' && s !== 'installed'
      })
    }
    const q = search.trim().toLowerCase()
    if (!q) return list
    return list.filter(
      (v) => v.name.toLowerCase().includes(q) || (v.tags ?? []).some((t) => t.toLowerCase().includes(q)),
    )
  }, [vms, search, folder])

  // Resolve against the full list, not filteredVms — otherwise typing a search
  // query (or switching folder) that excludes the open VM makes its detail panel
  // vanish mid-interaction.
  const selectedVm = selectedVmId
    ? vms.find((v) => v.id === selectedVmId) ?? null
    : null

  const statsSubtitle = useMemo(() => {
    const total = vms.length
    const running = vms.filter((v) => v.observed_state === 'running').length
    const stopped = vms.filter((v) => v.observed_state === 'shutoff' || v.observed_state === 'stopped').length
    const needBackup = finder?.smart_folders.find((f) => f.id === 'unprotected')?.count ?? 0
    return `${total} machines · ${running} running · ${stopped} stopped · ${needBackup} need backup`
  }, [vms, finder])

  const sourceCounts = useMemo(() => ({
    libvirt: vms.filter((v) => (v.inventory_source ?? 'libvirt') === 'libvirt').length,
    kubevirt: vms.filter((v) => v.inventory_source === 'kubevirt').length,
    vmware: vms.filter((v) => v.inventory_source === 'vmware' || v.inventory_source === 'vsphere').length,
    discovered: vms.filter((v) => v.managed === false).length,
  }), [vms])

  const setFilter = useCallback(
    (next: { folder?: string; tag?: string; project?: string; source?: string }) => {
      const p = new URLSearchParams(searchParams)
      if (next.source !== undefined) {
        if (next.source) p.set('source', next.source)
        else p.delete('source')
        p.delete('folder')
        p.delete('tag')
        p.delete('project')
      }
      if (next.folder !== undefined) {
        if (next.folder === 'all') p.delete('folder')
        else p.set('folder', next.folder)
        p.delete('tag')
        p.delete('project')
        p.delete('source')
      }
      if (next.tag !== undefined) {
        if (next.tag) p.set('tag', next.tag)
        else p.delete('tag')
        p.delete('folder')
        p.delete('project')
      }
      if (next.project !== undefined) {
        if (next.project) p.set('project', next.project)
        else p.delete('project')
        p.delete('folder')
        p.delete('tag')
      }
      setSearchParams(p, { replace: true })
    },
    [searchParams, setSearchParams],
  )

  const setLens = useCallback(
    (next: MachineFinderLens) => {
      const p = new URLSearchParams(searchParams)
      if (next === 'grid') p.delete('lens')
      else p.set('lens', next)
      setSearchParams(p, { replace: true })
    },
    [searchParams, setSearchParams],
  )

  const setOverlay = useCallback(
    (next: MachineFinderOverlay) => {
      const p = new URLSearchParams(searchParams)
      if (next === 'default') p.delete('overlay')
      else p.set('overlay', next)
      setSearchParams(p, { replace: true })
    },
    [searchParams, setSearchParams],
  )

  const load = useCallback(async () => {
    const listParams: Parameters<typeof listPlatformVms>[0] = {}
    if (source) listParams.source = source
    if (tag) listParams.tag = tag
    else if (project) listParams.project = project
    else if (folder && folder !== 'all' && !CLIENT_ONLY_FOLDERS.has(folder)) listParams.folder = folder

    const [vResult, hResult, fResult] = await Promise.allSettled([
      listPlatformVms(listParams),
      listPlatformHosts(),
      getFleetFinder(),
    ])

    const failures: string[] = []
    if (vResult.status === 'fulfilled') {
      const vmList = Array.isArray(vResult.value) ? vResult.value : []
      setVms(vmList)
      const needIps = vmList
        .filter((v) => v.observed_state === 'running' && !v.guest_ip?.trim() && v.inventory_source !== 'kubevirt')
        .map((v) => v.id)
      if (needIps.length > 0) {
        void batchVmGuestIps(needIps.slice(0, 64))
          .then((r) => {
            const hints: Record<string, string> = {}
            for (const [vmId, row] of Object.entries(r.items ?? {})) {
              const ip = row.guest_ip?.trim() || row.nic_ip?.trim()
              if (ip) hints[vmId] = ip
            }
            setGuestIpHints(hints)
          })
          .catch(() => setGuestIpHints({}))
      } else {
        setGuestIpHints({})
      }
    } else {
      failures.push(formatUserError(vResult.reason))
    }
    if (hResult.status === 'fulfilled') {
      setHosts(Array.isArray(hResult.value) ? hResult.value : [])
    } else {
      failures.push(formatUserError(hResult.reason))
    }
    if (fResult.status === 'fulfilled') {
      setFinder(fResult.value)
    } else {
      setFinder(null)
    }

    if (failures.length > 0) {
      setError({
        message: failures[0],
        remediation: failures.length > 1
          ? `${failures.length} platform API calls failed. Confirm machina-daemon and machina-controller are running (systemctl status machina-daemon machina-controller).`
          : 'Confirm machina-daemon and machina-controller are running and refresh. VM list may be stale until services respond.',
      })
    } else {
      setError(null)
    }
  }, [folder, tag, project, source])

  useEffect(() => { void load() }, [load])

  // Power/create/snapshot/migrate actions queue an async task, so the immediate
  // load() after them still reads the pre-change state (card stays "running"
  // after Stop; a new VM doesn't appear). These vm.* events are emitted by the
  // worker AFTER the task runs and the DB is updated, so reloading on them
  // reflects the real state.
  useEffect(() => {
    if (!lastEvent) return
    if (lastEvent.kind.startsWith('vm.') || lastEvent.kind.startsWith('ha.')) void load()
  }, [refreshKey, lastEvent, load])

  // Apply ?vm=ID URL param to pre-select a VM after data has loaded, then drop
  // the param — otherwise every live vm.*/ha.* reload re-fires this and re-opens
  // the detail panel after the user dismissed it (vms changes on each event).
  // The topology lens manages ?vm= itself as persistent selection state (its own inspector,
  // independent of the command-center panel this effect opens) — skip there so this one-time
  // pre-select-and-drop doesn't fight that lens's click handler over the same param.
  const vmParamId = searchParams.get('vm')
  useEffect(() => {
    if (!vmParamId || vms.length === 0 || lens === 'topology') return
    if (vms.some((v) => v.id === vmParamId)) setSelectedVmId(vmParamId)
    const next = new URLSearchParams(searchParams)
    next.delete('vm')
    setSearchParams(next, { replace: true })
  }, [vmParamId, vms, lens, searchParams, setSearchParams])

  useEffect(() => {
    if (overlay !== 'security') return
    void getAiSecurity().then(setAiSecurity).catch(() => setAiSecurity(null))
  }, [overlay])

  const deleteTaskId = (location.state as { vmDeleteTaskId?: string; vmDeleteLabel?: string } | null)?.vmDeleteTaskId
  const deleteTaskLabel = (location.state as { vmDeleteLabel?: string } | null)?.vmDeleteLabel

  useEffect(() => {
    if (!deleteTaskId) return
    if (deleteTaskToastRef.current !== deleteTaskId) {
      deleteTaskToastRef.current = deleteTaskId
      toastQueuedOperation(toast, deleteTaskLabel ?? 'Delete queued', deleteTaskId, tier)
      void load()
    }
    navigate(`${location.pathname}${location.search}`, { replace: true, state: null })
  }, [deleteTaskId, deleteTaskLabel, load, location.pathname, location.search, navigate, toast, tier])

  useEffect(() => {
    const create = searchParams.get('create')
    if (!create) return
    setWizardInitial({
      name: create,
      os: searchParams.get('os') ?? undefined,
      size: searchParams.get('size') ?? undefined,
      network: searchParams.get('network') ?? undefined,
      hostId: searchParams.get('host_id') ?? undefined,
    })
    setWizardOpen(true)
    const next = new URLSearchParams(searchParams)
    next.delete('create')
    next.delete('os')
    next.delete('size')
    next.delete('network')
    next.delete('host_id')
    setSearchParams(next, { replace: true })
  }, [searchParams, setSearchParams])

  useEffect(() => { setSelectedVmIds(new Set()) }, [search, folder, tag, project, source, lens])

  const buildVmBody = (
    name: string,
    os: string,
    size: string,
    network: string,
    extraTags: string[] = [],
    cloudInitSshPubkey?: string,
    customSpec?: VmWizardPayload['customSpec'],
    graphicsType: VmWizardPayload['graphicsType'] = 'both',
    graphicsListen: VmWizardPayload['graphicsListen'] = '127.0.0.1',
  ): CreatePlatformVmBody => {
    const spec = sizeToSpec(size, customSpec)
    const cloudUser = cloudInitUserForOs(os)
    return {
      api_version: 'virt.zyvor.dev/v1',
      kind: 'VirtualMachine',
      metadata: { name },
      tags: [os, network, ...extraTags],
      spec: {
        cpu: { sockets: 1, cores: spec.cores },
        memory: spec.memory,
        storage: [{ name: 'root', size: spec.disk, class: 'silver' }],
        network: [{ network, ip_mode: 'dhcp' }],
        graphics: { type: graphicsType, listen: graphicsListen },
        ...(cloudInitSshPubkey ? { cloud_init: { user: cloudUser, ssh_pubkey: cloudInitSshPubkey } } : {}),
      },
    }
  }

  const handleCreate = async (payload: VmWizardPayload) => {
    try {
      if (payload.os === 'custom-iso') {
        navigate(`/platform/create-iso?name=${encodeURIComponent(payload.name)}`)
        return
      }
      if (payload.os === 'custom-virt-install') {
        const q = new URLSearchParams({ name: payload.name })
        if (payload.network) q.set('network', payload.network)
        navigate(`/platform/create-advanced?${q}`)
        return
      }
      if (payload.windows) {
        await handleWindowsCreate({
          name: payload.name,
          os: payload.os,
          size: payload.size,
          network: payload.network,
          windows: payload.windows,
        })
        return
      }
      const spec = sizeToSpec(payload.size, payload.customSpec)
      if (payload.fromTemplate) {
        const r = await createFromTemplate({
          template_ref: `${payload.os}@${payload.templateVersion ?? '1.0.0'}`,
          name: payload.name,
          memory: spec.memory,
          host_id: payload.hostId,
          template_vars: { hostname: payload.name, name: payload.name },
          cloud_init_user: cloudInitUserForOs(payload.os),
          cloud_init_ssh_pubkey: payload.cloudInitSshPubkey,
        })
        toastQueuedOperation(toast, `Deploying ${payload.name}`, r.task_id, tier)
      } else {
        const r = await createPlatformVm(
          buildVmBody(
            payload.name,
            payload.os,
            payload.size,
            payload.network,
            [],
            payload.cloudInitSshPubkey,
            payload.customSpec,
            payload.graphicsType,
            payload.graphicsListen,
          ),
        )
        toastQueuedOperation(toast, `Creating ${payload.name}`, r.task_id, tier)
      }
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
      throw e
    }
  }

  const handleWindowsCreate = async (payload: {
    name: string
    os: string
    size: string
    network: string
    windows: { virtio: boolean; virtioIsoPath: string; uefi: boolean; tpm: boolean; secureBoot: boolean }
  }) => {
    const spec = sizeToSpec(payload.size)
    const labels: Record<string, string> = { os_family: 'windows' }
    if (payload.windows.tpm) labels.tpm = 'true'
    if (payload.windows.secureBoot) labels.secure_boot = 'true'
    if (payload.windows.virtio) {
      labels.virtio_win = 'true'
      if (payload.windows.virtioIsoPath.trim()) {
        labels.virtio_win_iso = payload.windows.virtioIsoPath.trim()
      }
    }
    const body: CreatePlatformVmBody = {
      api_version: 'virt.zyvor.dev/v1',
      kind: 'VirtualMachine',
      metadata: { name: payload.name, labels },
      tags: ['windows', payload.os, payload.network],
      spec: {
        cpu: { sockets: 1, cores: spec.cores },
        memory: spec.memory,
        firmware: payload.windows.uefi ? 'uefi' : 'bios',
        storage: [{ name: 'root', size: spec.disk, class: 'silver' }],
        network: [{ network: payload.network, ip_mode: 'dhcp' }],
      },
    }
    const r = await createPlatformVm(body)
    toastQueuedOperation(toast, `Creating ${payload.name}`, r.task_id, tier)
    await load()
  }

  const onHostDrop = (hostId: string) => {
    const vmId = dragVmId
    setDropHost(null)
    setDragVmId(null)
    if (!vmId) return
    const vm = vmById.get(vmId)
    const host = hosts.find((h) => h.id === hostId)
    if (!vm || !host || vm.host_id === hostId) return
    setMigrateModal({ vm, destId: hostId, destName: host.hostname })
  }

  const pruneMissing = () => {
    setConfirmPrune(true)
  }

  const doPruneMissing = async () => {
    setPruneBusy(true)
    try {
      purgeVmShortcuts(filteredVms.map((v) => v.name))
      const r = await pruneMissingPlatformVms()
      toast.success(`Pruned ${r.deleted} missing record(s)`)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setPruneBusy(false)
    }
  }

  const toggleVmSelect = (id: string) => {
    setSelectedVmIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  const toggleAllVisible = () => {
    if (selectedVmIds.size === filteredVms.length) setSelectedVmIds(new Set())
    else setSelectedVmIds(new Set(filteredVms.map((v) => v.id)))
  }

  const displayGuestIp = useCallback(
    (vm: PlatformVm) => vm.guest_ip?.trim() || guestIpHints[vm.id] || '',
    [guestIpHints],
  )

  const handleBatchSnapshot = async () => {
    const ids = Array.from(selectedVmIds).filter((id) => vmById.get(id)?.inventory_source !== 'kubevirt')
    if (ids.length === 0) {
      toast.error('Batch snapshot applies to libvirt VMs only')
      return
    }
    const name = `batch-${Date.now()}`
    setBatchPowerBusy(true)
    try {
      const blocked: string[] = []
      for (const vmId of ids.slice(0, 32)) {
        const vm = vmById.get(vmId)
        if (vm?.observed_state !== 'running') continue
        try {
          const pre = await precheckVmSnapshot(vmId, { name, disk_only: true })
          if (pre.blocked) blocked.push(vm?.name ?? vmId)
        } catch {
          /* allow queue if precheck unavailable */
        }
      }
      if (blocked.length > 0) {
        toast.error(`Snapshot blocked for: ${blocked.join(', ')} (VFIO or free-space)`)
        return
      }
      const r = await batchVmSnapshot({ vm_ids: ids, name, disk_only: true })
      const ok = r.results.filter((x) => x.task_id).length
      const fail = r.results.filter((x) => x.error).length
      if (ok > 0) toast.success(`Snapshot queued for ${ok} VM(s)`)
      if (fail > 0) toast.error(`${fail} VM(s) failed`)
      setSelectedVmIds(new Set())
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBatchPowerBusy(false)
    }
  }

  const handleBatchPower = async (action: 'start' | 'stop' | 'shutdown' | 'pause' | 'resume') => {
    const ids = Array.from(selectedVmIds).filter((id) => vmById.get(id)?.inventory_source !== 'kubevirt')
    if (ids.length === 0) {
      toast.error('Batch power applies to libvirt VMs only')
      return
    }
    setBatchPowerBusy(true)
    try {
      const r = await batchVmPower(ids, action)
      const ok = r.results.filter((x) => x.task_id).length
      const fail = r.results.filter((x) => x.error).length
      if (ok > 0) toast.success(`${action} queued for ${ok} VM(s)`)
      if (fail > 0) toast.error(`${fail} VM(s) could not be updated`)
      // Neither queued nor errored (e.g. already in the requested state) — still
      // give feedback so the click doesn't look like it did nothing.
      if (ok === 0 && fail === 0) toast.info(`No power changes needed for the selected VM(s)`)
      setSelectedVmIds(new Set())
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBatchPowerBusy(false)
    }
  }

  const handleBatchDelete = async () => {
    setBatchDeleteOpen(false)
    setBatchDeleteBusy(true)
    try {
      const ids = Array.from(selectedVmIds)
      const libvirtIds = ids.filter((id) => vmById.get(id)?.inventory_source !== 'kubevirt')
      const kubevirtCount = ids.length - libvirtIds.length
      if (libvirtIds.length === 0) {
        toast.error('KubeVirt guests must be deleted from the cluster')
        return
      }
      const r = await batchVmDelete(libvirtIds, true)
      const okItems = r.results.filter((x) => !x.error)
      const failItems = r.results.filter((x) => x.error)
      if (okItems.length > 0) {
        purgeVmShortcuts(
          okItems.map((x) => vmById.get(x.vm_id)?.name).filter((n): n is string => Boolean(n)),
        )
        const removedIds = new Set(okItems.map((x) => x.vm_id))
        setVms((prev) => prev.filter((v) => !removedIds.has(v.id)))
        if (selectedVmId && removedIds.has(selectedVmId)) setSelectedVmId(null)
        const pruned = okItems.filter((x) => !x.task_id).length
        const queued = okItems.length - pruned
        if (pruned > 0 && queued > 0) {
          toast.success(`Removed ${pruned} stale record(s), delete queued for ${queued} VM(s)`)
        } else if (pruned > 0) {
          toast.success(`Removed ${pruned} stale VM record(s) from inventory`)
        } else {
          toast.success(`Delete queued for ${queued} VM(s)`)
        }
      }
      if (failItems.length > 0) toast.error(`${failItems.length} VM(s) could not be deleted`)
      if (kubevirtCount > 0) toast.error(`${kubevirtCount} KubeVirt guest(s) skipped — delete from the cluster`)
      setSelectedVmIds(new Set())
      void load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBatchDeleteBusy(false)
    }
  }

  const runFleetGuestQuery = async () => {
    setFleetGuestBusy(true)
    try {
      const vmIds = selectedVmId
        ? [selectedVmId]
        : filteredVms
            .filter((v) => v.observed_state === 'running' && v.inventory_source !== 'kubevirt')
            .slice(0, 25)
            .map((v) => v.id)
      const r = await fleetGuestQuery({
        query: search.trim() || 'guest agent status and logged in users',
        vm_ids: vmIds,
        project: project || undefined,
        tag: tag || undefined,
      })
      setFleetGuestReport(r)
      setContextVmIds(r.vms.map((v) => v.vm_id))
      setContextSummary(r.summary.slice(0, 120))
      openCopilot()
      toast.success(`${r.matched_count} VM(s) matched`)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setFleetGuestBusy(false)
    }
  }

  const vmPowerAction = async (vm: PlatformVm, action: 'start' | 'stop' | 'shutdown' | 'pause' | 'resume') => {
    if (vm.inventory_source === 'kubevirt') {
      toast.error('Power actions apply to libvirt VMs only')
      return
    }
    try {
      const r = await vmPower(vm.id, action)
      toastQueuedOperation(toast, `${action} ${vm.name}`, r.task_id, tier)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const vmSnapshotAction = async (vm: PlatformVm) => {
    if (vm.inventory_source === 'kubevirt') {
      toast.error('Snapshots apply to libvirt VMs only')
      return
    }
    const name = `snap-${Date.now()}`
    try {
      const r = await createVmSnapshot(vm.id, name)
      toastQueuedOperation(toast, `Snapshot ${vm.name}`, r.task_id, tier)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const vmDeleteAction = (vm: PlatformVm) => {
    if (vm.inventory_source === 'kubevirt') {
      toast.error('KubeVirt guests must be deleted from the cluster')
      return
    }
    setDeleteVmTarget(vm)
  }

  const doVmDeleteAction = async (vm: PlatformVm) => {
    try {
      await queuePlatformVmDelete(vm, toast, tier)
      if (selectedVmId === vm.id) setSelectedVmId(null)
      setSelectedVmIds((prev) => {
        if (!prev.has(vm.id)) return prev
        const next = new Set(prev)
        next.delete(vm.id)
        return next
      })
      setVms((prev) => prev.filter((v) => v.id !== vm.id))
      if (vm.host_id) {
        setHosts((prev) =>
          prev.map((h) =>
            h.id === vm.host_id
              ? { ...h, vm_count: Math.max(0, (h.vm_count ?? 1) - 1) }
              : h,
          ),
        )
      }
      void load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const adoptVm = async (vm: PlatformVm) => {
    try {
      await adoptPlatformVm(vm.id)
      toast.success('Adopted')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const activeLabel =
    tag ? `#${tag}` :
    project ? project :
    source ? source :
    folder === 'guest-gaps' || folder === 'guest_agent_missing' ? 'Guest Agent Missing' :
    folder === 'missing' ? 'Missing from inventory' :
    finder?.smart_folders.find((f) => f.id === folder)?.label ?? 'All Machines'

  return {
    folder,
    tag,
    project,
    source,
    lens,
    overlay,
    vms,
    finder,
    hosts,
    hostMap,
    hasGpuVms,
    vmById,
    error,
    search,
    setSearch,
    filteredVms,
    selectedVm,
    selectedVmId,
    setSelectedVmId,
    selectedVmIds,
    setSelectedVmIds,
    statsSubtitle,
    sourceCounts,
    activeLabel,
    wizardOpen,
    setWizardOpen,
    wizardInitial,
    setWizardInitial,
    windowsOpen,
    setWindowsOpen,
    dragVmId,
    setDragVmId,
    migrateModal,
    setMigrateModal,
    dropHost,
    setDropHost,
    batchDeleteOpen,
    setBatchDeleteOpen,
    batchDeleteBusy,
    batchPowerBusy,
    sshVm,
    setSshVm,
    pruneBusy,
    confirmPrune,
    setConfirmPrune,
    deleteVmTarget,
    setDeleteVmTarget,
    doPruneMissing,
    doVmDeleteAction,
    fleetGuestReport,
    setFleetGuestReport,
    fleetGuestBusy,
    aiSecurity,
    tier,
    load,
    setFilter,
    setLens,
    setOverlay,
    handleCreate,
    onHostDrop,
    pruneMissing,
    toggleVmSelect,
    toggleAllVisible,
    handleBatchSnapshot,
    handleBatchPower,
    handleBatchDelete,
    runFleetGuestQuery,
    displayGuestIp,
    vmPowerAction,
    vmSnapshotAction,
    vmDeleteAction,
    adoptVm,
    searchParams,
    setSearchParams,
  }
}

export type MachineFinderState = ReturnType<typeof useMachineFinder>
