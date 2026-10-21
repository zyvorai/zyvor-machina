// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { BrowserRouter, Routes, Route, useNavigate, useLocation, Navigate, useSearchParams } from 'react-router'
import { MotionConfig } from 'framer-motion'
import { Suspense, useState, useCallback, useMemo, useEffect } from 'react'
import { lazyWithRetry } from './utils/lazyWithRetry'
import AppErrorBoundary from './components/AppErrorBoundary'
import { ToastProvider, ToastRenderer } from './contexts/ToastContext'
import { WebSocketProvider } from './contexts/WebSocketContext'
import { AuthProvider, useAuth } from './contexts/AuthContext'
import { ThemeProvider, useTheme } from './contexts/ThemeContext'
import { PlatformInfoProvider } from './contexts/PlatformInfoContext'
import NotFound from './pages/NotFound'
import LoginPage from './pages/Login'
import CommandPalette from './components/CommandPalette'
import ZyraSpotlight from './components/ai/ZyraSpotlight'
import ZyraAssistant from './components/ai/ZyraAssistant'
import ZyraAmbientBar from './components/ai/ZyraAmbientBar'
import { BreadcrumbNameProvider } from './contexts/BreadcrumbNameContext'
import HelpDialog, { type HelpTab } from './components/HelpDialog'
import { OPEN_HELP_EVENT } from './utils/openHelp'
import PageSkeleton from './components/PageSkeleton'
import { AiProvider } from './contexts/AiContext'
import { useSequenceShortcuts } from './hooks/useSequenceShortcut'
import { safePostLoginPath } from './api/authRedirect'
import { useKeyboardShortcut, isInputFocused } from './hooks/useKeyboardShortcut'
import { useRecordRecentPage } from './hooks/useRecordRecentPage'
import { usePlatformInfo } from './contexts/PlatformInfoContext'
import { routeLabels } from './utils/routes'

const Dashboard = lazyWithRetry(() => import('./pages/Dashboard'))
const VMList = lazyWithRetry(() => import('./pages/VMList'))
const VMDetails = lazyWithRetry(() => import('./pages/VMDetails'))
const CreateVM = lazyWithRetry(() => import('./pages/CreateVM'))
const Sprites = lazyWithRetry(() => import('./pages/Sprites'))
const Networks = lazyWithRetry(() => import('./pages/Networks'))
const Storage = lazyWithRetry(() => import('./pages/Storage'))
const Snapshots = lazyWithRetry(() => import('./pages/Snapshots'))
const NodeInfo = lazyWithRetry(() => import('./pages/NodeInfo'))
const Events = lazyWithRetry(() => import('./pages/Events'))
const ClassicConsoleHub = lazyWithRetry(() => import('./pages/ClassicConsoleHub'))
const ClassicConsoleRedirect = lazyWithRetry(() => import('./pages/ClassicConsoleRedirect'))
const Capabilities = lazyWithRetry(() => import('./pages/Capabilities'))
const Devices = lazyWithRetry(() => import('./pages/Devices'))
const NWFilters = lazyWithRetry(() => import('./pages/NWFilters'))
const Secrets = lazyWithRetry(() => import('./pages/Secrets'))
const Backups = lazyWithRetry(() => import('./pages/Backups'))
const HostNetworking = lazyWithRetry(() => import('./pages/HostNetworking'))
const AuditLog = lazyWithRetry(() => import('./pages/AuditLog'))
const SettingsPage = lazyWithRetry(() => import('./pages/Settings'))
const ImportVM = lazyWithRetry(() => import('./pages/ImportVM'))
const SSHPage = lazyWithRetry(() => import('./pages/SSHPage'))
const HostSSHPage = lazyWithRetry(() => import('./pages/HostSSHPage'))
const ApiDocs = lazyWithRetry(() => import('./pages/ApiDocs'))
const Services = lazyWithRetry(() => import('./pages/Services'))
const Logs = lazyWithRetry(() => import('./pages/Logs'))
const StoragePoolDetail = lazyWithRetry(() => import('./pages/StoragePoolDetail'))
const AdminSessions = lazyWithRetry(() => import('./pages/AdminSessions'))
const DiskImages = lazyWithRetry(() => import('./pages/DiskImages'))
const Jobs = lazyWithRetry(() => import('./pages/Jobs'))
const K8sOverview = lazyWithRetry(() => import('./pages/K8sOverview'))
const K8sWorkloads = lazyWithRetry(() => import('./pages/K8sWorkloads'))
const KataContainers = lazyWithRetry(() => import('./pages/KataContainers'))
const Containers = lazyWithRetry(() => import('./pages/Containers'))
const ContainerPods = lazyWithRetry(() => import('./pages/ContainerPods'))
const FleetCloudOverview = lazyWithRetry(() => import('./pages/FleetCloudOverview'))
const FleetCloudInstances = lazyWithRetry(() => import('./pages/FleetCloudInstances'))
const FleetCloudInstanceDetail = lazyWithRetry(() => import('./pages/FleetCloudInstanceDetail'))
const FleetCloudCreateInstance = lazyWithRetry(() => import('./pages/FleetCloudCreateInstance'))
const FleetCloudImages = lazyWithRetry(() => import('./pages/FleetCloudImages'))
const FleetCloudSecurityGroups = lazyWithRetry(() => import('./pages/FleetCloudSecurityGroups'))
const FleetCloudConsole = lazyWithRetry(() => import('./pages/FleetCloudConsole'))
const FleetCloudVolumes = lazyWithRetry(() => import('./pages/FleetCloudVolumes'))
const FleetCloudVpc = lazyWithRetry(() => import('./pages/FleetCloudVpc'))
const FleetCloudAutopilot = lazyWithRetry(() => import('./pages/FleetCloudAutopilot'))
const FleetCloudChaos = lazyWithRetry(() => import('./pages/FleetCloudChaos'))
const FleetCloudPreemptible = lazyWithRetry(() => import('./pages/FleetCloudPreemptible'))
const FleetCloudNetworking = lazyWithRetry(() => import('./pages/FleetCloudNetworking'))
const FleetCloudKeypairs = lazyWithRetry(() => import('./pages/FleetCloudKeypairs'))
const FleetCloudFlavors = lazyWithRetry(() => import('./pages/FleetCloudFlavors'))
const FleetCloudServerGroups = lazyWithRetry(() => import('./pages/FleetCloudServerGroups'))
const FleetCloudImageDetail = lazyWithRetry(() => import('./pages/FleetCloudImageDetail'))
const FleetCloudVolumeDetail = lazyWithRetry(() => import('./pages/FleetCloudVolumeDetail'))
const FleetCloudFloatingIps = lazyWithRetry(() => import('./pages/FleetCloudFloatingIps'))
const FleetCloudVolumeSnapshots = lazyWithRetry(() => import('./pages/FleetCloudVolumeSnapshots'))
const FleetCloudNetworkDetail = lazyWithRetry(() => import('./pages/FleetCloudNetworkDetail'))
const FleetCloudFlavorDetail = lazyWithRetry(() => import('./pages/FleetCloudFlavorDetail'))
const FleetCloudServerGroupDetail = lazyWithRetry(() => import('./pages/FleetCloudServerGroupDetail'))
const FleetCloudInstanceInterfaces = lazyWithRetry(() => import('./pages/FleetCloudInstanceInterfaces'))
const FleetCloudSecurityGroupDetail = lazyWithRetry(() => import('./pages/FleetCloudSecurityGroupDetail'))
const FleetCloudVolumeSnapshotDetail = lazyWithRetry(() => import('./pages/FleetCloudVolumeSnapshotDetail'))
const FleetCloudHeat = lazyWithRetry(() => import('./pages/FleetCloudHeat'))
const FleetCloudHeatDetail = lazyWithRetry(() => import('./pages/FleetCloudHeatDetail'))
const FleetCloudLoadBalancers = lazyWithRetry(() => import('./pages/FleetCloudLoadBalancers'))
const FleetCloudLoadBalancerDetail = lazyWithRetry(() => import('./pages/FleetCloudLoadBalancerDetail'))
const FleetCloudIdentity = lazyWithRetry(() => import('./pages/FleetCloudIdentity'))
const FleetCloudIdentityProjectDetail = lazyWithRetry(() => import('./pages/FleetCloudIdentityProjectDetail'))
const FleetCloudTopology = lazyWithRetry(() => import('./pages/FleetCloudTopology'))
const Fleet = lazyWithRetry(() => import('./pages/Fleet'))
const PlatformLayout = lazyWithRetry(() => import('./layouts/PlatformLayout'))
const PlatformDashboard = lazyWithRetry(() => import('./pages/platform/PlatformDashboard'))
const PlatformHosts = lazyWithRetry(() => import('./pages/platform/PlatformHosts'))
const PlatformMachineFinder = lazyWithRetry(() => import('./pages/platform/PlatformMachineFinder'))
const PlatformGpuCommandCenter = lazyWithRetry(() => import('./pages/platform/PlatformGpuCommandCenter'))
const PlatformVms = lazyWithRetry(() => import('./pages/platform/PlatformVms'))
const PlatformVmDetail = lazyWithRetry(() => import('./pages/platform/PlatformVmDetail'))
const PlatformConsoleHub = lazyWithRetry(() => import('./pages/platform/PlatformConsoleHub'))
const MissionControlLiveWall = lazyWithRetry(() => import('./pages/platform/MissionControlLiveWall'))
const PlatformConsoleRedirect = lazyWithRetry(() => import('./pages/platform/PlatformConsoleRedirect'))
const PlatformContent = lazyWithRetry(() => import('./pages/platform/PlatformContent'))
const PlatformIsoCreate = lazyWithRetry(() => import('./pages/platform/PlatformIsoCreate'))
const PlatformVirtInstallCreate = lazyWithRetry(() => import('./pages/platform/PlatformVirtInstallCreate'))
const PlatformVmBuilder = lazyWithRetry(() => import('./pages/platform/PlatformVmBuilder'))
const PlatformTemplates = lazyWithRetry(() => import('./pages/platform/PlatformTemplates'))
const CloudInitStudio = lazyWithRetry(() => import('./pages/platform/CloudInitStudio'))
const PlatformNetworkCanvas = lazyWithRetry(() => import('./pages/platform/PlatformNetworkCanvas'))
const PlatformDatacenter = lazyWithRetry(() => import('./pages/platform/PlatformDatacenter'))
const PlatformFleetSnapshots = lazyWithRetry(() => import('./pages/platform/PlatformFleetSnapshots'))
const PlatformFailureDomains = lazyWithRetry(() => import('./pages/platform/PlatformFailureDomains'))
const PlatformK8sClusters = lazyWithRetry(() => import('./pages/platform/PlatformK8sClusters'))
const PlatformDesktopPools = lazyWithRetry(() => import('./pages/platform/PlatformDesktopPools'))
const PlatformMigrationWaves = lazyWithRetry(() => import('./pages/platform/PlatformMigrationWaves'))
const PlatformBackupReplication = lazyWithRetry(() => import('./pages/platform/PlatformBackupReplication'))
const PlatformAlertRules = lazyWithRetry(() => import('./pages/platform/PlatformAlertRules'))
const PlatformScheduledJobs = lazyWithRetry(() => import('./pages/platform/PlatformScheduledJobs'))
const PlatformEnroll = lazyWithRetry(() => import('./pages/platform/PlatformEnroll'))
const PlatformPlacement = lazyWithRetry(() => import('./pages/platform/PlatformPlacement'))
const PlatformTasks = lazyWithRetry(() => import('./pages/platform/PlatformTasks'))
const PlatformEvents = lazyWithRetry(() => import('./pages/platform/PlatformEvents'))
const PlatformStorage = lazyWithRetry(() => import('./pages/platform/PlatformStorage'))
const PlatformAtlasStorage = lazyWithRetry(() => import('./pages/platform/PlatformAtlasStorage'))
const PlatformNetworks = lazyWithRetry(() => import('./pages/platform/PlatformNetworks'))
const PlatformHostDetail = lazyWithRetry(() => import('./pages/platform/PlatformHostDetail'))
const PlatformUsers = lazyWithRetry(() => import('./pages/platform/PlatformUsers'))
const PlatformWebhooks = lazyWithRetry(() => import('./pages/platform/PlatformWebhooks'))
const PlatformReports = lazyWithRetry(() => import('./pages/platform/PlatformReports'))
const PlatformApiKeys = lazyWithRetry(() => import('./pages/platform/PlatformApiKeys'))
const PlatformMaintenance = lazyWithRetry(() => import('./pages/platform/PlatformMaintenance'))
const PlatformProjects = lazyWithRetry(() => import('./pages/platform/PlatformProjects'))
const PlatformNotifications = lazyWithRetry(() => import('./pages/platform/PlatformNotifications'))
const PlatformResourcesHub = lazyWithRetry(() => import('./pages/platform/PlatformResourcesHub'))
const PlatformInfrastructureHub = lazyWithRetry(() => import('./pages/platform/PlatformInfrastructureHub'))
const PlatformWorkloadsHub = lazyWithRetry(() => import('./pages/platform/PlatformWorkloadsHub'))
const PlatformAdministrationHub = lazyWithRetry(() => import('./pages/platform/PlatformAdministrationHub'))
const PlatformOperationsHub = lazyWithRetry(() => import('./pages/platform/PlatformOperationsHub'))
const PlatformSettingsHub = lazyWithRetry(() => import('./pages/platform/PlatformSettingsHub'))
const PlatformNotFound = lazyWithRetry(() => import('./pages/platform/PlatformNotFound'))
const PlatformMigration = lazyWithRetry(() => import('./pages/platform/PlatformMigration'))
const PlatformActivityMonitor = lazyWithRetry(() => import('./pages/platform/PlatformActivityMonitor'))
const PlatformRecommendations = lazyWithRetry(() => import('./pages/platform/PlatformRecommendations'))
const PlatformApplications = lazyWithRetry(() => import('./pages/platform/PlatformApplications'))
const PlatformBackups = lazyWithRetry(() => import('./pages/platform/PlatformBackups'))
const PlatformTopology = lazyWithRetry(() => import('./pages/platform/PlatformTopology'))
const PlatformZyraOs = lazyWithRetry(() => import('./pages/platform/PlatformZyraOs'))
const PlatformZyraSettings = lazyWithRetry(() => import('./pages/platform/PlatformZyraSettings'))
const PlatformAiProviders = lazyWithRetry(() => import('./pages/platform/PlatformAiProviders'))
const PlatformHa = lazyWithRetry(() => import('./pages/platform/PlatformHa'))
const PlatformBareMetal = lazyWithRetry(() => import('./pages/platform/PlatformBareMetal'))
const PlatformStorageTiers = lazyWithRetry(() => import('./pages/platform/PlatformStorageTiers'))
const PlatformMarketplace = lazyWithRetry(() => import('./pages/platform/PlatformMarketplace'))
const PlatformUpgrade = lazyWithRetry(() => import('./pages/platform/PlatformUpgrade'))
const PlatformRightsizing = lazyWithRetry(() => import('./pages/platform/PlatformRightsizing'))
const PlatformIncidentCommander = lazyWithRetry(() => import('./pages/platform/PlatformIncidentCommander'))
const PlatformZyraApprovals = lazyWithRetry(() => import('./pages/platform/PlatformZyraApprovals'))
const PlatformSecurityCenter = lazyWithRetry(() => import('./pages/platform/PlatformSecurityCenter'))
const PlatformSoc = lazyWithRetry(() => import('./pages/platform/PlatformSoc'))
const PlatformMachineSecurity = lazyWithRetry(() => import('./pages/platform/PlatformMachineSecurity'))
const PlatformThreatHunting = lazyWithRetry(() => import('./pages/platform/PlatformThreatHunting'))
const PlatformRuntimeEnforcement = lazyWithRetry(() => import('./pages/platform/PlatformRuntimeEnforcement'))
const PlatformNativeBpf = lazyWithRetry(() => import('./pages/platform/PlatformNativeBpf'))
const PlatformVmNetworkPolicies = lazyWithRetry(() => import('./pages/platform/PlatformVmNetworkPolicies'))
const PlatformFirewallOverview = lazyWithRetry(() => import('./pages/platform/security/PlatformFirewallOverview'))
const PlatformFirewallTargetDetail = lazyWithRetry(() => import('./pages/platform/security/PlatformFirewallTargetDetail'))
const PlatformFirewallPorts = lazyWithRetry(() => import('./pages/platform/security/PlatformFirewallPorts'))
const PlatformFirewallServices = lazyWithRetry(() => import('./pages/platform/security/PlatformFirewallServices'))
const PlatformFirewallActivity = lazyWithRetry(() => import('./pages/platform/security/PlatformFirewallActivity'))
const PlatformFirewallCompliance = lazyWithRetry(() => import('./pages/platform/security/PlatformFirewallCompliance'))
const PlatformFirewallK8s = lazyWithRetry(() => import('./pages/platform/security/PlatformFirewallK8s'))
const PlatformFirewallCloud = lazyWithRetry(() => import('./pages/platform/security/PlatformFirewallCloud'))
const PlatformFirewallConnectivity = lazyWithRetry(() => import('./pages/platform/security/PlatformFirewallConnectivity'))
const PlatformFirewallPolicies = lazyWithRetry(() => import('./pages/platform/security/PlatformFirewallPolicies'))
const PlatformPolicy = lazyWithRetry(() => import('./pages/platform/PlatformPolicy'))
const PlatformBlueprints = lazyWithRetry(() => import('./pages/platform/PlatformBlueprints'))
const PlatformLaunchpad = lazyWithRetry(() => import('./pages/platform/PlatformLaunchpad'))
const PlatformSupport = lazyWithRetry(() => import('./pages/platform/PlatformSupport'))
const PlatformDeveloper = lazyWithRetry(() => import('./pages/platform/PlatformDeveloper'))
const PlatformObservability = lazyWithRetry(() => import('./pages/platform/PlatformObservability'))
const PlatformEnterprise = lazyWithRetry(() => import('./pages/platform/PlatformEnterprise'))
const RdpConsole = lazyWithRetry(() => import('./pages/RdpConsole'))
const MissionControl = lazyWithRetry(() => import('./pages/MissionControl'))
const SystemCheck = lazyWithRetry(() => import('./pages/SystemCheck'))

function GlobalShortcuts({
  helpOpen,
  helpTab,
  onOpenHelp,
  onCloseHelp,
  onHelpTabChange,
}: {
  helpOpen: boolean
  helpTab: HelpTab
  onOpenHelp: (tab?: HelpTab) => void
  onCloseHelp: () => void
  onHelpTabChange: (tab: HelpTab) => void
}) {
  const navigate = useNavigate()
  const location = useLocation()
  const shortcuts = useMemo(() => {
    const base: { sequence: [string, string]; handler: () => void }[] = [
      { sequence: ['g', 'd'], handler: () => navigate('/') },
      { sequence: ['g', 'v'], handler: () => navigate('/vms') },
      { sequence: ['g', 'n'], handler: () => navigate('/networks') },
      { sequence: ['g', 's'], handler: () => navigate('/storage') },
      { sequence: ['g', 'c'], handler: () => navigate('/create') },
      { sequence: ['g', 'e'], handler: () => navigate('/events') },
      { sequence: ['g', 'j'], handler: () => navigate('/jobs') },
      { sequence: ['g', 'b'], handler: () => navigate('/backups') },
      { sequence: ['g', 'i'], handler: () => navigate('/disk-images') },
      { sequence: ['g', 'k'], handler: () => navigate('/k8s/workloads') },
      { sequence: ['g', 'o'], handler: () => navigate('/fleet-cloud') },
    ]
    return base
  }, [navigate])

  useSequenceShortcuts(shortcuts)

  const toggleHelp = useCallback(
    (e: KeyboardEvent) => {
      if (isInputFocused()) return
      e.preventDefault()
      if (helpOpen) onCloseHelp()
      else onOpenHelp('shortcuts')
    },
    [helpOpen, onCloseHelp, onOpenHelp],
  )

  useKeyboardShortcut({ key: '?', handler: toggleHelp })
  useEffect(() => {
    const onOpen = (e: Event) => {
      const tab = (e as CustomEvent<{ tab?: HelpTab }>).detail?.tab ?? 'shortcuts'
      onOpenHelp(tab)
    }
    window.addEventListener(OPEN_HELP_EVENT, onOpen)
    return () => window.removeEventListener(OPEN_HELP_EVENT, onOpen)
  }, [onOpenHelp])
  useKeyboardShortcut({
    key: 'F3',
    handler: (e) => {
      if (isInputFocused()) return
      e.preventDefault()
      if (location.pathname.startsWith('/platform')) {
        window.dispatchEvent(new CustomEvent('machina-open-mission-control'))
      } else {
        navigate('/platform?mission=1')
      }
    },
  })

  return (
    <HelpDialog open={helpOpen} tab={helpTab} onClose={onCloseHelp} onTabChange={onHelpTabChange} />
  )
}

function AuthenticatedApp() {
  const { isAuthenticated, loading } = useAuth()

  if (loading) {
    return (
      <div className="min-h-screen dashboard-liquid-glass liquid-glass-app flex items-center justify-center">
        <div className="glass glass-elevated p-8 flex items-center justify-center">
          <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-[var(--accent)]" />
        </div>
      </div>
    )
  }

  if (!isAuthenticated) {
    return <LoginPage />
  }

  return <AuthenticatedShell />
}

function RouteRecorder() {
  useRecordRecentPage()
  return null
}

/** When the platform control plane is active, land on the macOS desktop (dock + menubar). */
function HomeRoute() {
  const { info, loading } = usePlatformInfo()
  if (!loading && info?.control_plane?.proxy_url) {
    return <Navigate to="/platform" replace />
  }
  return <Dashboard />
}

/** Authenticated visit to `/login?next=…` must honor `next` (not drop to `/` → Mission Control). */
function PostLoginRedirect() {
  const [params] = useSearchParams()
  const dest = safePostLoginPath(params.get('next'), '/')
  return <Navigate to={dest} replace />
}

function AuthenticatedShell() {
  return (
    <WebSocketProvider>
      <PlatformInfoProvider>
        <BrowserRouter>
          <BreadcrumbNameProvider>
            <AiProvider>
              <AuthenticatedShellRoutes />
            </AiProvider>
          </BreadcrumbNameProvider>
        </BrowserRouter>
      </PlatformInfoProvider>
    </WebSocketProvider>
  )
}

function AuthenticatedShellRoutes() {
  const { theme } = useTheme()
  const location = useLocation()
  const [helpOpen, setHelpOpen] = useState(false)
  const [helpTab, setHelpTab] = useState<HelpTab>('shortcuts')

  useEffect(() => {
    const label = routeLabels[location.pathname] ?? routeLabels[location.pathname.replace(/\/[^/]+$/, '/:id')]
    document.title = label ? `${label} — Machina` : 'Machina'
  }, [location.pathname])

  const openHelp = useCallback((tab: HelpTab = 'shortcuts') => {
    setHelpTab(tab)
    setHelpOpen(true)
  }, [])

  const closeHelp = useCallback(() => setHelpOpen(false), [])

  useEffect(() => {
    setHelpOpen(false)
  }, [location.pathname, location.search])

  // Reset scroll to the top on navigation between pages (but honor in-page #hash
  // anchors). Without this, opening a detail page lands mid-page at the previous
  // list's scroll offset.
  useEffect(() => {
    if (location.hash) return
    window.scrollTo(0, 0)
    document.getElementById('main-content')?.scrollTo?.(0, 0)
  }, [location.pathname])

  const shellClass =
    theme === 'light'
      ? 'apple-light-shell min-h-screen flex flex-col text-[var(--text-primary)]'
      : 'dashboard-liquid-glass liquid-glass-app min-h-screen flex flex-col text-[var(--text-primary)]'

  return (
    <>
          <RouteRecorder />
          <ToastRenderer />
          <div className={`${shellClass} flex flex-col min-h-dvh`}>
            <a
              href="#main-content"
              className="sr-only focus:not-sr-only focus:absolute focus:top-4 focus:left-4 focus:z-[9999] focus:px-4 focus:py-2 focus:bg-[var(--apple-surface)] focus:text-[var(--text-primary)] focus:rounded-lg focus:shadow-lg focus:outline-none focus:ring-2 focus:ring-[var(--accent)]"
            >
              Skip to main content
            </a>
            <ZyraSpotlight onOpenHelp={openHelp} />
            <ZyraAssistant />
            <ZyraAmbientBar />
            <GlobalShortcuts
              helpOpen={helpOpen}
              helpTab={helpTab}
              onOpenHelp={openHelp}
              onCloseHelp={closeHelp}
              onHelpTabChange={setHelpTab}
            />
            <main
              id="main-content"
              className="platform-route-main flex-1 min-w-0 flex flex-col w-full"
            >
              <AppErrorBoundary resetKey={location.pathname}>
              <Suspense fallback={<PageSkeleton />}>
                <Routes>
                <Route path="/login" element={<PostLoginRedirect />} />
                <Route element={<PlatformLayout />}>
                <Route path="/" element={<HomeRoute />} />
                <Route path="/vms" element={<VMList />} />
                <Route path="/vms/:name" element={<VMDetails />} />
                <Route path="/vms/:name/console" element={<ClassicConsoleRedirect />} />
                <Route path="/vms/:name/consolehub" element={<ClassicConsoleHub />} />
                <Route path="/vms/:name/rdp" element={<RdpConsole />} />
                <Route path="/fleet" element={<Fleet />} />
                <Route path="/platform">
                  <Route index element={<PlatformDashboard />} />
                  <Route path="hosts" element={<PlatformHosts />} />
                  <Route path="hosts/finder" element={<PlatformMachineFinder />} />
                  <Route path="hosts/:id" element={<PlatformHostDetail />} />
                  <Route path="vms" element={<PlatformVms />} />
                  <Route path="vms/:id" element={<PlatformVmDetail />} />
                  <Route path="vms/:id/consolehub" element={<PlatformConsoleHub />} />
                  <Route path="mission-control/live" element={<MissionControlLiveWall />} />
                  <Route path="vms/:id/console" element={<PlatformConsoleRedirect />} />
                  <Route path="applications" element={<PlatformApplications />} />
                  <Route path="content" element={<PlatformContent />} />
                  <Route path="create-iso" element={<PlatformIsoCreate />} />
                  <Route path="create-advanced" element={<PlatformVirtInstallCreate />} />
                  <Route path="vm-builder" element={<PlatformVmBuilder />} />
                  <Route path="templates" element={<PlatformTemplates />} />
                  <Route path="cloud-init" element={<CloudInitStudio />} />
                  <Route path="network-canvas" element={<PlatformNetworkCanvas />} />
                  <Route path="datacenter" element={<PlatformDatacenter />} />
                  <Route path="migration" element={<PlatformMigration />} />
                  <Route path="backups" element={<PlatformBackups />} />
                  <Route path="fleet-snapshots" element={<PlatformFleetSnapshots />} />
                  <Route path="failure-domains" element={<PlatformFailureDomains />} />
                  <Route path="k8s-clusters" element={<PlatformK8sClusters />} />
                  <Route path="desktop-pools" element={<PlatformDesktopPools />} />
                  <Route path="migration-waves" element={<PlatformMigrationWaves />} />
                  <Route path="backup-replication" element={<PlatformBackupReplication />} />
                  <Route path="alert-rules" element={<PlatformAlertRules />} />
                  <Route path="scheduled-jobs" element={<PlatformScheduledJobs />} />
                  <Route path="enroll" element={<PlatformEnroll />} />
                  <Route path="placement" element={<PlatformPlacement />} />
                  <Route path="tasks" element={<PlatformTasks />} />
                  <Route path="events" element={<PlatformEvents />} />
                  <Route path="activity" element={<PlatformActivityMonitor />} />
                  <Route path="recommendations" element={<PlatformRecommendations />} />
                  <Route path="topology" element={<PlatformTopology />} />
                  <Route path="developer" element={<PlatformDeveloper />} />
                  <Route path="observability" element={<PlatformObservability />} />
                  <Route path="enterprise" element={<PlatformEnterprise />} />
                  <Route path="zyra" element={<PlatformZyraOs />} />
                  <Route path="zyra/configure" element={<PlatformZyraSettings />} />
                  <Route path="ai-providers" element={<PlatformAiProviders />} />
                  <Route path="ha" element={<PlatformHa />} />
                  <Route path="baremetal" element={<PlatformBareMetal />} />
                  <Route path="storage-tiers" element={<PlatformStorageTiers />} />
                  <Route path="marketplace" element={<PlatformMarketplace />} />
                  <Route path="upgrade" element={<PlatformUpgrade />} />
                  <Route path="zyra/rightsizing" element={<PlatformRightsizing />} />
                  <Route path="zyra/incidents" element={<PlatformIncidentCommander />} />
                  <Route path="zyra/approvals" element={<PlatformZyraApprovals />} />
                  <Route path="zyra/security/hunt" element={<PlatformThreatHunting />} />
                  <Route path="zyra/security/enforcement" element={<PlatformRuntimeEnforcement />} />
                  <Route path="zyra/security/native-bpf" element={<PlatformNativeBpf />} />
                  <Route path="zyra/security/network-policies" element={<PlatformVmNetworkPolicies />} />
                  <Route path="soc" element={<PlatformSoc />} />
                  <Route path="zeus/security" element={<PlatformSecurityCenter />} />
                  <Route path="zyra/machines/:hostId" element={<PlatformMachineSecurity />} />
                  <Route path="zeus/security/firewall" element={<PlatformFirewallOverview />} />
                  <Route path="zeus/security/firewall/:id" element={<PlatformFirewallTargetDetail />} />
                  <Route path="zeus/security/ports" element={<PlatformFirewallPorts />} />
                  <Route path="zeus/security/services" element={<PlatformFirewallServices />} />
                  <Route path="zeus/security/activity" element={<PlatformFirewallActivity />} />
                  <Route path="zeus/security/compliance" element={<PlatformFirewallCompliance />} />
                  <Route path="zeus/security/k8s" element={<PlatformFirewallK8s />} />
                  <Route path="zeus/security/cloud" element={<PlatformFirewallCloud />} />
                  <Route path="zeus/security/connectivity" element={<PlatformFirewallConnectivity />} />
                  <Route path="zeus/security/policies" element={<PlatformFirewallPolicies />} />
                  <Route path="policy" element={<PlatformPolicy />} />
                  <Route path="integrations" element={<Navigate to="/platform/settings?section=integrations" replace />} />
                  <Route path="alerts" element={<Navigate to="/platform/notifications" replace />} />
                  <Route path="infrastructure" element={<PlatformInfrastructureHub />} />
                  <Route path="workloads" element={<PlatformWorkloadsHub />} />
                  <Route path="administration" element={<PlatformAdministrationHub />} />
                  <Route path="resources" element={<PlatformResourcesHub />} />
                  <Route path="operations" element={<PlatformOperationsHub />} />
                  <Route path="blueprints" element={<PlatformBlueprints />} />
                  <Route path="launchpad" element={<PlatformLaunchpad />} />
                  <Route path="support" element={<PlatformSupport />} />
                  <Route path="storage" element={<PlatformStorage />} />
                  <Route path="storage-atlas" element={<PlatformAtlasStorage />} />
                  <Route path="gpu" element={<PlatformGpuCommandCenter />} />
                  <Route path="networks" element={<PlatformNetworks />} />
                  <Route path="users" element={<PlatformUsers />} />
                  <Route path="webhooks" element={<PlatformWebhooks />} />
                  <Route path="reports" element={<PlatformReports />} />
                  <Route path="api-keys" element={<PlatformApiKeys />} />
                  <Route path="maintenance" element={<PlatformMaintenance />} />
                  <Route path="projects" element={<PlatformProjects />} />
                  <Route path="notifications" element={<PlatformNotifications />} />
                  <Route path="settings" element={<PlatformSettingsHub />} />
                  <Route path="*" element={<PlatformNotFound />} />
                </Route>
                <Route path="/create" element={<CreateVM />} />
                <Route path="/jobs/:jobId" element={<Jobs />} />
                <Route path="/jobs" element={<Jobs />} />
                <Route path="/k8s" element={<K8sOverview />} />
                <Route path="/k8s/workloads" element={<K8sWorkloads />} />
                <Route path="/k8s/kata" element={<KataContainers />} />
                <Route path="/containers" element={<Containers />} />
                <Route path="/containers/pods" element={<ContainerPods />} />
                <Route path="/fleet-cloud" element={<FleetCloudOverview />} />
                <Route path="/fleet-cloud/instances" element={<FleetCloudInstances />} />
                <Route path="/fleet-cloud/instances/:id" element={<FleetCloudInstanceDetail />} />
                <Route path="/fleet-cloud/instances/:id/interfaces" element={<FleetCloudInstanceInterfaces />} />
                <Route path="/fleet-cloud/create" element={<FleetCloudCreateInstance />} />
                <Route path="/fleet-cloud/images" element={<FleetCloudImages />} />
                <Route path="/fleet-cloud/images/:id" element={<FleetCloudImageDetail />} />
                <Route path="/fleet-cloud/security-groups" element={<FleetCloudSecurityGroups />} />
                <Route path="/fleet-cloud/security-groups/:id" element={<FleetCloudSecurityGroupDetail />} />
                <Route path="/fleet-cloud/volumes" element={<FleetCloudVolumes />} />
                <Route path="/fleet-cloud/volumes/:id" element={<FleetCloudVolumeDetail />} />
                <Route path="/fleet-cloud/floating-ips" element={<FleetCloudFloatingIps />} />
                <Route path="/fleet-cloud/vpcs" element={<FleetCloudVpc />} />
                <Route path="/fleet-cloud/autopilot" element={<FleetCloudAutopilot />} />
                <Route path="/fleet-cloud/chaos" element={<FleetCloudChaos />} />
                <Route path="/fleet-cloud/preemptible" element={<FleetCloudPreemptible />} />
                <Route path="/fleet-cloud/networking" element={<FleetCloudNetworking />} />
                <Route path="/fleet-cloud/keypairs" element={<FleetCloudKeypairs />} />
                <Route path="/fleet-cloud/flavors" element={<FleetCloudFlavors />} />
                <Route path="/fleet-cloud/flavors/:id" element={<FleetCloudFlavorDetail />} />
                <Route path="/fleet-cloud/server-groups" element={<FleetCloudServerGroups />} />
                <Route path="/fleet-cloud/server-groups/:id" element={<FleetCloudServerGroupDetail />} />
                <Route path="/fleet-cloud/networks/:id" element={<FleetCloudNetworkDetail />} />
                <Route path="/fleet-cloud/volume-snapshots" element={<FleetCloudVolumeSnapshots />} />
                <Route path="/fleet-cloud/volume-snapshots/:id" element={<FleetCloudVolumeSnapshotDetail />} />
                <Route path="/fleet-cloud/heat" element={<FleetCloudHeat />} />
                <Route path="/fleet-cloud/heat/:name/:id" element={<FleetCloudHeatDetail />} />
                <Route path="/fleet-cloud/load-balancers" element={<FleetCloudLoadBalancers />} />
                <Route path="/fleet-cloud/load-balancers/:id" element={<FleetCloudLoadBalancerDetail />} />
                <Route path="/fleet-cloud/identity" element={<FleetCloudIdentity />} />
                <Route path="/fleet-cloud/identity/projects/:id" element={<FleetCloudIdentityProjectDetail />} />
                <Route path="/fleet-cloud/topology" element={<FleetCloudTopology />} />
                <Route path="/fleet-cloud/instances/:id/console" element={<FleetCloudConsole />} />
                <Route path="/networks" element={<Networks />} />
                <Route path="/storage" element={<Storage />} />
                <Route path="/storage/:pool" element={<StoragePoolDetail />} />
                <Route path="/disk-images" element={<DiskImages />} />
                <Route path="/snapshots" element={<Snapshots />} />
                <Route path="/node" element={<NodeInfo />} />
                <Route path="/events" element={<Events />} />
                <Route path="/capabilities" element={<Capabilities />} />
                <Route path="/devices" element={<Devices />} />
                <Route path="/nwfilters" element={<NWFilters />} />
                <Route path="/secrets" element={<Secrets />} />
                <Route path="/backups" element={<Backups />} />
                <Route path="/host-networking" element={<HostNetworking />} />
                <Route path="/host-ssh" element={<HostSSHPage />} />
                <Route path="/audit" element={<AuditLog />} />
                <Route path="/import" element={<ImportVM />} />
                <Route path="/sprites" element={<Sprites />} />
                <Route path="/ssh/:host" element={<SSHPage />} />
                <Route path="/ssh" element={<SSHPage />} />
                <Route path="/api-docs" element={<ApiDocs />} />
                <Route path="/services" element={<Services />} />
                <Route path="/mission-control" element={<MissionControl />} />
                <Route path="/system-check" element={<SystemCheck />} />
                <Route path="/logs" element={<Logs />} />
                <Route path="/settings" element={<SettingsPage />} />
                <Route path="/admin/sessions" element={<AdminSessions />} />
                <Route path="*" element={<NotFound />} />
                </Route>
              </Routes>
            </Suspense>
            </AppErrorBoundary>
          </main>
        </div>
    </>
  )
}

function App() {
  return (
    <MotionConfig reducedMotion="user">
      <ThemeProvider>
        <ToastProvider>
          <AuthProvider>
            <AuthenticatedApp />
          </AuthProvider>
        </ToastProvider>
      </ThemeProvider>
    </MotionConfig>
  )
}

export default App
