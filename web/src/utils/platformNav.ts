// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React from 'react'
import {
  LayoutDashboard,
  Monitor,
  Server,
  Package,
  Settings,
  Cpu,
  ShieldAlert,
  Shield,
  UserPlus,
  Wrench,
  BrainCircuit,
  Cog,
  Building2,
  Home,
  Cloud,
  HardDrive,
  Network,
} from 'lucide-react'

export interface PlatformNavItem {
  to: string
  label: string
  icon: React.ReactNode
  /** Optional Go-menu subsection header (not shown in sidebar). */
  subsection?: string
}

export interface PlatformNavSection {
  label: string
  items: PlatformNavItem[]
  /** macOS Finder-style collapsible section */
  collapsible?: boolean
  defaultCollapsed?: boolean
}

const ic = (Icon: React.ComponentType<{ className?: string }>) =>
  React.createElement(Icon, { className: 'w-4 h-4' })

/** macOS 26 Tahoe–style Finder sidebar — Zeus OS 1:1 (host + platform). */
export const PLATFORM_SIDEBAR: PlatformNavSection[] = [
  {
    label: 'Favorites',
    items: [
      { to: '/platform', label: 'Mission Control', icon: ic(LayoutDashboard) },
      { to: '/', label: 'Dashboard', icon: ic(Home) },
      { to: '/vms', label: 'Virtual Machines', icon: ic(Monitor) },
      { to: '/platform/vms', label: 'Machine Finder', icon: ic(Monitor) },
      { to: '/platform/hosts', label: 'Hosts', icon: ic(Server) },
      { to: '/platform/settings', label: 'Settings', icon: ic(Settings) },
    ],
  },
  {
    label: 'Host',
    collapsible: true,
    items: [
      { to: '/vms', label: 'Virtual Machines', icon: ic(Monitor) },
      { to: '/create', label: 'Create VM', icon: ic(Cpu) },
      { to: '/networks', label: 'Networks', icon: ic(Network) },
      { to: '/storage', label: 'Storage', icon: ic(HardDrive) },
      { to: '/fleet-cloud', label: 'Fleet Cloud', icon: ic(Cloud) },
      { to: '/containers', label: 'Containers', icon: ic(Package) },
    ],
  },
  {
    label: 'Fleet',
    collapsible: true,
    items: [
      { to: '/platform/zyra', label: 'Machina Zyra OS', icon: ic(Cpu) },
      { to: '/platform/zyra/configure', label: 'Configure Zyra', icon: ic(Cog) },
      { to: '/platform/ai-providers', label: 'AI Providers', icon: ic(BrainCircuit) },
      { to: '/platform/enroll', label: 'Add Host', icon: ic(UserPlus) },
    ],
  },
  {
    label: 'Platform',
    collapsible: true,
    items: [
      { to: '/platform/datacenter', label: 'Datacenter View', icon: ic(Building2) },
      { to: '/platform/infrastructure', label: 'Infrastructure', icon: ic(Server) },
      { to: '/platform/workloads', label: 'Workloads', icon: ic(Package) },
      { to: '/platform/operations', label: 'Operations', icon: ic(Wrench) },
      { to: '/platform/upgrade', label: 'Upgrade Matrix', icon: ic(Cog) },
      { to: '/platform/administration', label: 'Administration', icon: ic(Settings) },
      { to: '/platform/zeus/security', label: 'Security Center', icon: ic(ShieldAlert) },
      { to: '/platform/zeus/security/firewall', label: 'Zeus Firewall', icon: ic(Shield) },
    ],
  },
]

/** Normal-tier sidebar favorites (flat list). */
export const NORMAL_FAVORITE_PATHS = [
  '/platform',
  '/',
  '/vms',
  '/platform/vms',
  '/platform/hosts',
  '/platform/settings',
] as const

export const PLATFORM_PAGE_LABELS: Record<string, string> = {
  '/platform': 'Mission Control',
  '/platform/vms': 'Finder',
  '/platform/applications': 'Applications',
  '/platform/hosts': 'Hosts',
  '/platform/hosts/finder': 'Machine Finder',
  '/platform/integrations': 'Apps & Integrations',
  '/platform/infrastructure': 'Infrastructure',
  '/platform/workloads': 'Workloads',
  '/platform/administration': 'Administration',
  '/platform/resources': 'Infrastructure',
  '/platform/operations': 'Operations',
  '/platform/storage': 'Disk Utility',
  '/platform/gpu': 'GPU Command Center',
  '/platform/networks': 'Networks',
  '/platform/content': 'Images & ISOs',
  '/platform/templates': 'Templates',
  '/platform/cloud-init': 'Cloud-Init Studio',
  '/platform/network-canvas': 'Network Canvas',
  '/platform/datacenter': 'Datacenter View',
  '/platform/fleet-snapshots': 'Fleet Snapshots',
  '/platform/migration': 'Migration Assistant',
  '/platform/backups': 'Backups',
  '/platform/placement': 'Disaster Recovery',
  '/platform/tasks': 'Tasks',
  '/platform/notifications': 'Alerts',
  '/platform/activity': 'Activity Monitor',
  '/platform/maintenance': 'Software Update',
  '/platform/recommendations': 'Recommendations',
  '/platform/blueprints': 'Shortcuts',
  '/platform/zyra': 'Machina Zyra OS',
  '/platform/soc': 'Security Operations Center',
  '/platform/zeus/security': 'Security Center',
  '/platform/zyra/security/hunt': 'Threat Hunting',
  '/platform/zyra/security/enforcement': 'Runtime Enforcement',
  '/platform/zyra/security/native-bpf': 'Native eBPF',
  '/platform/zyra/security/network-policies': 'VM Network Policies',
  '/platform/zeus/security/firewall': 'Zeus Firewall',
  '/platform/zeus/security/ports': 'Open Ports',
  '/platform/zeus/security/services': 'Allowed Apps',
  '/platform/zeus/security/activity': 'Firewall Activity',
  '/platform/zeus/security/compliance': 'Firewall Compliance',
  '/platform/zeus/security/k8s': 'K8s Firewall',
  '/platform/zeus/security/cloud': 'Cloud Security Groups',
  '/platform/zeus/security/connectivity': 'Connectivity Matrix',
  '/platform/topology': 'Topology',
  '/platform/observability': 'Observability',
  '/platform/developer': 'SDK & Terraform',
  '/platform/enterprise': 'Enterprise Features',
  '/platform/users': 'Users & Groups',
  '/platform/projects': 'Stage Manager',
  '/platform/reports': 'Reports',
  '/platform/support': 'Support',
  '/platform/settings': 'Settings',
  '/platform/enroll': 'Add Host',
  '/platform/events': 'Console',
  '/platform/webhooks': 'Webhooks',
  '/platform/api-keys': 'API Keys',
  '/platform/policy': 'Policy & Quotas',
  '/platform/zeus/security/policies': 'Firewall Policies',
  '/platform/ai-providers': 'AI Providers',
  '/platform/zyra/configure': 'Configure Zyra',
  '/platform/ha': 'High Availability',
  '/platform/baremetal': 'Bare Metal',
  '/platform/storage-tiers': 'Storage Tiers',
  '/platform/marketplace': 'Marketplace',
  '/platform/upgrade': 'Upgrade Matrix',
  '/platform/mission-control/live': 'Live Wall',
  '/platform/create-iso': 'Create ISO',
  '/platform/create-advanced': 'Advanced Create',
  '/platform/vm-builder': 'VM Builder',
  '/platform/zyra/rightsizing': 'Rightsizing',
  '/platform/zyra/incidents': 'Incident Commander',
  '/platform/zyra/approvals': 'Approvals',
}
