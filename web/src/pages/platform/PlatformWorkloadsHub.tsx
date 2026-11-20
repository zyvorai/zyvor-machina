// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Server, LayoutTemplate, Camera, Boxes, Rocket, LayoutGrid } from 'lucide-react'
import PlatformPageChrome from '../../components/platform/PlatformPageChrome'
import { AppleDestinationList } from '../../components/platform/apple/AppleStoryKit'

const DESTINATIONS = [
  { to: '/platform/vms', icon: <Server className="w-5 h-5" />, title: 'Virtual Machines', subtitle: 'All fleet VMs — search, filter, manage' },
  { to: '/platform/templates', icon: <LayoutTemplate className="w-5 h-5" />, title: 'Templates', subtitle: 'Golden images and cloud-init templates' },
  { to: '/platform/fleet-snapshots', icon: <Camera className="w-5 h-5" />, title: 'Snapshots', subtitle: 'Fleet-wide snapshot management' },
  { to: '/platform/applications', icon: <Boxes className="w-5 h-5" />, title: 'Applications', subtitle: 'Deployed application stacks' },
  { to: '/platform/blueprints', icon: <Rocket className="w-5 h-5" />, title: 'Blueprints', subtitle: 'Automation shortcuts and launchpad' },
]

export default function PlatformWorkloadsHub() {
  return (
    <PlatformPageChrome
      eyebrow="Platform"
      title="Workloads"
      subtitle="Virtual machines, templates, and containerized workloads"
      icon={<LayoutGrid className="w-6 h-6 text-[var(--text-muted)]" />}
    >
      <AppleDestinationList items={DESTINATIONS} />
    </PlatformPageChrome>
  )
}
