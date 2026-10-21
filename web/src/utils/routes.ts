// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React from 'react'
import type { LucideIcon } from 'lucide-react'
import {
  Home, Server, Plus, Upload, Network, HardDrive, Camera, Shield, Archive, Globe,
  Cpu, Activity, MonitorCog, Usb, Cog, ScrollText, FileText, Key, Users, Database, Terminal,
  ClipboardList,
  Boxes,
  Package,
  Box,
  Cloud,
  Stethoscope,
  Layers,
  Webhook,
  Bell,
  Settings,
  BookOpen,
  ShieldCheck,
  Radar,
  BarChart2,
  Microchip,
  Code2,
  GitBranch,
  CloudCog,
  Zap,
  Moon,
  AlertTriangle,
  CheckCircle2,
  Share2,
  TrendingDown,
  Flame,
  Lock,
  BrainCircuit,
  Building2,
  ArrowUpCircle,
  Wrench,
  Hammer,
  SlidersHorizontal,
  Copy,
  FolderKanban,
  Container,
  ServerCog,
  PlugZap,
  Sparkles,
  Scale,
  Waypoints,
  Rocket,
  LifeBuoy,
  CalendarClock,
  BellRing,
  Search,
} from 'lucide-react'

interface NavItem {
  to: string
  icon: React.ReactNode
  label: string
  /** If true, only show in nav when signed in as UNIX `root`. */
  requiresRoot?: boolean
}

/** Match nav item href against current location (supports /fleet-cloud prefix + settings query). */
export function navItemActive(
  item: NavItem,
  pathname: string,
  search: string,
): boolean {
  const [path, query] = item.to.split('?')
  if (query) {
    if (pathname !== path) return false
    const params = new URLSearchParams(query)
    for (const [k, v] of params.entries()) {
      if (new URLSearchParams(search).get(k) !== v) return false
    }
    return true
  }
  if (path === '/fleet-cloud') {
    return pathname === '/fleet-cloud'
  }
  if (path.startsWith('/fleet-cloud/')) {
    return pathname === path || pathname.startsWith(`${path}/`)
  }
  if (path.startsWith('/platform/zyra/')) {
    return pathname === path || pathname.startsWith(`${path}/`)
  }
  return pathname === path
}

export function navItemVisible(item: NavItem, username: string): boolean {
  if (item.requiresRoot && username !== 'root') return false
  return true
}

interface NavSection {
  label: string
  items: NavItem[]
}

interface NavGroup {
  /** Full name (tooltips, mobile drawer, aria). */
  label: string
  /** Top-bar icon trigger (v9s-style icon cluster). */
  barIcon: LucideIcon
  items: NavItem[]
  /** When set, desktop nav renders a sectioned dropdown instead of a flat list. */
  sections?: NavSection[]
  /** Tall menus scroll inside the panel (long groups). */
  menuScroll?: boolean
}

/** Sections for dropdown / mobile drawer (single section when `items` only). */
function navDropdownSections(group: NavGroup): NavSection[] {
  if (group.sections?.length) return group.sections
  if (group.items.length) return [{ label: '', items: group.items }]
  return []
}

/** Flatten group items (sections or top-level items). */
export function navGroupItems(group: NavGroup): NavItem[] {
  return navDropdownSections(group).flatMap((s) => s.items)
}

/** Always-visible toolbar shortcuts beside the main nav icons. */
export const TOP_BAR_QUICK_LINKS: NavItem[] = [
  { to: '/settings', icon: React.createElement(Settings, { className: 'w-4 h-4' }), label: 'Settings' },
  { to: '/host-ssh', icon: React.createElement(Terminal, { className: 'w-4 h-4' }), label: 'Host SSH' },
  { to: '/api-docs', icon: React.createElement(BookOpen, { className: 'w-4 h-4' }), label: 'API Docs' },
]

export const navGroups: NavGroup[] = [
  {
    label: 'Core',
    barIcon: Home,
    items: [
      { to: '/', icon: React.createElement(Home, { className: 'w-4 h-4' }), label: 'Dashboard' },
      { to: '/vms', icon: React.createElement(Server, { className: 'w-4 h-4' }), label: 'Virtual Machines' },
      { to: '/create', icon: React.createElement(Plus, { className: 'w-4 h-4' }), label: 'Create VM' },
      { to: '/import', icon: React.createElement(Upload, { className: 'w-4 h-4' }), label: 'Import VM' },
      { to: '/sprites', icon: React.createElement(Zap, { className: 'w-4 h-4' }), label: 'Sprites' },
      { to: '/fleet', icon: React.createElement(Globe, { className: 'w-4 h-4' }), label: 'Fleet' },
    ],
  },
  {
    label: 'Platform',
    barIcon: Cloud,
    items: [],
    menuScroll: true,
    sections: [
      {
        label: 'Overview',
        items: [
          { to: '/platform', icon: React.createElement(Cloud, { className: 'w-4 h-4' }), label: 'Mission Control' },
          { to: '/platform/vms', icon: React.createElement(MonitorCog, { className: 'w-4 h-4' }), label: 'Virtual Machines' },
          { to: '/platform/hosts', icon: React.createElement(Server, { className: 'w-4 h-4' }), label: 'Hosts' },
          { to: '/platform/applications', icon: React.createElement(Boxes, { className: 'w-4 h-4' }), label: 'Applications' },
          { to: '/platform/datacenter', icon: React.createElement(Building2, { className: 'w-4 h-4' }), label: 'Datacenter View' },
          { to: '/platform/hosts/finder', icon: React.createElement(Search, { className: 'w-4 h-4' }), label: 'Machine Finder' },
          { to: '/platform/launchpad', icon: React.createElement(Rocket, { className: 'w-4 h-4' }), label: 'Launchpad' },
        ],
      },
      {
        label: 'Resources',
        items: [
          { to: '/platform/storage', icon: React.createElement(HardDrive, { className: 'w-4 h-4' }), label: 'Disk Utility' },
          { to: '/platform/storage-atlas', icon: React.createElement(Database, { className: 'w-4 h-4' }), label: 'Storage (Atlas)' },
          { to: '/platform/storage-tiers', icon: React.createElement(Layers, { className: 'w-4 h-4' }), label: 'Storage Tiers' },
          { to: '/platform/vgpu', icon: React.createElement(Cpu, { className: 'w-4 h-4' }), label: 'vGPU Partitions' },
          { to: '/platform/networks', icon: React.createElement(Network, { className: 'w-4 h-4' }), label: 'Networks' },
          { to: '/platform/content', icon: React.createElement(Package, { className: 'w-4 h-4' }), label: 'Images & ISOs' },
          { to: '/platform/templates', icon: React.createElement(Copy, { className: 'w-4 h-4' }), label: 'Templates' },
          { to: '/platform/cloud-init', icon: React.createElement(CloudCog, { className: 'w-4 h-4' }), label: 'Cloud-Init Studio' },
          { to: '/platform/create-iso', icon: React.createElement(Archive, { className: 'w-4 h-4' }), label: 'Create ISO' },
          { to: '/platform/baremetal', icon: React.createElement(ServerCog, { className: 'w-4 h-4' }), label: 'Bare Metal' },
        ],
      },
      {
        label: 'Workloads',
        items: [
          { to: '/platform/migration', icon: React.createElement(Upload, { className: 'w-4 h-4' }), label: 'Migration Assistant' },
          { to: '/platform/vm-builder', icon: React.createElement(Hammer, { className: 'w-4 h-4' }), label: 'VM Builder' },
          { to: '/platform/create-advanced', icon: React.createElement(Terminal, { className: 'w-4 h-4' }), label: 'Advanced Create' },
          { to: '/platform/blueprints', icon: React.createElement(GitBranch, { className: 'w-4 h-4' }), label: 'Blueprints' },
          { to: '/platform/tasks', icon: React.createElement(ClipboardList, { className: 'w-4 h-4' }), label: 'Tasks' },
          { to: '/platform/network-canvas', icon: React.createElement(Network, { className: 'w-4 h-4' }), label: 'Network Canvas' },
        ],
      },
      {
        label: 'Security',
        items: [
          { to: '/platform/zyra', icon: React.createElement(Sparkles, { className: 'w-4 h-4' }), label: 'Zyra AI' },
          { to: '/platform/zyra/configure', icon: React.createElement(SlidersHorizontal, { className: 'w-4 h-4' }), label: 'Configure Zyra' },
          { to: '/platform/zeus/security', icon: React.createElement(ShieldCheck, { className: 'w-4 h-4' }), label: 'Zeus Security' },
          { to: '/platform/zyra/security/network-policies', icon: React.createElement(Shield, { className: 'w-4 h-4' }), label: 'VM Network Policies' },
          { to: '/platform/soc', icon: React.createElement(Radar, { className: 'w-4 h-4' }), label: 'Security Operations' },
          { to: '/platform/policy', icon: React.createElement(Lock, { className: 'w-4 h-4' }), label: 'Policy' },
        ],
      },
      {
        label: 'Operations',
        items: [
          { to: '/platform/backups', icon: React.createElement(Archive, { className: 'w-4 h-4' }), label: 'Backup & Restore' },
          { to: '/platform/fleet-snapshots', icon: React.createElement(Camera, { className: 'w-4 h-4' }), label: 'Fleet Snapshots' },
          { to: '/platform/backup-replication', icon: React.createElement(Cloud, { className: 'w-4 h-4' }), label: 'Backup Replication' },
          { to: '/platform/failure-domains', icon: React.createElement(Layers, { className: 'w-4 h-4' }), label: 'Failure Domains' },
          { to: '/platform/k8s-clusters', icon: React.createElement(Boxes, { className: 'w-4 h-4' }), label: 'Kubernetes Clusters' },
          { to: '/platform/desktop-pools', icon: React.createElement(MonitorCog, { className: 'w-4 h-4' }), label: 'Desktop Pools' },
          { to: '/platform/migration-waves', icon: React.createElement(GitBranch, { className: 'w-4 h-4' }), label: 'Migration Waves' },
          { to: '/platform/placement', icon: React.createElement(Globe, { className: 'w-4 h-4' }), label: 'Disaster Recovery' },
          { to: '/platform/ha', icon: React.createElement(ShieldCheck, { className: 'w-4 h-4' }), label: 'High Availability' },
          { to: '/platform/upgrade', icon: React.createElement(ArrowUpCircle, { className: 'w-4 h-4' }), label: 'Upgrade Matrix' },
          { to: '/platform/zyra/incidents', icon: React.createElement(AlertTriangle, { className: 'w-4 h-4' }), label: 'Incident Commander' },
          { to: '/platform/zyra/approvals', icon: React.createElement(CheckCircle2, { className: 'w-4 h-4' }), label: 'Approvals' },
          { to: '/platform/maintenance', icon: React.createElement(Wrench, { className: 'w-4 h-4' }), label: 'Maintenance' },
          { to: '/platform/recommendations', icon: React.createElement(Sparkles, { className: 'w-4 h-4' }), label: 'Recommendations' },
          { to: '/platform/notifications', icon: React.createElement(Bell, { className: 'w-4 h-4' }), label: 'Alerts' },
          { to: '/platform/alert-rules', icon: React.createElement(BellRing, { className: 'w-4 h-4' }), label: 'Alert Rules' },
          { to: '/platform/scheduled-jobs', icon: React.createElement(CalendarClock, { className: 'w-4 h-4' }), label: 'Scheduled Jobs' },
        ],
      },
      {
        label: 'Analytics',
        items: [
          { to: '/platform/observability', icon: React.createElement(BarChart2, { className: 'w-4 h-4' }), label: 'Observability' },
          { to: '/platform/activity', icon: React.createElement(Activity, { className: 'w-4 h-4' }), label: 'Activity Monitor' },
          { to: '/platform/topology', icon: React.createElement(Share2, { className: 'w-4 h-4' }), label: 'Topology' },
          { to: '/platform/zyra/rightsizing', icon: React.createElement(TrendingDown, { className: 'w-4 h-4' }), label: 'Rightsizing' },
          { to: '/platform/reports', icon: React.createElement(FileText, { className: 'w-4 h-4' }), label: 'Reports' },
          { to: '/platform/gpu', icon: React.createElement(Microchip, { className: 'w-4 h-4' }), label: 'GPU Command Center' },
        ],
      },
      {
        label: 'Administration',
        items: [
          { to: '/platform/settings', icon: React.createElement(Settings, { className: 'w-4 h-4' }), label: 'Settings' },
          { to: '/platform/users', icon: React.createElement(Users, { className: 'w-4 h-4' }), label: 'Users & Groups' },
          { to: '/platform/projects', icon: React.createElement(FolderKanban, { className: 'w-4 h-4' }), label: 'Stage Manager' },
          { to: '/platform/enroll', icon: React.createElement(PlugZap, { className: 'w-4 h-4' }), label: 'Add Host' },
          { to: '/platform/webhooks', icon: React.createElement(Webhook, { className: 'w-4 h-4' }), label: 'Webhooks' },
          { to: '/platform/api-keys', icon: React.createElement(Key, { className: 'w-4 h-4' }), label: 'API Keys' },
          { to: '/platform/marketplace', icon: React.createElement(Package, { className: 'w-4 h-4' }), label: 'Marketplace' },
          { to: '/platform/ai-providers', icon: React.createElement(BrainCircuit, { className: 'w-4 h-4' }), label: 'AI Providers' },
          { to: '/platform/enterprise', icon: React.createElement(Shield, { className: 'w-4 h-4' }), label: 'Enterprise Features' },
          { to: '/platform/developer', icon: React.createElement(Code2, { className: 'w-4 h-4' }), label: 'Developer Hub' },
          { to: '/platform/support', icon: React.createElement(LifeBuoy, { className: 'w-4 h-4' }), label: 'Support' },
          { to: '/platform/events', icon: React.createElement(ScrollText, { className: 'w-4 h-4' }), label: 'Event Log' },
        ],
      },
    ],
  },
  {
    label: 'Infrastructure',
    barIcon: Layers,
    items: [],
    menuScroll: true,
    sections: [
      {
        label: 'Storage',
        items: [
          { to: '/storage', icon: React.createElement(HardDrive, { className: 'w-4 h-4' }), label: 'Storage Pools' },
          { to: '/disk-images', icon: React.createElement(Database, { className: 'w-4 h-4' }), label: 'Disk Images' },
          { to: '/snapshots', icon: React.createElement(Camera, { className: 'w-4 h-4' }), label: 'Snapshots' },
          { to: '/backups', icon: React.createElement(Archive, { className: 'w-4 h-4' }), label: 'Host Backups' },
          { to: '/secrets', icon: React.createElement(Key, { className: 'w-4 h-4' }), label: 'Secrets' },
        ],
      },
      {
        label: 'Network',
        items: [
          { to: '/networks', icon: React.createElement(Network, { className: 'w-4 h-4' }), label: 'Networks' },
          { to: '/nwfilters', icon: React.createElement(Shield, { className: 'w-4 h-4' }), label: 'Network Filters' },
          { to: '/host-networking', icon: React.createElement(Globe, { className: 'w-4 h-4' }), label: 'Host Networking' },
        ],
      },
      {
        label: 'Containers',
        items: [
          { to: '/containers', icon: React.createElement(Box, { className: 'w-4 h-4' }), label: 'Containers' },
          { to: '/containers/pods', icon: React.createElement(Container, { className: 'w-4 h-4' }), label: 'Pods' },
        ],
      },
      {
        label: 'Kubernetes',
        items: [
          { to: '/k8s', icon: React.createElement(Boxes, { className: 'w-4 h-4' }), label: 'Kubernetes' },
          { to: '/k8s/workloads', icon: React.createElement(Box, { className: 'w-4 h-4' }), label: 'K8s Workloads' },
          { to: '/k8s/kata', icon: React.createElement(Package, { className: 'w-4 h-4' }), label: 'Kata + Cloud Hypervisor' },
        ],
      },
    ],
  },
  {
    label: 'Fleet Cloud',
    barIcon: Boxes,
    items: [],
    menuScroll: true,
    sections: [
      {
        label: 'Compute',
        items: [
          { to: '/fleet-cloud', icon: React.createElement(Cloud, { className: 'w-4 h-4' }), label: 'Overview' },
          { to: '/fleet-cloud/instances', icon: React.createElement(Server, { className: 'w-4 h-4' }), label: 'Instances' },
          { to: '/fleet-cloud/create', icon: React.createElement(Plus, { className: 'w-4 h-4' }), label: 'Create Instance' },
          { to: '/fleet-cloud/server-groups', icon: React.createElement(Boxes, { className: 'w-4 h-4' }), label: 'Server Groups' },
          { to: '/fleet-cloud/keypairs', icon: React.createElement(Key, { className: 'w-4 h-4' }), label: 'Keypairs' },
          { to: '/fleet-cloud/flavors', icon: React.createElement(Cpu, { className: 'w-4 h-4' }), label: 'Flavors' },
          { to: '/fleet-cloud/autopilot', icon: React.createElement(Activity, { className: 'w-4 h-4' }), label: 'Autopilot' },
          { to: '/fleet-cloud/chaos', icon: React.createElement(Zap, { className: 'w-4 h-4' }), label: 'Game days' },
          { to: '/fleet-cloud/preemptible', icon: React.createElement(Moon, { className: 'w-4 h-4' }), label: 'Preemptible' },
        ],
      },
      {
        label: 'Storage',
        items: [
          { to: '/fleet-cloud/volumes', icon: React.createElement(HardDrive, { className: 'w-4 h-4' }), label: 'Volumes' },
          { to: '/fleet-cloud/volume-snapshots', icon: React.createElement(Camera, { className: 'w-4 h-4' }), label: 'Volume Snapshots' },
          { to: '/fleet-cloud/images', icon: React.createElement(Package, { className: 'w-4 h-4' }), label: 'Images' },
        ],
      },
      {
        label: 'Networking',
        items: [
          { to: '/fleet-cloud/vpcs', icon: React.createElement(Cloud, { className: 'w-4 h-4' }), label: 'VPCs & elastic compute' },
          { to: '/fleet-cloud/networking', icon: React.createElement(Network, { className: 'w-4 h-4' }), label: 'Networking' },
          { to: '/fleet-cloud/security-groups', icon: React.createElement(Shield, { className: 'w-4 h-4' }), label: 'Security Groups' },
          { to: '/fleet-cloud/floating-ips', icon: React.createElement(Globe, { className: 'w-4 h-4' }), label: 'Floating IPs' },
          { to: '/fleet-cloud/load-balancers', icon: React.createElement(Scale, { className: 'w-4 h-4' }), label: 'Load Balancers' },
          { to: '/fleet-cloud/topology', icon: React.createElement(Waypoints, { className: 'w-4 h-4' }), label: 'Network Topology' },
        ],
      },
      {
        label: 'Orchestration & Identity',
        items: [
          { to: '/fleet-cloud/heat', icon: React.createElement(Flame, { className: 'w-4 h-4' }), label: 'Heat Orchestration' },
          { to: '/fleet-cloud/identity', icon: React.createElement(Users, { className: 'w-4 h-4' }), label: 'Identity' },
        ],
      },
    ],
  },
  {
    label: 'Monitoring',
    barIcon: Activity,
    items: [],
    menuScroll: true,
    sections: [
      {
        label: 'Host',
        items: [
          { to: '/node', icon: React.createElement(Cpu, { className: 'w-4 h-4' }), label: 'Host Overview' },
          { to: '/events', icon: React.createElement(Activity, { className: 'w-4 h-4' }), label: 'Live Metrics' },
          { to: '/system-check', icon: React.createElement(Stethoscope, { className: 'w-4 h-4' }), label: 'System Check' },
          { to: '/capabilities', icon: React.createElement(MonitorCog, { className: 'w-4 h-4' }), label: 'Capabilities' },
          { to: '/devices', icon: React.createElement(Usb, { className: 'w-4 h-4' }), label: 'Node Devices' },
          { to: '/services', icon: React.createElement(Cog, { className: 'w-4 h-4' }), label: 'Services' },
        ],
      },
      {
        label: 'Diagnostics',
        items: [
          { to: '/jobs', icon: React.createElement(ClipboardList, { className: 'w-4 h-4' }), label: 'Daemon Jobs' },
          { to: '/logs', icon: React.createElement(ScrollText, { className: 'w-4 h-4' }), label: 'System Logs' },
          { to: '/audit', icon: React.createElement(FileText, { className: 'w-4 h-4' }), label: 'Audit Log' },
          { to: '/admin/sessions', icon: React.createElement(Users, { className: 'w-4 h-4' }), label: 'Web Sessions', requiresRoot: true },
        ],
      },
    ],
  },
]

export const routeLabels: Record<string, string> = {
  '/': 'Dashboard',
  '/vms': 'Virtual Machines',
  '/create': 'Create VM',
  '/networks': 'Networks',
  '/storage': 'Storage',
  '/snapshots': 'Snapshots',
  '/disk-images': 'Disk Images',
  '/node': 'Host Overview',
  '/events': 'Live Metrics',
  '/system-check': 'System Check',
  '/jobs': 'Daemon Jobs',
  '/capabilities': 'Capabilities',
  '/devices': 'Node Devices',
  '/nwfilters': 'Network Filters',
  '/secrets': 'Secrets',
  '/backups': 'Host Backups',
  '/host-networking': 'Host Networking',
  '/host-ssh': 'Host SSH',
  '/k8s': 'Kubernetes',
  '/k8s/workloads': 'K8s Workloads',
  '/k8s/kata': 'Kata + Cloud Hypervisor',
  '/containers': 'Containers',
  '/containers/pods': 'Container Pods',
  '/fleet-cloud': 'Fleet Cloud',
  '/fleet-cloud/instances': 'Instances',
  '/fleet-cloud/instances/:id': 'Instance',
  '/fleet-cloud/create': 'Create Instance',
  '/fleet-cloud/images': 'Images',
  '/fleet-cloud/volumes': 'Volumes',
  '/fleet-cloud/volume-snapshots': 'Volume Snapshots',
  '/fleet-cloud/security-groups': 'Security Groups',
  '/fleet-cloud/floating-ips': 'Floating IPs',
  '/fleet-cloud/vpcs': 'VPCs & elastic compute',
  '/fleet-cloud/autopilot': 'Autopilot',
  '/fleet-cloud/chaos': 'Game days',
  '/fleet-cloud/preemptible': 'Preemptible',
  '/fleet-cloud/networking': 'Networking',
  '/fleet-cloud/load-balancers': 'Load Balancers',
  '/fleet-cloud/topology': 'Network Topology',
  '/fleet-cloud/heat': 'Heat Orchestration',
  '/fleet-cloud/identity': 'Identity',
  '/fleet-cloud/keypairs': 'Keypairs',
  '/fleet-cloud/flavors': 'Flavors',
  '/fleet-cloud/server-groups': 'Server Groups',
  '/audit': 'Audit Log',
  '/import': 'Import VM',
  '/sprites': 'Sprites',
  '/api-docs': 'API Docs',
  '/services': 'Services',
  '/logs': 'System Logs',
  '/settings': 'Settings',
  '/admin/sessions': 'Web Sessions',
  '/console': 'Console',
  '/platform': 'Mission Control',
  '/platform/vms': 'Virtual Machines',
  '/platform/applications': 'Applications',
  '/platform/hosts': 'Hosts',
  '/platform/content': 'Images & ISOs',
  '/platform/templates': 'Templates',
  '/platform/cloud-init': 'Cloud-Init Studio',
  '/platform/create-iso': 'Create ISO',
  '/platform/create-advanced': 'Advanced Create',
  '/platform/vm-builder': 'VM Builder',
  '/platform/migration': 'Migration Assistant',
  '/platform/blueprints': 'Blueprints',
  '/platform/launchpad': 'Launchpad',
  '/platform/backups': 'Backup & Restore',
  '/platform/activity': 'Activity Monitor',
  '/platform/recommendations': 'Recommendations',
  '/platform/enroll': 'Add Host',
  '/platform/placement': 'Disaster Recovery',
  '/platform/storage': 'Disk Utility',
  '/platform/storage-atlas': 'Storage (Atlas)',
  '/platform/networks': 'Networks',
  '/platform/tasks': 'Tasks',
  '/platform/events': 'Event Log',
  '/platform/alert-rules': 'Alert Rules',
  '/platform/scheduled-jobs': 'Scheduled Jobs',
  '/platform/reports': 'Reports',
  '/platform/projects': 'Stage Manager',
  '/platform/notifications': 'Alerts',
  '/platform/settings': 'Settings',
  '/platform/users': 'Users & Groups',
  '/platform/api-keys': 'API Keys',
  '/platform/webhooks': 'Webhooks',
  '/platform/maintenance': 'Maintenance',
  '/platform/soc': 'Security Operations',
  '/platform/policy': 'Policy',
  '/platform/fleet-snapshots': 'Fleet Snapshots',
  '/platform/backup-replication': 'Backup Replication',
  '/platform/failure-domains': 'Failure Domains',
  '/platform/k8s-clusters': 'Kubernetes Clusters',
  '/platform/desktop-pools': 'Desktop Pools',
  '/platform/migration-waves': 'Migration Waves',
  '/platform/topology': 'Topology',
  '/platform/observability': 'Observability',
  '/platform/gpu': 'GPU Command Center',
  '/platform/vgpu': 'vGPU Partitions',
  '/platform/network-canvas': 'Network Canvas',
  '/platform/developer': 'Developer Hub',
  '/platform/integrations': 'Integrations',
  '/platform/support': 'Support',
  '/platform/enterprise': 'Enterprise Features',
  '/platform/datacenter': 'Datacenter View',
  '/platform/ai-providers': 'AI Providers',
  '/platform/zyra/configure': 'Configure Zyra',
  '/platform/ha': 'High Availability',
  '/platform/baremetal': 'Bare Metal',
  '/platform/storage-tiers': 'Storage Tiers',
  '/platform/marketplace': 'Marketplace',
  '/platform/upgrade': 'Upgrade Matrix',
  '/fleet': 'Fleet',
  '/platform/zyra': 'Zyra OS',
  '/platform/zeus/security': 'Zeus Security',
  '/platform/zyra/rightsizing': 'Rightsizing',
  '/platform/zyra/incidents': 'Incident Commander',
  '/platform/zyra/approvals': 'Approvals',
  '/platform/zeus/security/firewall': 'Firewall',
  '/platform/zyra/security/hunt': 'Threat Hunting',
  '/platform/zyra/security/enforcement': 'Runtime Enforcement',
  '/platform/zyra/security/native-bpf': 'Native eBPF',
  '/platform/zyra/security/network-policies': 'VM Network Policies',
  '/platform/zeus/security/k8s': 'Kubernetes Security',
  '/platform/zeus/security/cloud': 'Cloud Security',
  '/platform/zeus/security/connectivity': 'Connectivity',
  '/platform/zeus/security/policies': 'Policies',
  '/platform/zeus/security/compliance': 'Compliance',
  '/platform/zeus/security/activity': 'Security Activity',
  '/platform/zeus/security/ports': 'Port Control',
  '/platform/zeus/security/services': 'Service Guard',
  '/platform/mission-control/live': 'Live Wall',
  '/platform/infrastructure': 'Infrastructure',
  '/platform/workloads': 'Workloads',
  '/platform/operations': 'Operations',
  '/platform/administration': 'Administration',
  '/platform/resources': 'Resources',
  '/platform/hosts/:id': 'Host',
  '/platform/hosts/finder': 'Machine Finder',
  '/platform/vms/:id': 'Virtual Machine',
  '/platform/vms/:id/consolehub': 'Console Hub',
  '/platform/vms/:id/console': 'Console',
  '/platform/zyra/machines/:hostId': 'Host',
  '/platform/zeus/security/firewall/:id': 'Firewall',
}

