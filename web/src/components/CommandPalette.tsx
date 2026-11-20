// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState, useEffect, useRef, useCallback } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { useLocation, useNavigate } from 'react-router'
import { Search, Plus, Camera, Server, Play, Square, Power, Terminal, ArrowRight, Network, HardDrive, Clock, Star, Boxes, Upload, Pin, Keyboard, Info, Bell, ClipboardList, Activity, Settings, Monitor, Stethoscope, Plug } from 'lucide-react'
import { navigateVmSshSession } from './vm/VmSshConnectDialog'
import { listVMs, startVM, stopVM, shutdownVM, VmInfo } from '../api/vm'
import { listPlatformHosts, listPlatformVms } from '../api/platform'
import { listNetworks, NetworkInfo } from '../api/network'
import { listPools, StoragePoolInfo } from '../api/storage'
import { listAllSnapshots, SnapshotInfo } from '../api/snapshot'
import { useToastContext } from '../contexts/ToastContext'
import { useKeyboardShortcut } from '../hooks/useKeyboardShortcut'
import { navGroups, navItemVisible, navGroupItems, TOP_BAR_QUICK_LINKS } from '../utils/routes'
import { usePlatformInfoSlow } from '../contexts/PlatformInfoContext'
import { useAi } from '../contexts/AiContext'
import { useAuth } from '../contexts/AuthContext'
import { getStateBadgeClasses } from '../utils/vm'
import { poolStateBadgeClasses, statusBadgeClasses, statusToneClass } from '../utils/semanticColors'
import { getRecentVMs } from '../utils/recentVMs'
import { getPinnedVMs } from '../utils/pinnedVMs'
import { getRecentPages, recordRecentPage } from '../utils/recentPages'
import { getPinnedPages, isPagePinned, togglePinnedPage } from '../utils/pinnedPages'
import { getPageLabel } from '../utils/pageLabels'
import type { HelpTab } from './HelpDialog'
import { formatUserError } from '../utils/apiError'
import {
  executePlatformCommand,
  parsePlatformCommand,
  platformCommandSuggestions,
  type PlatformCommand,
} from '../utils/platformCommands'
import { aiSpotlight, runNlOps, type NlOpsPlan, type SpotlightIntent, type SpotlightResult } from '../api/ai'
import { isInputFocused } from '../hooks/useKeyboardShortcut'
import { consolePaletteShortcutActive } from './consolehub/ConsoleCommandPalette'
import { loadPlatformDesktopTier } from '../utils/platformDesktopTier'
import { operationsHubHref, tasksHubHref, activityHubHref } from '../utils/platformHubLinks'
import { groupSpotlightByZone, spotlightNavForTier, spotlightPathSetForTier, spotlightZoneOrder } from '../utils/platformSpotlightNav'
import { cinemaHubPath } from '../utils/consoleExperienceMode'

const BACKDROP_INITIAL = { opacity: 0 }
const BACKDROP_ANIMATE = { opacity: 1 }
const BACKDROP_EXIT = { opacity: 0 }
const BACKDROP_TRANSITION = { duration: 0.15 }
const PANEL_INITIAL = { opacity: 0, scale: 0.96, y: -8 }
const PANEL_ANIMATE = { opacity: 1, scale: 1, y: 0 }
const PANEL_EXIT = { opacity: 0, scale: 0.96, y: -8 }
const PANEL_TRANSITION = { type: 'spring' as const, stiffness: 340, damping: 28 }

interface CommandPaletteProps {
  onOpenHelp?: (tab?: HelpTab) => void
  /** When true, show Zyra Spotlight branding (⌘Space primary). */
  spotlight?: boolean
}

interface PaletteItem {
  id: string
  icon: React.ReactNode
  label: string
  sublabel?: string
  badge?: React.ReactNode
  action: () => void
  category: string
}

export default function CommandPalette({ onOpenHelp, spotlight = false }: CommandPaletteProps) {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [vms, setVMs] = useState<VmInfo[]>([])
  const [networks, setNetworks] = useState<NetworkInfo[]>([])
  const [pools, setPools] = useState<StoragePoolInfo[]>([])
  const [snapshots, setSnapshots] = useState<SnapshotInfo[]>([])
  const [platformVms, setPlatformVms] = useState<{ id: string; name: string; observed_state: string; guest_ip?: string | null }[]>([])
  const [platformHosts, setPlatformHosts] = useState<{ id: string; hostname: string; state: string }[]>([])
  const [loading, setLoading] = useState(false)
  const [reviewCommand, setReviewCommand] = useState<PlatformCommand | null>(null)
  const [reviewNlOps, setReviewNlOps] = useState<NlOpsPlan | null>(null)
  const [nlOpsQuery, setNlOpsQuery] = useState('')
  const [spotlightIntents, setSpotlightIntents] = useState<SpotlightIntent[]>([])
  const [executing, setExecuting] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLDivElement>(null)
  const navigate = useNavigate()
  const location = useLocation()
  const toast = useToastContext()
  const [pinnedPages, setPinnedPages] = useState<string[]>(() => getPinnedPages())
  const { info } = usePlatformInfoSlow()
  const { username } = useAuth()
  const { openCopilotWithQuery } = useAi()

  const toggle = useCallback(() => {
    if (!open && isInputFocused()) return
    // On console pages the console command palette owns ⌘/Ctrl+K; don't also open
    // the global palette (the shortcut hook treats Ctrl and Meta interchangeably).
    if (!open && consolePaletteShortcutActive()) return
    setOpen((o) => !o)
  }, [open])

  useKeyboardShortcut({ key: 'k', ctrl: true, handler: toggle })
  useKeyboardShortcut({
    key: ' ',
    meta: true,
    handler: (e) => {
      if (isInputFocused()) return
      e.preventDefault()
      setOpen(true)
    },
    enabled: spotlight,
  })

  const appliedPrefillForOpenRef = useRef(false)

  const close = useCallback(() => {
    setOpen(false)
    setQuery('')
    setReviewCommand(null)
    setReviewNlOps(null)
    setNlOpsQuery('')
    setSpotlightIntents([])
  }, [])

  useEffect(() => {
    const onOpen = (e: Event) => {
      const detail = (e as CustomEvent<{ prefill?: string }>).detail
      const prefill = detail?.prefill?.trim()
      if (prefill) {
        appliedPrefillForOpenRef.current = true
        setQuery(prefill)
      }
      setOpen(true)
    }
    window.addEventListener('machina-open-spotlight', onOpen)
    return () => window.removeEventListener('machina-open-spotlight', onOpen)
  }, [])

  useEffect(() => {
    close()
  }, [location.pathname, close])

  useEffect(() => {
    if (!open) return
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      if (reviewNlOps) {
        setReviewNlOps(null)
        e.preventDefault()
        return
      }
      if (reviewCommand) {
        setReviewCommand(null)
        e.preventDefault()
        return
      }
      close()
      e.preventDefault()
    }
    window.addEventListener('keydown', onKeyDown, true)
    return () => window.removeEventListener('keydown', onKeyDown, true)
  }, [open, close, reviewCommand, reviewNlOps])

  // Seed empty query when spotlight opens without a page-context prefill.
  useEffect(() => {
    if (!open) {
      appliedPrefillForOpenRef.current = false
      return
    }
    if (appliedPrefillForOpenRef.current) return
    appliedPrefillForOpenRef.current = true
    setQuery('')
    setSelectedIndex(0)
  }, [open])

  // Fetch VMs when palette opens. Deferred past the entrance spring's settle
  // time (~320ms for stiffness 340/damping 28) so the resulting setState
  // burst doesn't land mid-animation and interrupt it (Framer Motion can
  // leave the animation's completion promise unresolved — frozen mid-fade —
  // if a re-render lands while it's still interpolating).
  useEffect(() => {
    if (!open) return
    setPinnedPages(getPinnedPages())
    setLoading(true)
    const id = setTimeout(() => {
      Promise.allSettled([
        listVMs(),
        listNetworks(),
        listPools(),
        listAllSnapshots(),
        info?.control_plane?.proxy_url ? listPlatformVms() : Promise.resolve([]),
        info?.control_plane?.proxy_url ? listPlatformHosts() : Promise.resolve([]),
      ])
        .then(([vmR, netR, poolR, snapR, pVmR, pHostR]) => {
          setVMs(vmR.status === 'fulfilled' ? vmR.value : [])
          setNetworks(netR.status === 'fulfilled' ? netR.value : [])
          setPools(poolR.status === 'fulfilled' ? poolR.value : [])
          setSnapshots(snapR.status === 'fulfilled' ? snapR.value : [])
          setPlatformVms(pVmR.status === 'fulfilled' ? pVmR.value.map((v) => ({
            id: v.id,
            name: v.name,
            observed_state: v.observed_state,
            guest_ip: v.guest_ip,
          })) : [])
          setPlatformHosts(pHostR.status === 'fulfilled' ? pHostR.value.map((h) => ({ id: h.id, hostname: h.hostname, state: h.state })) : [])
        })
        .finally(() => setLoading(false))
    }, 350)
    return () => clearTimeout(id)
  }, [open, info?.control_plane?.proxy_url])

  // Focus input when opened
  useEffect(() => {
    if (!open) return
    const id = setTimeout(() => inputRef.current?.focus(), 50)
    return () => clearTimeout(id)
  }, [open])

  const platformConnected = Boolean(info?.control_plane?.proxy_url)
  const onPlatformDesktop = location.pathname.startsWith('/platform')
  const platformTier = loadPlatformDesktopTier()
  const platformSpotlightPaths = onPlatformDesktop
    ? spotlightPathSetForTier(platformTier, info)
    : new Set<string>()

  useEffect(() => {
    if (!open || !platformConnected || !query.trim() || query.length < 3) {
      setSpotlightIntents([])
      return
    }
    const t = window.setTimeout(() => {
      void aiSpotlight(query.trim())
        .then((r: SpotlightResult) => setSpotlightIntents(r.intents ?? []))
        .catch(() => setSpotlightIntents([]))
    }, 200)
    return () => window.clearTimeout(t)
  }, [open, platformConnected, query])

  const go = useCallback((path: string) => {
    recordRecentPage(path)
    close()
    navigate(path)
  }, [close, navigate])

  const onlineHostCount = platformHosts.filter((h) => h.state === 'online').length

  const confirmNlOps = useCallback(async () => {
    if (!nlOpsQuery.trim()) return
    setExecuting(true)
    try {
      const plan = await runNlOps(nlOpsQuery.trim(), false)
      toast.success(plan.action_ids.length > 0 ? `Queued ${plan.action_ids.length} approval(s)` : plan.summary)
      close()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setExecuting(false)
    }
  }, [close, nlOpsQuery, toast])

  const openNlOpsReview = useCallback(async (q: string, label: string) => {
    setNlOpsQuery(q)
    setExecuting(true)
    try {
      const plan = await runNlOps(q, true)
      setReviewNlOps({ ...plan, summary: label || plan.summary })
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setExecuting(false)
    }
  }, [toast])

  const confirmPlatformCommand = useCallback(async (cmd: PlatformCommand) => {
    setExecuting(true)
    try {
      const result = await executePlatformCommand(cmd)
      toast.success(result.message)
      if (result.navigate) {
        go(result.navigate)
      } else {
        close()
      }
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setExecuting(false)
      setReviewCommand(null)
    }
  }, [close, go, toast])

  const vmAction = useCallback(async (name: string, fn: (n: string) => Promise<void>, label: string) => {
    close()
    try {
      await fn(name)
      toast.success(`${label} '${name}' OK`)
    } catch (e: unknown) {
      toast.error(`${label} '${name}' failed: ${formatUserError(e)}`)
    }
  }, [close, toast])

  // Build items list
  const items: PaletteItem[] = []

  const currentPath = location.pathname

  // Pinned app pages
  for (const path of pinnedPages) {
    items.push({
      id: `pin-page-${path}`,
      icon: <Star className={`w-4 h-4 ${statusToneClass('warn')}`} />,
      label: getPageLabel(path),
      action: () => go(path),
      category: 'Pinned pages',
    })
  }

  // Recent app pages (non-VM routes)
  const recentPagePaths = getRecentPages()
  for (const path of recentPagePaths) {
    if (pinnedPages.includes(path)) continue
    items.push({
      id: `recent-page-${path}`,
      icon: <Clock className="w-4 h-4" />,
      label: getPageLabel(path),
      action: () => go(path),
      category: 'Recent pages',
    })
  }

  items.push({
    id: 'pin-current-page',
    icon: <Pin className={`w-4 h-4 ${isPagePinned(currentPath) ? statusToneClass('warn') : ''}`} />,
    label: isPagePinned(currentPath) ? 'Unpin current page from navbar' : 'Pin current page to navbar',
    sublabel: getPageLabel(currentPath),
    action: () => {
      setPinnedPages(togglePinnedPage(currentPath))
      close()
    },
    category: 'Quick Actions',
  })

  if (onPlatformDesktop) {
    for (const { zone, items: zoneItems } of groupSpotlightByZone(spotlightNavForTier(platformTier, info))) {
      for (const entry of zoneItems) {
        items.push({
          id: entry.id,
          icon: entry.kind === 'hub' ? <Boxes className="w-4 h-4 text-[var(--link)]" /> : <ArrowRight className="w-4 h-4" />,
          label: entry.label,
          sublabel: entry.kind === 'hub'
            ? `Hub · ${entry.description ?? entry.zone}`
            : entry.description ?? entry.zone,
          action: () => go(entry.path),
          category: zone,
        })
      }
    }
  }

  // Platform quick actions (deduped when already in hub zone nav)
  const desktopTier = loadPlatformDesktopTier()
  const platformActions: PaletteItem[] = [
    { id: 'plat-create-vm', icon: <Plus className="w-4 h-4" />, label: 'Create VM', sublabel: 'Platform wizard', action: () => go('/platform/vms'), category: 'Platform Actions' },
    { id: 'plat-migrate', icon: <Upload className="w-4 h-4" />, label: 'Import VM', sublabel: 'Migration Radar', action: () => go('/platform/migration'), category: 'Platform Actions' },
    { id: 'plat-iso', icon: <HardDrive className="w-4 h-4" />, label: 'Upload ISO', action: () => go('/platform/content'), category: 'Platform Actions' },
    { id: 'plat-tasks', icon: <ClipboardList className="w-4 h-4" />, label: 'Show running tasks', action: () => go(tasksHubHref(desktopTier)), category: 'Platform Actions' },
    { id: 'plat-alerts', icon: <Bell className="w-4 h-4" />, label: 'Show alerts', action: () => go(operationsHubHref(desktopTier)), category: 'Platform Actions' },
    { id: 'plat-activity', icon: <Activity className="w-4 h-4" />, label: 'Activity Monitor', action: () => go(activityHubHref(desktopTier)), category: 'Platform Actions' },
    { id: 'plat-settings', icon: <Settings className="w-4 h-4" />, label: 'Platform settings', action: () => go('/platform/settings'), category: 'Platform Actions' },
  ]
  for (const action of platformActions) {
    if (onPlatformDesktop) {
      const pathById: Record<string, string> = {
        'plat-create-vm': '/platform/vms',
        'plat-migrate': '/platform/migration',
        'plat-iso': '/platform/content',
        'plat-tasks': tasksHubHref(desktopTier),
        'plat-alerts': operationsHubHref(desktopTier),
        'plat-activity': activityHubHref(desktopTier),
        'plat-settings': '/platform/settings',
      }
      if (platformSpotlightPaths.has(pathById[action.id] ?? '')) continue
    }
    items.push(action)
  }

  if (onPlatformDesktop) {
    for (const pv of platformVms) {
      if (platformSpotlightPaths.has(`/platform/vms/${pv.id}`)) continue
      items.push({
        id: `platform-vm-${pv.id}`,
        icon: <Boxes className="w-4 h-4" />,
        label: pv.name,
        sublabel: 'Platform VM',
        action: () => go(`/platform/vms/${pv.id}`),
        category: 'Fleet',
      })
      items.push(
        {
          id: `platform-vm-bootdoctor-${pv.id}`,
          icon: <Stethoscope className="w-4 h-4" />,
          label: `${pv.name} — Boot Doctor (check & repair disk)`,
          action: () => go(`/platform/vms/${pv.id}?action=bootdoctor`),
          category: 'Fleet',
        },
        {
          id: `platform-vm-agent-${pv.id}`,
          icon: <Plug className="w-4 h-4" />,
          label: `${pv.name} — Set up guest agent`,
          action: () => go(`/platform/vms/${pv.id}?action=agent`),
          category: 'Fleet',
        },
      )
      if (pv.observed_state === 'running') {
        items.push(
          {
            id: `platform-vm-vnc-${pv.id}`,
            icon: <Monitor className="w-4 h-4" />,
            label: `${pv.name} — Open Cinema`,
            action: () => go(cinemaHubPath(pv.id)),
            category: 'Fleet',
          },
          {
            id: `platform-vm-ssh-${pv.id}`,
            icon: <Terminal className="w-4 h-4" />,
            label: `${pv.name} — SSH`,
            sublabel: pv.guest_ip ?? 'enter IP on connect',
            action: () => {
              const ip = pv.guest_ip?.trim()
              if (ip) navigateVmSshSession(pv.name, ip, 'ubuntu')
              else go(`/platform/vms/${pv.id}`)
            },
            category: 'Fleet',
          },
        )
      }
    }
    for (const ph of platformHosts) {
      if (platformSpotlightPaths.has(`/platform/hosts/${ph.id}`)) continue
      items.push({
        id: `platform-host-${ph.id}`,
        icon: <Server className="w-4 h-4" />,
        label: ph.hostname,
        sublabel: 'Platform host',
        action: () => go(`/platform/hosts/${ph.id}`),
        category: 'Fleet',
      })
    }
  }

  const knownVmNames = new Set([
    ...vms.map((v) => v.name),
    ...platformVms.map((pv) => pv.name),
  ])

  // Recent VMs (drop names deleted from inventory)
  const recentNames = getRecentVMs().filter((n) => knownVmNames.has(n))
  for (const rName of recentNames) {
    items.push({
      id: `recent-${rName}`,
      icon: <Clock className="w-4 h-4" />,
      label: rName,
      action: () => go(`/vms/${rName}`),
      category: 'Recent',
    })
  }

  // Pinned VMs
  const pinnedNames = getPinnedVMs().filter((n) => knownVmNames.has(n))
  for (const pName of pinnedNames) {
    if (recentNames.includes(pName)) continue // avoid duplicates with Recent
    items.push({
      id: `pinned-${pName}`,
      icon: <Star className="w-4 h-4" />,
      label: pName,
      action: () => go(`/vms/${pName}`),
      category: 'Pinned',
    })
  }

  // Quick actions
  items.push(
    { id: 'qa-create', icon: <Plus className="w-4 h-4" />, label: 'Create VM', action: () => go('/create'), category: 'Quick Actions' },
    { id: 'qa-host-ssh', icon: <Terminal className="w-4 h-4" />, label: 'Host SSH (hypervisor)', action: () => go('/host-ssh'), category: 'Quick Actions' },
    { id: 'qa-snap', icon: <Camera className="w-4 h-4" />, label: 'Snapshots', action: () => go('/snapshots'), category: 'Quick Actions' },
    { id: 'qa-disk-images', icon: <HardDrive className="w-4 h-4" />, label: 'Disk Images', sublabel: 'g i', action: () => go('/disk-images'), category: 'Quick Actions' },
    {
      id: 'qa-kubevirt-upload',
      icon: <Upload className="w-4 h-4" />,
      label: 'Upload qcow2 to KubeVirt',
      sublabel: 'last picked qcow2',
      action: () => go('/disk-images?kv=open'),
      category: 'Quick Actions',
    },
    { id: 'qa-kubevirt-workloads', icon: <Boxes className="w-4 h-4" />, label: 'KubeVirt Workloads', sublabel: 'g k', action: () => go('/k8s/workloads'), category: 'Quick Actions' },
  )
  items.push(
    { id: 'qa-fleet-cloud', icon: <Server className="w-4 h-4" />, label: 'Fleet Cloud overview', sublabel: 'g o', action: () => go('/fleet-cloud'), category: 'Quick Actions' },
    { id: 'qa-fleet-cloud-instances', icon: <Server className="w-4 h-4" />, label: 'Fleet Cloud instances', sublabel: 'g o', action: () => go('/fleet-cloud/instances'), category: 'Quick Actions' },
    { id: 'qa-fleet-cloud-create', icon: <Plus className="w-4 h-4" />, label: 'Create Fleet Cloud instance', sublabel: 'g o', action: () => go('/fleet-cloud/create'), category: 'Quick Actions' },
    { id: 'qa-fleet-cloud-images', icon: <HardDrive className="w-4 h-4" />, label: 'Fleet Cloud images', sublabel: 'g o', action: () => go('/fleet-cloud/images'), category: 'Quick Actions' },
  )
  if (onOpenHelp) {
    items.push(
      {
        id: 'help-shortcuts',
        icon: <Keyboard className="w-4 h-4" />,
        label: 'Help: keyboard shortcuts',
        sublabel: '?',
        action: () => {
          close()
          onOpenHelp('shortcuts')
        },
        category: 'Help',
      },
      {
        id: 'help-about',
        icon: <Info className="w-4 h-4" />,
        label: 'Help: about Machina',
        sublabel: 'zyvor.dev',
        action: () => {
          close()
          onOpenHelp('about')
        },
        category: 'Help',
      },
    )
  }

  // Classic Machina nav — hidden on Platform desktop (Spotlight registry covers platform routes).
  if (!onPlatformDesktop) {
    for (const group of navGroups) {
      for (const item of navGroupItems(group)) {
        if (!navItemVisible(item, username)) continue
        items.push({
          id: `nav-${item.to}`,
          icon: item.icon,
          label: item.label,
          sublabel: group.label,
          action: () => go(item.to),
          category: 'Pages',
        })
      }
    }
    for (const item of TOP_BAR_QUICK_LINKS) {
      items.push({
        id: `quick-${item.to}`,
        icon: item.icon,
        label: item.label,
        sublabel: 'Shortcuts',
        action: () => go(item.to),
        category: 'Pages',
      })
    }
  }

  // VMs
  for (const vm of vms) {
    items.push({
      id: `vm-${vm.name}`,
      icon: <Server className="w-4 h-4" />,
      label: vm.name,
      badge: <span className={`px-1.5 py-0.5 rounded text-[10px] font-medium ${getStateBadgeClasses(vm.state)}`}>{vm.state}</span>,
      action: () => go(`/vms/${vm.name}`),
      category: 'Virtual Machines',
    })
    if (vm.state === 'running') {
      items.push(
        { id: `vm-console-${vm.name}`, icon: <Monitor className="w-4 h-4" />, label: `${vm.name} — Open Cinema`, action: () => go(`/vms/${vm.name}/consolehub?mode=cinema`), category: 'Virtual Machines' },
        {
          id: `vm-ssh-${vm.name}`,
          icon: <Terminal className="w-4 h-4" />,
          label: `${vm.name} — SSH`,
          sublabel: vm.guest_ip ?? 'open VM to enter IP',
          action: () => {
            const ip = vm.guest_ip?.trim()
            if (ip) navigateVmSshSession(vm.name, ip, 'root')
            else go(`/vms/${vm.name}`)
          },
          category: 'Virtual Machines',
        },
        { id: `vm-shutdown-${vm.name}`, icon: <Power className="w-4 h-4" />, label: `${vm.name} — Shutdown`, action: () => vmAction(vm.name, shutdownVM, 'Shutdown'), category: 'Virtual Machines' },
        { id: `vm-stop-${vm.name}`, icon: <Square className="w-4 h-4" />, label: `${vm.name} — Force Stop`, action: () => vmAction(vm.name, stopVM, 'Stop'), category: 'Virtual Machines' },
      )
    }
    if (vm.state === 'shutoff') {
      items.push(
        { id: `vm-start-${vm.name}`, icon: <Play className="w-4 h-4" />, label: `${vm.name} — Start`, action: () => vmAction(vm.name, startVM, 'Start'), category: 'Virtual Machines' },
      )
    }
  }

  // Networks
  for (const net of networks) {
    items.push({
      id: `net-${net.name}`, icon: <Network className="w-4 h-4" />, label: net.name,
      badge: <span className={`px-1.5 py-0.5 rounded text-[10px] font-medium ${statusBadgeClasses(net.active ? 'ok' : 'error')}`}>{net.active ? 'active' : 'inactive'}</span>,
      action: () => go('/networks'), category: 'Networks',
    })
  }

  // Storage Pools
  for (const pool of pools) {
    items.push({
      id: `pool-${pool.name}`, icon: <HardDrive className="w-4 h-4" />, label: pool.name,
      badge: <span className={`px-1.5 py-0.5 rounded text-[10px] font-medium ${poolStateBadgeClasses(pool.state)}`}>{pool.state}</span>,
      action: () => go('/storage'), category: 'Storage Pools',
    })
  }

  // Snapshots
  for (const snap of snapshots) {
    items.push({
      id: `snap-${snap.vm_name}-${snap.name}`, icon: <Camera className="w-4 h-4" />,
      label: `${snap.vm_name} / ${snap.name}`,
      action: () => go(`/vms/${snap.vm_name}`), category: 'Snapshots',
    })
  }

  // Filter
  const q = query.toLowerCase()
  const parsedCommand = parsePlatformCommand(query, onlineHostCount)

  for (const intent of spotlightIntents) {
    const cmdId = intent.id as PlatformCommand['id']
    const knownCmd = ['import-networks', 'import-storage', 'sync-hosts', 'create-vm', 'show-offline-hosts', 'backup-vm', 'migrate-vm', 'enable-ha-vm'].includes(cmdId)
    items.unshift({
      id: `ai-intent-${intent.id}`,
      icon: <Terminal className="w-4 h-4 text-orange-400" />,
      label: intent.label,
      sublabel: intent.review || 'Zyra Spotlight',
      action: () => {
        if (intent.navigate) {
          go(intent.navigate)
        } else if (intent.action === 'nl_ops') {
          const q = (intent.prefill as { query?: string } | undefined)?.query ?? query.trim()
          void openNlOpsReview(q, intent.label)
        } else if (knownCmd) {
          const prefill = intent.prefill as { name?: string; os?: string; size?: string; network?: string; target_host?: string } | undefined
          setReviewCommand({
            id: cmdId,
            label: intent.label,
            review: intent.review,
            vmName: intent.vm_name ?? prefill?.name,
            prefill: prefill
              ? {
                  name: prefill.name ?? intent.vm_name,
                  os: typeof prefill.os === 'string' ? prefill.os : undefined,
                  size: typeof prefill.size === 'string' ? prefill.size : undefined,
                  network: typeof prefill.network === 'string' ? prefill.network : undefined,
                  target_host: typeof prefill.target_host === 'string' ? prefill.target_host : undefined,
                }
              : intent.vm_name
                ? { name: intent.vm_name }
                : undefined,
          })
        }
      },
      category: 'Zyra Spotlight',
    })
  }

  if (parsedCommand && q) {
    items.unshift({
      id: `plat-cmd-${parsedCommand.id}`,
      icon: <Terminal className="w-4 h-4 text-orange-400" />,
      label: parsedCommand.label,
      sublabel: 'Platform command — review',
      action: () => setReviewCommand(parsedCommand),
      category: 'Platform Commands',
    })
  } else if (query.startsWith('>') || /^import |^sync |^create |^show /i.test(query)) {
    for (const sug of platformCommandSuggestions(onlineHostCount)) {
      if (sug.label.toLowerCase().includes(q.replace(/^>/, '').trim()) || q === '>') {
        items.unshift({
          id: `plat-sug-${sug.id}`,
          icon: <Terminal className="w-4 h-4 text-orange-400" />,
          label: sug.label,
          sublabel: sug.review,
          action: () => setReviewCommand(sug),
          category: 'Platform Commands',
        })
      }
    }
  }

  const queryTokens = q.split(/\s+/).filter(Boolean)
  const matchesQuery = (label: string, sublabel?: string, category?: string) => {
    if (queryTokens.length === 0) return true
    const hay = `${label} ${sublabel ?? ''} ${category ?? ''}`.toLowerCase()
    return queryTokens.every((token) => hay.includes(token))
  }
  const filtered = q
    ? items.filter((i) => matchesQuery(i.label, i.sublabel, i.category) || i.category === 'Platform Commands')
    : items

  // Group by category
  const categories = [
    'Pinned pages',
    'Recent pages',
    'Recent',
    'Pinned',
    'Zyra Spotlight',
    'Platform Commands',
    ...spotlightZoneOrder(),
    'Platform Actions',
    'Quick Actions',
    'Help',
    'Setup',
    'Pages',
    'Virtual Machines',
    'Networks',
    'Storage Pools',
    'Snapshots',
  ]
  const grouped = categories
    .map(cat => ({ cat, items: filtered.filter(i => i.category === cat) }))
    .filter(g => g.items.length > 0)

  const flatFiltered = grouped.flatMap(g => g.items)
  const clampedIndex = Math.min(selectedIndex, Math.max(0, flatFiltered.length - 1))

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      if (reviewNlOps) { setReviewNlOps(null); return }
      if (reviewCommand) { setReviewCommand(null); return }
      close()
      return
    }
    if (reviewNlOps && e.key === 'Enter') {
      e.preventDefault()
      void confirmNlOps()
      return
    }
    if (reviewCommand && e.key === 'Enter') {
      e.preventDefault()
      void confirmPlatformCommand(reviewCommand)
      return
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setSelectedIndex(i => Math.min(i + 1, flatFiltered.length - 1))
      return
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      setSelectedIndex(i => Math.max(i - 1, 0))
      return
    }
    if (e.key === 'Enter' && flatFiltered.length > 0) {
      e.preventDefault()
      flatFiltered[clampedIndex]?.action()
    }
  }

  // Scroll selected item into view
  useEffect(() => {
    const el = listRef.current?.querySelector(`[data-index="${clampedIndex}"]`)
    el?.scrollIntoView({ block: 'nearest' })
  }, [clampedIndex])

  // Reset index on query change
  useEffect(() => setSelectedIndex(0), [query])

  let runningIdx = 0

  return (
    <AnimatePresence>
      {open ? (
        <motion.div
          key="command-palette"
          role="dialog"
          aria-modal="true"
          aria-label={spotlight ? 'Zyra Spotlight' : 'Command palette'}
          initial={BACKDROP_INITIAL}
          animate={BACKDROP_ANIMATE}
          exit={BACKDROP_EXIT}
          transition={BACKDROP_TRANSITION}
          className="fixed inset-0 z-[500] liquid-glass-modal-backdrop"
          onClick={close}
        >
          <div className="fixed inset-x-0 top-[12%] mx-auto max-w-2xl px-4" onClick={(e) => e.stopPropagation()}>
            <motion.div
              role="dialog"
              aria-modal
              aria-label="Command palette"
              initial={PANEL_INITIAL}
              animate={PANEL_ANIMATE}
              exit={PANEL_EXIT}
              transition={PANEL_TRANSITION}
              className="liquid-glass-modal-panel overflow-hidden"
              onKeyDown={handleKeyDown}
            >
          {/* Search input */}
          <div className="flex items-center gap-3 px-4 border-b border-white/[0.06]">
            <Search className="w-4 h-4 text-[var(--text-secondary)] shrink-0" strokeWidth={1.75} />
            <input
              ref={inputRef}
              type="text"
              aria-label={spotlight ? 'Zyra — search or ask' : 'Search command palette'}
              value={query}
              onChange={e => setQuery(e.target.value)}
              placeholder={spotlight ? 'Zyra — search or ask…' : 'Search or type > import networks…'}
              className="flex-1 py-3.5 bg-transparent text-sm text-[var(--text-primary)] placeholder-slate-500 focus:outline-none focus-visible:ring-1 focus-visible:ring-[color-mix(in_srgb,var(--accent)_45%,transparent)]"
            />
            {spotlight ? (
              <kbd className="px-1.5 py-0.5 bg-[var(--surface-hover)] border border-[var(--apple-hairline)] rounded text-[10px] font-mono text-[var(--text-muted)] hidden sm:inline">⌘Space</kbd>
            ) : (
              <kbd className="px-1.5 py-0.5 bg-[var(--surface-hover)] border border-[var(--apple-hairline)] rounded text-[10px] font-mono text-[var(--text-muted)]">⌘K</kbd>
            )}
            <kbd className="px-1.5 py-0.5 bg-[var(--surface-hover)] border border-[var(--apple-hairline)] rounded text-[10px] font-mono text-[var(--text-muted)]">Esc</kbd>
          </div>

          {/* Results */}
          <div ref={listRef} className="overflow-y-auto max-h-[60vh] py-1">
            {reviewNlOps ? (
              <div className="p-4 space-y-4">
                <p className="text-xs font-semibold uppercase tracking-wider text-orange-400/80">NL Ops plan (dry-run)</p>
                <p className="text-sm text-[var(--text-primary)] font-medium">{reviewNlOps.summary}</p>
                <p className="text-xs text-[var(--text-muted)]">Risk {reviewNlOps.risk_score}/10 · {reviewNlOps.intent}</p>
                <ul className="text-xs text-[var(--text-muted)] space-y-1">
                  {reviewNlOps.steps.map((s) => (
                    <li key={s.label}>• {s.label} ({s.action_type})</li>
                  ))}
                </ul>
                {reviewNlOps.approval_required && (
                  <button
                    type="button"
                    className="btn-primary text-sm w-full"
                    disabled={executing}
                    onClick={() => void confirmNlOps()}
                  >
                    {executing ? 'Queueing…' : 'Queue for approval — press Enter'}
                  </button>
                )}
              </div>
            ) : reviewCommand ? (
              <div className="p-4 space-y-4">
                <p className="text-xs font-semibold uppercase tracking-wider text-orange-400/80">Review command</p>
                <p className="text-sm text-[var(--text-primary)] font-medium">{reviewCommand.label}</p>
                <p className="text-sm text-[var(--text-muted)]">{reviewCommand.review}</p>
                <button
                  type="button"
                  className="btn-primary text-sm w-full"
                  disabled={executing}
                  onClick={() => void confirmPlatformCommand(reviewCommand)}
                >
                  {executing ? 'Running…' : 'Confirm — press Enter'}
                </button>
              </div>
            ) : loading ? (
              <div className="px-4 py-8 text-center text-sm text-[var(--text-muted)]">Loading...</div>
            ) : flatFiltered.length === 0 ? (
              <div className="px-4 py-8 text-center text-sm text-[var(--text-muted)] space-y-3">
                <p>No results for "{query}"</p>
                {platformConnected && query.trim() && (
                  <button
                    type="button"
                    className="btn-secondary text-sm"
                    onClick={() => { openCopilotWithQuery(query.trim()); close() }}
                  >
                    Ask Zyra "{query.trim()}"
                  </button>
                )}
              </div>
            ) : (
              grouped.map(({ cat, items: groupItems }) => (
                <div key={cat}>
                  <div className="px-4 py-1.5 text-[10px] font-bold text-[var(--text-muted)] uppercase tracking-wider">{cat}</div>
                  {groupItems.map(item => {
                    const idx = runningIdx++
                    const selected = idx === clampedIndex
                    return (
                      <button
                        key={item.id}
                        data-index={idx}
                        onClick={() => item.action()}
                        className={`w-full flex items-center gap-3 px-4 py-2.5 text-sm transition-colors ${
                          selected ? 'bg-[var(--surface-hover)]/60 text-[var(--text-primary)]' : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]/40'
                        }`}
                      >
                        <span className="text-[var(--text-muted)]">{item.icon}</span>
                        <span className="flex-1 text-left truncate">{item.label}</span>
                        {item.badge}
                        {item.sublabel && <span className="text-[10px] text-[var(--text-muted)]">{item.sublabel}</span>}
                        {selected && <ArrowRight className="w-3 h-3 text-[var(--text-muted)]" />}
                      </button>
                    )
                  })}
                </div>
              ))
            )}
          </div>

          {/* Footer */}
          <div className="px-4 py-2 border-t border-white/[0.06] flex items-center gap-4 text-[10px] text-[var(--text-muted)]">
            <span><kbd className="px-1 py-0.5 bg-white/5 border border-white/10 rounded font-mono">↑↓</kbd> navigate</span>
            <span><kbd className="px-1 py-0.5 bg-white/5 border border-white/10 rounded font-mono">↵</kbd> select</span>
            <span><kbd className="px-1 py-0.5 bg-white/5 border border-white/10 rounded font-mono">esc</kbd> close</span>
          </div>
            </motion.div>
          </div>
        </motion.div>
      ) : null}
    </AnimatePresence>
  )
}
